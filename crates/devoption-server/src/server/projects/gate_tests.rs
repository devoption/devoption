//! Controlled interleavings, not wall-clock latency benchmarks.
use super::*;
use crate::owner_store::OwnerStore;
use devoption_projects::{CatalogStore, RepositoryId};
use std::{
    fs,
    future::{Future, poll_fn},
    os::unix::fs::PermissionsExt,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::Poll,
    time::Duration,
};
use tokio::sync::oneshot;

struct Fixture {
    _temporary: tempfile::TempDir,
    projects: Projects,
    project: ProjectId,
    repository: RepositoryId,
    tree: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let base = temporary.path().canonicalize().unwrap();
        let data = base.join("data");
        let root = base.join("repositories");
        let tree = root.join("app");
        for path in [
            &data,
            &root,
            &tree,
            &tree.join(".git"),
            &tree.join(".git/objects"),
            &tree.join(".git/refs"),
        ] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(tree.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(
            tree.join(".git/config"),
            "[core]\nrepositoryformatversion = 0\nbare = false\n",
        )
        .unwrap();
        let owner = OwnerStore::open_absolute(&data).unwrap();
        let project;
        let repository;
        {
            let mut writer = CatalogStore::open_absolute(&data).unwrap();
            project = writer.create_project("Before").unwrap();
            repository = writer
                .attach_repository(
                    project,
                    &PermittedRoot::open_absolute(&root).unwrap(),
                    Path::new("app"),
                )
                .unwrap();
        }
        Self {
            _temporary: temporary,
            projects: Projects::open_with_root(&owner, Some(&root)).unwrap(),
            project,
            repository,
            tree,
        }
    }
}

// Poll once to enqueue a waiter and prove it cannot run yet. No sleeps or
// scheduler timing assumptions: access must reach the held mutex and suspend.
async fn queued<T>(mut future: Pin<&mut impl Future<Output = T>>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[test]
fn inspection_holds_gate_after_caller_cancellation_and_releases_on_success_or_failure() {
    for failed in [false, true] {
        let f = Fixture::new();
        if failed {
            fs::remove_file(f.tree.join(".git/HEAD")).unwrap();
        }
        Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (entered, started) = oneshot::channel();
                let (release, wait) = mpsc::channel();
                let root = f.projects.root.clone().unwrap();
                let (project, repository) = (f.project, f.repository);
                let first = tokio::spawn(f.projects.clone().access(move |directory| {
                    let result = directory
                        .load()?
                        .unwrap()
                        .repository_status(project, repository, &root);
                    assert_eq!(result.is_err(), failed);
                    entered.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(10)).unwrap();
                    result.map(|_| ())
                }));
                started.await.unwrap();
                // Read-only inspection has the process gate, but no disk writer lock.
                drop(f.projects.directory.writer().unwrap());
                let cancelled_entered = Arc::new(AtomicBool::new(false));
                let entered = cancelled_entered.clone();
                let mut cancelled = Box::pin(f.projects.clone().access(move |_| {
                    entered.store(true, Ordering::SeqCst);
                    Ok(())
                }));
                queued(cancelled.as_mut()).await;
                drop(cancelled);
                let read = f.projects.clone().access(|directory| {
                    Ok(directory.load()?.unwrap().projects()[0].name().to_owned())
                });
                let write = f.projects.clone().access(move |directory| {
                    let mut writer = directory.writer()?;
                    let mut catalog = writer.load()?.unwrap();
                    catalog.rename_project(project, "After")?;
                    writer.save(&catalog)
                });
                let (mut read, mut write) = (std::pin::pin!(read), std::pin::pin!(write));
                queued(read.as_mut()).await;
                queued(write.as_mut()).await;
                first.abort();
                assert!(first.await.unwrap_err().is_cancelled());
                assert!(f.projects.gate.try_lock().is_err());
                queued(read.as_mut()).await;
                queued(write.as_mut()).await;
                release.send(()).unwrap();
                // FIFO readers observe the pre-write snapshot; the queued mutation
                // survives both failed inspection and cancellation of its caller.
                assert_eq!(read.await.unwrap(), "Before");
                write.await.unwrap();
                assert!(!cancelled_entered.load(Ordering::SeqCst));
                assert_eq!(
                    f.projects.directory.load().unwrap().unwrap().projects()[0].name(),
                    "After"
                );
                assert!(f.projects.gate.try_lock().is_ok());
            });
    }
}

#[test]
fn queued_inspection_loads_fresh_associations_and_revalidates_checkout_identity() {
    for changed_catalog in [false, true] {
        let f = Fixture::new();
        Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (entered, started) = oneshot::channel();
                let (release, wait) = mpsc::channel();
                let tree = f.tree.clone();
                let mutation = tokio::spawn(f.projects.clone().access(move |directory| {
                    entered.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(10)).unwrap();
                    if changed_catalog {
                        // A valid replacement snapshot removes the old association.
                        // This is a fixture mutation, not a new HTTP deletion API.
                        directory.writer()?.save(&Catalog::default())?;
                    } else {
                        let retained = tree.with_file_name("retained");
                        fs::rename(&tree, &retained)?;
                        fs::create_dir(&tree)?;
                        fs::set_permissions(&tree, fs::Permissions::from_mode(0o700))?;
                        fs::rename(retained.join(".git"), tree.join(".git"))?;
                    }
                    Ok(())
                }));
                started.await.unwrap();
                let root = f.projects.root.clone().unwrap();
                let (project, repository) = (f.project, f.repository);
                let inspect = f.projects.clone().access(move |directory| {
                    directory
                        .load()?
                        .unwrap()
                        .repository_status(project, repository, &root)
                });
                let mut inspect = std::pin::pin!(inspect);
                queued(inspect.as_mut()).await;
                release.send(()).unwrap();
                mutation.await.unwrap().unwrap();
                let result = inspect.await;
                if changed_catalog {
                    assert!(matches!(result, Err(Error::UnknownProject)));
                } else {
                    assert!(matches!(result, Err(Error::StaleCheckout)));
                }
                // A failed inspection never strands the gate or its next mutation.
                f.projects
                    .clone()
                    .access(|directory| {
                        let mut writer = directory.writer()?;
                        let mut catalog = writer.load()?.unwrap();
                        catalog.create_project("After failure")?;
                        writer.save(&catalog)
                    })
                    .await
                    .unwrap();
            });
    }
}
