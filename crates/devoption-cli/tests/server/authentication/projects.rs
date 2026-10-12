use super::*;
use devoption_projects::{Catalog, CatalogStore};
use std::{
    fs::OpenOptions,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
};

#[path = "projects/create.rs"]
mod create;

const NAME: &str = "<script>alert('fixture')</script> & \"工具\"";
const ESCAPED: &str =
    "&lt;script&gt;alert(&#39;fixture&#39;)&lt;/script&gt; &amp; &quot;工具&quot;";

fn seed(path: &Path) -> Catalog {
    let mut catalog = Catalog::default();
    let project = catalog.create_project(NAME).unwrap();
    let first = catalog.create_repository("First fixture").unwrap();
    let second = catalog.create_repository("Second fixture").unwrap();
    catalog.associate(project, first).unwrap();
    catalog.associate(project, second).unwrap();
    CatalogStore::open_absolute(path)
        .unwrap()
        .save(&catalog)
        .unwrap();
    fs::remove_file(path.join(".projects.lock")).unwrap();
    catalog
}

fn page(address: SocketAddr, method: &str, issued: Option<&Reply>) -> String {
    let credentials = issued
        .map(|reply| {
            format!(
                "Cookie: {}\r\n",
                reply
                    .header("set-cookie")
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
            )
        })
        .unwrap_or_default();
    request(
        address,
        &format!(
            "{method} /projects HTTP/1.1\r\nHost: {address}\r\n{credentials}Connection: close\r\n\r\n"
        ),
    )
}

#[test]
fn catalog_pages_require_a_session_escape_names_and_read_fresh_snapshots() {
    let (_directory, data) = configured();
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let mut catalog = seed(&data);
    let initial = fs::read(data.join("projects.bin")).unwrap();
    let (server, address) = start_with(&data);
    for method in ["GET", "HEAD"] {
        let denied = page(address, method, None);
        assert!(denied.starts_with("HTTP/1.1 303"));
        assert!(denied.contains("location: /sign-in"));
        assert!(denied.split_once("\r\n\r\n").unwrap().1.is_empty());
    }
    let issued = sign_in(address);
    let response = page(address, "GET", Some(&issued));
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("cache-control: no-store"));
    assert!(response.contains(ESCAPED));
    assert!(!response.contains(NAME));
    assert!(response.contains("<p>2 repositories</p>"));
    assert!(response.len() < 128 * 1024);
    assert!(
        page(address, "HEAD", Some(&issued))
            .split_once("\r\n\r\n")
            .unwrap()
            .1
            .is_empty()
    );
    assert_eq!(post(address, "/projects", Some(&issued), "").status, 415);
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), initial);
    assert_eq!(fs::read_dir(&data).unwrap().count(), 2);
    catalog
        .create_project("Added outside the running server")
        .unwrap();
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&catalog)
        .unwrap();
    assert!(page(address, "GET", Some(&issued)).contains("Added outside the running server"));
    drop(server);
    let saved = fs::read(data.join("projects.bin")).unwrap();
    let (_server, address) = start_with(&data);
    assert!(page(address, "GET", Some(&issued)).starts_with("HTTP/1.1 303"));
    let response = page(address, "GET", Some(&sign_in(address)));
    assert!(response.contains(ESCAPED));
    assert!(response.contains("Added outside the running server"));
    assert!(response.contains("<p>0 repositories</p>"));
    assert_eq!(
        devoption_projects::read_catalog(&data).unwrap().unwrap(),
        catalog
    );
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
}

#[test]
fn missing_catalog_is_empty_and_unconfigured_preview_never_exposes_a_catalog() {
    let (_directory, data) = configured();
    let (_server, address) = start_with(&data);
    let response = page(address, "GET", Some(&sign_in(address)));
    assert!(response.contains("No projects yet."));
    assert!(!response.contains("aria-label=\"Saved projects\""));
    assert_eq!(fs::read_dir(&data).unwrap().count(), 1);

    let (_directory, unconfigured) = data_directory::fixture();
    seed(&unconfigured);
    let (_server, address) = start_with(&unconfigured);
    let response = page(address, "GET", None);
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("Project management is not available yet."));
    assert!(!response.contains(ESCAPED));
    assert!(!response.contains("aria-label=\"Saved projects\""));
    assert_eq!(fs::read_dir(&unconfigured).unwrap().count(), 1);
}

fn rejected_before_listening(data: &Path) {
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .args([
            "serve",
            "--port",
            &occupied.local_addr().unwrap().port().to_string(),
            "--data-dir",
        ])
        .arg(data)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("project catalog unavailable:"), "{error}");
    assert!(!error.contains(&data.to_string_lossy().to_string()));
    assert!(!error.contains("private invalid catalog"));
}

#[test]
fn invalid_unsupported_oversized_or_unsafe_catalogs_stop_before_binding_without_reset() {
    let (_directory, data) = configured();
    seed(&data);
    let path = data.join("projects.bin");
    let good = fs::read(&path).unwrap();
    let mut unsupported = good.clone();
    unsupported[8] = 255;
    for bytes in [
        b"private invalid catalog".to_vec(),
        unsupported,
        vec![0; 512 * 1024 + 1],
    ] {
        fs::write(&path, &bytes).unwrap();
        rejected_before_listening(&data);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(&data).unwrap().count(), 2);
    }
    fs::write(&path, &good).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    rejected_before_listening(&data);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    fs::remove_file(&path).unwrap();
    let outside = data.join("preserved");
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&outside)
        .unwrap()
        .write_all(&good)
        .unwrap();
    symlink(&outside, &path).unwrap();
    rejected_before_listening(&data);
    assert_eq!(fs::read(&outside).unwrap(), good);
}
