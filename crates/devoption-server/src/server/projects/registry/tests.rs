use super::*;
use crate::{
    owner_store::OwnerStore,
    session::{CSRF_TOKEN_BYTES, MAX_LIFETIME, Session, TOKEN_BYTES, slot::SessionSlot},
};
use axum::body::Body;
use devoption_builds::{
    BuildPlan, EnvironmentEntry, Limits, PlanInput, library::cargo_package::CargoPackage,
    source_snapshot::registered,
};
use devoption_projects::{CatalogStore, worktree::PermittedRoot};
use devoption_registry::CargoMaterialization;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
};
use tower::ServiceExt;
const TOKEN: [u8; TOKEN_BYTES] = [0xab; TOKEN_BYTES];
const CSRF: [u8; CSRF_TOKEN_BYTES] = [0xef; CSRF_TOKEN_BYTES];
const MEMBER_TOKEN: [u8; TOKEN_BYTES] = [0xcd; TOKEN_BYTES];
pub(super) fn private(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
        ])
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
pub(super) struct Fixture {
    _temp: tempfile::TempDir,
    pub(super) registry: PathBuf,
    data: PathBuf,
    pub(super) projects: Projects,
    slot: SharedSession,
    app: Router,
    pub(super) package: CargoPackage,
    pub(super) source: ProjectId,
    pub(super) consumer: ProjectId,
    owner: IdentityId,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let data = base.join("data");
        let root = base.join("repositories");
        let tree = root.join("library");
        let registry = base.join("registry");
        for dir in [
            &data,
            &root,
            &tree,
            &tree.join("src"),
            &registry,
            &registry.join("candidates"),
            &registry.join("policy"),
        ] {
            private(dir);
        }
        fs::write(tree.join("Cargo.toml"),"[package]\nname='shared-tool'\nversion='0.1.0'\nedition='2024'\nbuild=false\n[lib]\nname='shared_tool'\ncrate-type=['rlib']\n").unwrap();
        fs::write(
            tree.join("Cargo.lock"),
            "version=4\n[[package]]\nname='shared-tool'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(tree.join("src/lib.rs"), "pub fn value()->u32 {42}\n").unwrap();
        git(&tree, &["init", "-b", "main"]);
        git(
            &tree,
            &["add", "--", "Cargo.toml", "Cargo.lock", "src/lib.rs"],
        );
        git(&tree, &["commit", "-m", "fixture"]);
        let commit = git(&tree, &["rev-parse", "HEAD"]);
        let owner_store = OwnerStore::open_absolute(&data).unwrap();
        let owner =
            crate::owner_auth::setup(&owner_store, "public synthetic fixture password").unwrap();
        let (catalog, source, consumer, repository) = {
            let mut writer = CatalogStore::open_absolute(&data).unwrap();
            let source = writer.create_project("Library").unwrap();
            let consumer = writer.create_project("Consumer").unwrap();
            let repository = writer
                .attach_repository(
                    source,
                    &PermittedRoot::open_absolute(&root).unwrap(),
                    Path::new("library"),
                )
                .unwrap();
            (
                writer.load().unwrap().unwrap(),
                source,
                consumer,
                repository,
            )
        };
        let snapshot = registered::capture(
            &catalog,
            source,
            repository,
            &PermittedRoot::open_absolute(&root).unwrap(),
            &commit,
            RegistryId::new("prepared").unwrap(),
        )
        .unwrap();
        let plan = BuildPlan::validate(
            PlanInput {
                project: source,
                repository,
                commit,
                dependencies: CargoMaterialization::new(
                    snapshot.request().clone(),
                    Sha256::digest(b"empty fixture dependencies"),
                    1,
                ),
                source: snapshot,
                rust_version: "1.99.0".into(),
                cargo_executable: "/fixture/toolchain/bin/cargo".into(),
                work_directory: "/fixture/work/one".into(),
                package: "shared-tool".into(),
                binary: "shared_tool".into(),
                environment: vec![EnvironmentEntry {
                    name: "LANG".into(),
                    value: "C.UTF-8".into(),
                }],
                limits: Limits {
                    elapsed_seconds: 60,
                    log_bytes: 1024,
                },
            },
            &catalog,
        )
        .unwrap();
        let binding = HostedRegistry::new(
            RegistryId::new("team").unwrap(),
            "sparse+https://packages.example.test/index/",
        )
        .unwrap();
        let package = CargoPackage::from_plan(binding, &plan).unwrap();
        let mut candidates =
            PackageStore::open(registry.join("candidates").to_str().unwrap()).unwrap();
        candidates.stage(&package).unwrap();
        drop(candidates);
        fs::write(
            registry.join("config.toml"),
            "schema=1\nregistry='team'\nindex='sparse+https://packages.example.test/index/'\n",
        )
        .unwrap();
        fs::set_permissions(
            registry.join("config.toml"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let mut projects = Projects::open_with_root(&owner_store, Some(&root)).unwrap();
        let engine =
            Engine::open(&registry, owner, &projects.directory, &[data.clone(), root]).unwrap();
        projects.registry = Some(Arc::new(Mutex::new(engine)));
        let slot = Arc::new(SessionSlot::default());
        slot.activate(
            Session::new(owner, TOKEN, MAX_LIFETIME)
                .unwrap()
                .with_csrf(CSRF),
            &TOKEN,
        )
        .unwrap();
        slot.activate(
            Session::new(IdentityId::from_bytes([3; 16]), MEMBER_TOKEN, MAX_LIFETIME)
                .unwrap()
                .with_csrf(CSRF),
            &MEMBER_TOKEN,
        )
        .unwrap();
        projects.registry_session = Some(slot.clone());
        let app = super::super::super::protect(
            super::super::super::page_routes(
                true,
                slot.clone(),
                super::super::routes(projects.clone()),
                false,
                None,
            ),
            3000,
            slot.clone(),
        );
        Self {
            _temp: temp,
            registry,
            data,
            projects,
            slot,
            app,
            package,
            source,
            consumer,
            owner,
        }
    }
    fn url(&self, project: ProjectId, suffix: &str) -> String {
        format!(
            "/projects/{}/registry/shared-tool/0.1.0/{suffix}",
            hex(&project.to_bytes())
        )
    }
    pub(super) fn request(&self, method: &str, url: &str, body: &str, member: bool) -> Request {
        Request::builder()
            .method(method)
            .uri(url)
            .header("host", "127.0.0.1:3000")
            .header("origin", "http://127.0.0.1:3000")
            .header("content-type", "text/plain;charset=UTF-8")
            .header(
                "cookie",
                format!(
                    "__Host-devoption_session={}",
                    crate::browser_session::encode(if member { &MEMBER_TOKEN } else { &TOKEN })
                ),
            )
            .header(
                crate::browser_session::CSRF_HEADER,
                crate::browser_session::encode(&CSRF),
            )
            .body(Body::from(body.to_owned()))
            .unwrap()
    }
    pub(super) fn send_request(
        &self,
        request: Request,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let response = self.app.clone().oneshot(request).await.unwrap();
                let (parts, body) = response.into_parts();
                let bytes = axum::body::to_bytes(body, 8 * 1024 * 1024).await.unwrap();
                (parts.status, parts.headers, bytes.to_vec())
            })
    }
    fn send(
        &self,
        method: &str,
        url: &str,
        body: &str,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        self.send_request(self.request(method, url, body, false))
    }
    fn prepare(&self, body: &str) -> String {
        let (status, _, bytes) = self.send("POST", &self.url(self.source, "prepare"), body);
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        serde_json::from_slice::<Value>(&bytes).unwrap()["review_token"]
            .as_str()
            .unwrap()
            .into()
    }
    fn approve(&self, token: &str) -> StatusCode {
        self.send("POST", &self.url(self.source, "approve"), token)
            .0
    }
    fn restart(&mut self) -> io::Result<()> {
        self.app = Router::new();
        self.projects.registry = None;
        let engine = Engine::open(
            &self.registry,
            self.owner,
            &self.projects.directory,
            &[
                self.data.clone(),
                self._temp
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("repositories"),
            ],
        )?;
        self.projects.registry = Some(Arc::new(Mutex::new(engine)));
        self.slot = Arc::new(SessionSlot::default());
        self.projects.registry_session = Some(self.slot.clone());
        self.app = super::super::super::protect(
            super::super::super::page_routes(
                true,
                self.slot.clone(),
                super::super::routes(self.projects.clone()),
                false,
                None,
            ),
            3000,
            self.slot.clone(),
        );
        Ok(())
    }
    fn sign_in_fixture(&self) {
        self.slot
            .activate(
                Session::new(self.owner, TOKEN, MAX_LIFETIME)
                    .unwrap()
                    .with_csrf(CSRF),
                &TOKEN,
            )
            .unwrap();
    }
    fn publish(&self) {
        let token = self.prepare(&format!(
            "{{\"action\":\"publish\",\"readers\":[\"{}\"]}}",
            hex(&self.consumer.to_bytes())
        ));
        assert_eq!(self.approve(&token), StatusCode::OK);
    }
}

#[test]
fn guarded_review_approval_status_project_download_and_yank_flow() {
    let f = Fixture::new();
    let archive = f.url(
        f.consumer,
        &format!("archive/{}", hex(f.package.archive_sha256().bytes())),
    );
    assert_eq!(f.send("GET", &archive, "").0, StatusCode::NOT_FOUND);
    let body = format!(
        "{{\"action\":\"publish\",\"readers\":[\"{}\"]}}",
        hex(&f.consumer.to_bytes())
    );
    let token = f.prepare(&body);
    assert_eq!(f.send("GET", &archive, "").0, StatusCode::NOT_FOUND);
    assert_eq!(f.approve(&token), StatusCode::OK);
    assert_eq!(f.approve(&token), StatusCode::CONFLICT);
    let (status, headers, bytes) = f.send("GET", &archive, "");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, f.package.archive());
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(headers.contains_key(header::CONTENT_DISPOSITION));
    let (status, _, bytes) = f.send("GET", &f.url(f.source, "status"), "");
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["events_used"], 1);
    assert_eq!(value["archive_bytes"], f.package.archive().len());
    assert_eq!(
        f.send("GET", &f.url(f.consumer, "status"), "").0,
        StatusCode::NOT_FOUND
    );
    let token = f.prepare("{\"action\":\"yank\",\"yanked\":true}");
    assert_eq!(f.approve(&token), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&f.send("GET", &f.url(f.consumer, "index"), "").2).unwrap();
    assert_eq!(value["yanked"], true);
    assert_eq!(f.send("GET", &archive, "").0, StatusCode::OK);
    let token = f.prepare("{\"action\":\"readers\",\"readers\":[]}");
    assert_eq!(f.approve(&token), StatusCode::OK);
    assert_eq!(f.send("GET", &archive, "").0, StatusCode::NOT_FOUND);
}

#[test]
fn owner_session_origin_csrf_member_and_forged_claims_are_checked_before_registry_access() {
    let f = Fixture::new();
    let path = f.url(f.source, "prepare");
    let mut request = f.request(
        "POST",
        &path,
        "{\"action\":\"publish\",\"readers\":[]}",
        false,
    );
    request
        .headers_mut()
        .remove(crate::browser_session::CSRF_HEADER);
    assert_eq!(f.send_request(request).0, StatusCode::UNAUTHORIZED);
    let mut request = f.request("POST", &path, "", false);
    request
        .headers_mut()
        .insert("origin", "https://other.example.test".parse().unwrap());
    assert_eq!(f.send_request(request).0, StatusCode::FORBIDDEN);
    let mut request = f.request("POST", &path, "", true);
    request
        .headers_mut()
        .insert("x-owner", "true".parse().unwrap());
    assert_eq!(f.send_request(request).0, StatusCode::FORBIDDEN);
    let request = f.request("GET", &f.url(f.source, "status"), "", true);
    assert_eq!(f.send_request(request).0, StatusCode::FORBIDDEN);
    let mut request = f.request("GET", &f.url(f.source, "status"), "", false);
    request.headers_mut().remove("cookie");
    assert_eq!(f.send_request(request).0, StatusCode::SEE_OTHER);
    let mut request = f.request("POST", &path, "", false);
    request
        .headers_mut()
        .insert("host", "other.example.test".parse().unwrap());
    assert_eq!(f.send_request(request).0, StatusCode::BAD_REQUEST);
    f.slot.revoke_identity(f.owner);
    assert_eq!(f.send("POST", &path, "").0, StatusCode::UNAUTHORIZED);
}

#[test]
fn bounded_input_source_route_token_and_catalog_changes_refuse_mutation() {
    let f = Fixture::new();
    assert_eq!(
        f.send("POST", &f.url(f.source, "prepare"), &"x".repeat(8193))
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        f.send(
            "POST",
            &f.url(f.source, "prepare"),
            "{\"action\":\"publish\",\"readers\":[],\"owner\":true}"
        )
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.send(
            "POST",
            &f.url(f.consumer, "prepare"),
            "{\"action\":\"publish\",\"readers\":[]}"
        )
        .0,
        StatusCode::NOT_FOUND
    );
    let token = f.prepare("{\"action\":\"publish\",\"readers\":[]}");
    assert_eq!(
        f.send("POST", &f.url(f.consumer, "approve"), &token).0,
        StatusCode::CONFLICT
    );
    assert_eq!(f.approve(&token), StatusCode::CONFLICT);
    let token = f.prepare("{\"action\":\"publish\",\"readers\":[]}");
    {
        let mut writer = CatalogStore::open_absolute(&f.data).unwrap();
        writer
            .rename_project(f.consumer, "Changed consumer")
            .unwrap();
    }
    assert_eq!(f.approve(&token), StatusCode::CONFLICT);
    let guard = f.projects.registry.as_ref().unwrap().try_lock().unwrap();
    assert_eq!(
        f.send("GET", &f.url(f.source, "status"), "").0,
        StatusCode::CONFLICT
    );
    drop(guard);
}

#[test]
fn configuration_changes_and_unsafe_or_overlapping_roots_fail_closed() {
    let f = Fixture::new();
    let mut config = fs::read_to_string(f.registry.join("config.toml")).unwrap();
    config.push_str("unknown=true\n");
    fs::write(f.registry.join("config.toml"), config).unwrap();
    assert_eq!(
        f.send("GET", &f.url(f.source, "status"), "").0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(Engine::open(&f.registry, f.owner, &f.projects.directory, &[]).is_err());
    let alias = f._temp.path().join("alias");
    symlink(&f.registry, &alias).unwrap();
    assert!(Engine::open(&alias, f.owner, &f.projects.directory, &[]).is_err());
    assert!(Engine::open(&f.data, f.owner, &f.projects.directory, &[]).is_err());
}

#[test]
fn durable_policy_survives_controller_restart_and_pending_review_tokens_do_not() {
    let mut f = Fixture::new();
    f.publish();
    let token = f.prepare("{\"action\":\"yank\",\"yanked\":true}");
    f.restart().unwrap();
    assert_eq!(
        f.send("GET", &f.url(f.source, "status"), "").0,
        StatusCode::SEE_OTHER
    );
    f.sign_in_fixture();
    assert_eq!(f.approve(&token), StatusCode::CONFLICT);
    let status: Value =
        serde_json::from_slice(&f.send("GET", &f.url(f.source, "status"), "").2).unwrap();
    assert_eq!(status["events_used"], 1);
    assert_eq!(status["yanked"], false);
    assert_eq!(
        f.send("GET", &f.url(f.consumer, "index"), "").0,
        StatusCode::OK
    );
}

#[test]
fn recovery_routes_complete_the_exact_pending_revocation_after_restart() {
    for linked in [false, true] {
        let mut f = Fixture::new();
        f.publish();
        let token = f.prepare("{\"action\":\"readers\",\"readers\":[]}");
        assert_eq!(f.approve(&token), StatusCode::OK);
        let final_path = f.registry.join("policy").join(format!("{:064x}.policy", 1));
        let pending = f.registry.join("policy/.publication-policy.pending");
        if linked {
            fs::hard_link(&final_path, &pending).unwrap();
        } else {
            fs::rename(&final_path, &pending).unwrap();
        }
        f.restart().unwrap();
        f.sign_in_fixture();
        assert_eq!(
            f.send("GET", &f.url(f.source, "status"), "").0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        let (status, _, bytes) = f.send("POST", "/package-registry/recovery/prepare", "");
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["final_link_present"], linked);
        assert_eq!(value["change"]["action"], "readers");
        let token = value["review_token"].as_str().unwrap();
        assert_eq!(
            f.send("POST", "/package-registry/recovery/approve", token)
                .0,
            StatusCode::OK
        );
        assert_eq!(
            f.send("POST", "/package-registry/recovery/approve", token)
                .0,
            StatusCode::CONFLICT
        );
        assert!(!pending.exists());
        let value: Value =
            serde_json::from_slice(&f.send("GET", &f.url(f.source, "status"), "").2).unwrap();
        assert_eq!(value["events_used"], 2);
        assert_eq!(
            f.send("GET", &f.url(f.consumer, "index"), "").0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.send("GET", &f.url(f.source, "index"), "").0,
            StatusCode::OK
        );
    }
}

#[test]
fn session_is_rechecked_after_request_body_delivery_before_mutation() {
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };
    struct LogoutBody {
        slot: SharedSession,
        owner: IdentityId,
        sent: bool,
    }
    impl http_body::Body for LogoutBody {
        type Data = axum::body::Bytes;
        type Error = Infallible;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
            if self.sent {
                return Poll::Ready(None);
            }
            self.sent = true;
            self.slot.revoke_identity(self.owner);
            Poll::Ready(Some(Ok(http_body::Frame::data(
                axum::body::Bytes::from_static(b"{\"action\":\"readers\",\"readers\":[]}"),
            ))))
        }
    }
    let f = Fixture::new();
    f.publish();
    let request = f.request("POST", &f.url(f.source, "prepare"), "", false);
    let (parts, _) = request.into_parts();
    let request = Request::from_parts(
        parts,
        Body::new(LogoutBody {
            slot: f.slot.clone(),
            owner: f.owner,
            sent: false,
        }),
    );
    assert_eq!(f.send_request(request).0, StatusCode::UNAUTHORIZED);
    f.sign_in_fixture();
    assert_eq!(
        f.send("GET", &f.url(f.consumer, "index"), "").0,
        StatusCode::OK
    );
}

#[test]
fn storage_permissions_binding_and_ancestry_are_checked_independently() {
    let mut f = Fixture::new();
    assert!(Root::open(&f.registry, &f.projects.directory, &[PathBuf::from("/usr")]).is_ok());
    assert!(
        Root::open(
            &f.registry,
            &f.projects.directory,
            &[f._temp.path().canonicalize().unwrap()]
        )
        .is_err()
    );
    let alias = f._temp.path().join("valid-alias");
    symlink(&f.registry, &alias).unwrap();
    assert!(Root::open(&alias, &f.projects.directory, &[]).is_err());
    fs::set_permissions(
        f.registry.join("config.toml"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(Root::open(&f.registry, &f.projects.directory, &[]).is_err());
    fs::set_permissions(
        f.registry.join("config.toml"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    f.publish();
    let config = fs::read_to_string(f.registry.join("config.toml"))
        .unwrap()
        .replace("packages.example.test", "changed.example.test");
    fs::write(f.registry.join("config.toml"), config).unwrap();
    assert!(f.restart().is_err());
}

#[test]
fn cooperating_catalog_writer_excludes_registry_acquisition_until_fresh_reload() {
    let f = Fixture::new();
    f.publish();
    let writer = CatalogStore::open_absolute(&f.data).unwrap();
    assert_eq!(
        f.send("GET", &f.url(f.consumer, "index"), "").0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(writer);
    assert_eq!(
        f.send("GET", &f.url(f.consumer, "index"), "").0,
        StatusCode::OK
    );
}

#[test]
fn real_loopback_http_serves_only_approved_owner_project_artifacts() {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        time::Duration,
    };
    let f = Fixture::new();
    f.publish();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    // The production transport guard is configured for this exact ephemeral port;
    // its inner router is the same owner/catalog/registry fixture as other tests.
    let app = super::super::super::protect(
        super::super::super::page_routes(
            true,
            f.slot.clone(),
            super::super::routes(f.projects.clone()),
            false,
            None,
        ),
        address.port(),
        f.slot.clone(),
    );
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await
                    .unwrap();
            })
    });
    let get = |path: &str, cookie: bool| {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let cookie = if cookie {
            format!(
                "Cookie: __Host-devoption_session={}\r\n",
                crate::browser_session::encode(&TOKEN)
            )
        } else {
            String::new()
        };
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {address}\r\n{cookie}Connection: close\r\n\r\n"
        )
        .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        bytes
    };
    let path = f.url(
        f.consumer,
        &format!("archive/{}", hex(f.package.archive_sha256().bytes())),
    );
    let allowed = get(&path, true);
    let denied = get(&path, false);
    stop.send(()).unwrap();
    thread.join().unwrap();
    assert!(allowed.starts_with(b"HTTP/1.1 200"));
    let body = allowed.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(&allowed[body..], f.package.archive());
    assert!(denied.starts_with(b"HTTP/1.1 303"));
    assert!(
        !denied
            .windows(f.package.archive().len())
            .any(|w| w == f.package.archive())
    );
}

#[test]
fn complete_project_index_uses_the_existing_authenticated_admission_and_live_grants() {
    let f = Fixture::new();
    let path = format!(
        "/projects/{}/registry-index/{}",
        hex(&f.consumer.to_bytes()),
        f.package.index_path()
    );
    assert_eq!(f.send("GET", &path, "").0, StatusCode::NOT_FOUND);
    let token = f.prepare(&format!(
        "{{\"action\":\"publish\",\"readers\":[\"{}\"]}}",
        hex(&f.consumer.to_bytes())
    ));
    assert_eq!(f.approve(&token), StatusCode::OK);
    let (status, headers, bytes) = f.send("GET", &path, "");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-ndjson");
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(bytes, f.send("GET", &f.url(f.consumer, "index"), "").2);
    for tail in [
        "config.json",
        "shared-tool",
        "sh/ar/Shared-Tool",
        "sh/ar/shared_tool",
        "sh/xx/shared-tool",
        "sh/ar/unknown",
        "sh/ar/shared-tool/extra",
    ] {
        let wrong = format!(
            "/projects/{}/registry-index/{tail}",
            hex(&f.consumer.to_bytes())
        );
        assert_eq!(f.send("GET", &wrong, "").0, StatusCode::NOT_FOUND, "{tail}");
    }
    let mut anonymous = f.request("GET", &path, "", false);
    anonymous.headers_mut().remove("cookie");
    assert_eq!(f.send_request(anonymous).0, StatusCode::SEE_OTHER);
    assert_eq!(
        f.send_request(f.request("GET", &path, "", true)).0,
        StatusCode::FORBIDDEN
    );
    let guard = f.projects.gate.clone().try_lock_owned().unwrap();
    assert_eq!(f.send("GET", &path, "").0, StatusCode::CONFLICT);
    drop(guard);
    let file = fs::read_dir(f.registry.join("candidates"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "cargo"))
        .unwrap();
    let saved = fs::read(&file).unwrap();
    fs::write(&file, b"corrupt candidate").unwrap();
    let (status, _, body) = f.send("GET", &path, "");
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(!body.starts_with(b"{"));
    fs::write(&file, saved).unwrap();
    let token = f.prepare("{\"action\":\"yank\",\"yanked\":true}");
    assert_eq!(f.approve(&token), StatusCode::OK);
    let row: Value = serde_json::from_slice(&f.send("GET", &path, "").2).unwrap();
    assert_eq!(row["yanked"], true);
    let token = f.prepare("{\"action\":\"readers\",\"readers\":[]}");
    assert_eq!(f.approve(&token), StatusCode::OK);
    assert_eq!(f.send("GET", &path, "").0, StatusCode::NOT_FOUND);
    let source_path = path.replace(&hex(&f.consumer.to_bytes()), &hex(&f.source.to_bytes()));
    assert_eq!(f.send("GET", &source_path, "").0, StatusCode::OK);
    f.projects.registry.as_ref().unwrap().blocking_lock().policy = None;
    assert_eq!(
        f.send("GET", &source_path, "").0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[test]
fn consumer_dependency_review_reacquires_current_policy_and_exact_archives() {
    let f = Fixture::new();
    let root = f.registry.parent().unwrap().join("repositories");
    let tree = root.join("consumer");
    private(&tree.join("src"));
    fs::write(tree.join("Cargo.toml"),"[package]\nname='consumer'\nversion='0.1.0'\nedition='2024'\nbuild=false\n[lib]\nname='consumer'\ncrate-type=['cdylib']\n[dependencies]\nshared-tool={version='=0.1.0',registry='team'}\n").unwrap();
    fs::write(tree.join("Cargo.lock"),format!("version=4\n[[package]]\nname='consumer'\nversion='0.1.0'\ndependencies=['shared-tool']\n[[package]]\nname='shared-tool'\nversion='0.1.0'\nsource='sparse+https://packages.example.test/index/'\nchecksum='{}'\n",hex(f.package.archive_sha256().bytes()))).unwrap();
    fs::write(
        tree.join("src/lib.rs"),
        "pub fn answer()->u32 {shared_tool::value()}\n",
    )
    .unwrap();
    git(&tree, &["init", "-b", "main"]);
    git(&tree, &["add", "--all"]);
    git(&tree, &["commit", "-m", "fixture"]);
    let root = PermittedRoot::open_absolute(&root).unwrap();
    let repository = f
        .projects
        .directory
        .writer()
        .unwrap()
        .attach_repository(f.consumer, &root, Path::new("consumer"))
        .unwrap();
    let catalog = f.projects.directory.load().unwrap().unwrap();
    let engine = f.projects.registry.as_ref().unwrap();
    let source = registered::capture_with_registries(
        &catalog,
        f.consumer,
        repository,
        &root,
        &git(&tree, &["rev-parse", "HEAD"]),
        devoption_builds::execution::registry(),
        &engine.blocking_lock().bindings().unwrap(),
    )
    .unwrap();
    assert!(
        engine
            .blocking_lock()
            .prepare_dependencies(&catalog, f.consumer, &source)
            .is_err()
    );
    f.publish();
    let prepared = engine
        .blocking_lock()
        .prepare_dependencies(&catalog, f.consumer, &source)
        .unwrap();
    engine
        .blocking_lock()
        .revalidate_dependencies(&catalog, f.consumer, &source, &prepared)
        .unwrap();
    assert!(
        prepared
            .vendor
            .files()
            .any(|file| file.path().ends_with("src/lib.rs"))
    );
    let candidate = fs::read_dir(f.registry.join("candidates"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|s| s == "cargo"))
        .unwrap();
    let bytes = fs::read(&candidate).unwrap();
    fs::write(&candidate, b"corrupt").unwrap();
    assert!(
        engine
            .blocking_lock()
            .revalidate_dependencies(&catalog, f.consumer, &source, &prepared)
            .is_err()
    );
    fs::write(&candidate, bytes).unwrap();
    let token = f.prepare("{\"action\":\"readers\",\"readers\":[]}");
    assert_eq!(f.approve(&token), StatusCode::OK);
    assert!(
        engine
            .blocking_lock()
            .revalidate_dependencies(&catalog, f.consumer, &source, &prepared)
            .is_err()
    );
}

#[test]
fn operator_public_archives_are_bounded_private_and_not_symlinked() {
    let f = Fixture::new();
    let engine = f.projects.registry.as_ref().unwrap().blocking_lock();
    let request = devoption_registry::CargoDependencyRequest::new(
        RegistryId::new("prepared").unwrap(),
        Sha256::digest(b"manifest"),
        Sha256::digest(b"lock"),
        vec![devoption_registry::LockedPackage {
            origin: devoption_registry::CargoOrigin::CratesIo,
            name: "fixture".into(),
            version: "0.1.0".into(),
            archive_sha256: Sha256::digest(b"archive"),
        }],
    )
    .unwrap();
    assert!(engine.root.public_archives(&request).is_err());
    let dir = f.registry.join("public-archives");
    private(&dir);
    let path = dir.join("fixture-0.1.0.crate");
    fs::write(&path, b"archive").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        engine.root.public_archives(&request).unwrap(),
        vec![b"archive".to_vec()]
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(engine.root.public_archives(&request).is_err());
    fs::remove_file(&path).unwrap();
    symlink("/dev/null", &path).unwrap();
    assert!(engine.root.public_archives(&request).is_err());
}
