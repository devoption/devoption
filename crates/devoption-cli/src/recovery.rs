//! Local-only two-step catalog recovery; serving never invokes this command.
use devoption_projects::{CatalogDirectory, worktree::PermittedRoot};
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

pub fn run(args: &[OsString]) -> ExitCode {
    match recover(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("error: {message}");
            ExitCode::from(code)
        }
    }
}

fn recover(mut args: &[OsString]) -> Result<(), (u8, &'static str)> {
    let mut data = None;
    let mut root = None;
    let mut approval = None;
    let invalid = (
        2,
        "use recover-repositories --data-dir ABSOLUTE_PATH --repository-root ABSOLUTE_PATH [--approve-device-rebind 64HEX]",
    );
    while !args.is_empty() {
        let [flag, value, rest @ ..] = args else {
            return Err(invalid);
        };
        if flag == "--data-dir" && data.is_none() {
            data = Some(PathBuf::from(value));
        } else if flag == "--repository-root" && root.is_none() {
            root = Some(PathBuf::from(value));
        } else if flag == "--approve-device-rebind" && approval.is_none() {
            let value = value.to_str().ok_or(invalid)?;
            if value.len() != 64
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid);
            }
            let mut bytes = [0; 32];
            for (i, byte) in bytes.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|_| invalid)?;
            }
            approval = Some(bytes);
        } else {
            return Err(invalid);
        }
        args = rest;
    }
    let (Some(data), Some(root)) = (data, root) else {
        return Err(invalid);
    };
    if !data.is_absolute() || !root.is_absolute() {
        return Err(invalid);
    }
    let failed = (
        1,
        "repository recovery refused; catalog/root checks failed or review is stale. No automatic repair. Keep any backup and inspect state before retrying.",
    );
    let directory = CatalogDirectory::open_absolute(&data).map_err(|_| failed)?;
    let root = PermittedRoot::open_absolute(&root).map_err(|_| failed)?;
    if let Some(approval) = approval {
        let backup = directory
            .writer()
            .map_err(|_| failed)?
            .approve_device_rebind(&root, approval)
            .map_err(|_| failed)?;
        println!(
            "Repository device binding updated. Backup: {:?}",
            data.join(backup)
        );
        println!("Start the server with its original repository-root configuration.");
    } else {
        let review = directory.review_device_rebind(&root).map_err(|_| failed)?;
        println!("Read-only device-rebind review. Data directory: {data:?}");
        println!(
            "Same paths/inodes do NOT prove the same filesystem. Verify the intended mounted volume and all repositories independently before approving this new trust binding. Stop the server first."
        );
        for change in review.changes() {
            println!(
                "{:?}: device/inode {:?} -> {:?}",
                change.location, change.previous, change.current
            );
        }
        println!("Backup on approval: {:?}", data.join(review.backup_name()));
        let token: String = review
            .fingerprint()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        println!("Review fingerprint: {token}");
        println!("Apply only after explicit approval: rerun with --approve-device-rebind {token}");
    }
    Ok(())
}
