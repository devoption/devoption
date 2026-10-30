//! Only a newly created private managed checkout receives this embedded template.
use super::{ManagedRoot, directory, write};
use crate::{Error, worktree::ChangeCounts};
use rustix::fs::{Mode, mkdirat};
use std::{path::Path, process::Command, time::Duration};

const FILES: &[(&str, &[u8])] = &[
    ("Cargo.toml", include_bytes!("../../../../../examples/static-site/Cargo.toml")),
    ("Cargo.lock", include_bytes!("../../../../../examples/static-site/Cargo.lock")),
    ("src/lib.rs", include_bytes!("../../../../../examples/static-site/src/lib.rs")),
    ("assets/index.html", include_bytes!("../../../../../examples/static-site/assets/index.html")),
    ("assets/site.css", include_bytes!("../../../../../examples/static-site/assets/site.css")),
    ("assets/site.js", include_bytes!("../../../../../examples/static-site/assets/site.js")),
    ("README.md", b"# Starter web app\n\nA Rust/WASIp2 web app with embedded HTML, CSS and JavaScript.\n\nOpen Builds in DevOption to inspect this committed source, review a build and explicitly approve it. No build or deployment runs when this repository is created.\n\nThe initial commit uses DevOption <devoption@localhost>.\n"),
];

pub(super) fn populate(root: &ManagedRoot, path: &Path) -> Result<(), Error> {
    let observed = root.root.inspect(path)?;
    let checkout = root.root.directory(path)?;
    for name in ["src", "assets"] {
        mkdirat(&checkout, name, Mode::RWXU)?;
    }
    for (name, bytes) in FILES {
        let selected = Path::new(name);
        let parent = match selected.parent().filter(|p| !p.as_os_str().is_empty()) {
            Some(parent) => directory(
                &checkout,
                parent.to_str().expect("fixed template directory"),
            )?,
            None => checkout.try_clone()?,
        };
        write(
            &parent,
            selected
                .file_name()
                .expect("fixed template filename")
                .to_str()
                .expect("ASCII template"),
            bytes,
        )?;
        parent.sync_all()?;
    }
    for arguments in [
        vec![
            "add",
            "--",
            "Cargo.toml",
            "Cargo.lock",
            "src/lib.rs",
            "assets/index.html",
            "assets/site.css",
            "assets/site.js",
            "README.md",
        ],
        vec![
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "feat: initialize starter web app",
        ],
    ] {
        if root.root.inspect(path)? != observed {
            return Err(Error::StaleCheckout);
        }
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "DevOption")
            .env("GIT_AUTHOR_EMAIL", "devoption@localhost")
            .env("GIT_COMMITTER_NAME", "DevOption")
            .env("GIT_COMMITTER_EMAIL", "devoption@localhost")
            .current_dir(&observed.path)
            .args([
                "--no-pager",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.attributesFile=/dev/null",
                "-c",
                "protocol.allow=never",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "gc.auto=0",
                "-c",
                "maintenance.auto=false",
                "-c",
                "core.fsync=all",
            ])
            .args(arguments);
        super::super::status::run(&mut command, Duration::from_secs(3))?;
    }
    if root.root.inspect(path)? != observed {
        return Err(Error::StaleCheckout);
    }
    let status = root.root.git_status(path)?;
    if status.commit.is_none()
        || status.branch.as_deref() != Some("main")
        || status.changes != ChangeCounts::default()
    {
        return Err(Error::CreationUncertain);
    }
    checkout.sync_all()?;
    root.directory()?.sync_all()?;
    Ok(())
}
