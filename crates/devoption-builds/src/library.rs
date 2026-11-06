//! Canonical source packaging from verified bytes, never compiler output or disk.
use crate::source_snapshot::SourceSnapshot;
use devoption_registry::cargo_inputs::SourceTarget;
use std::io;

pub(crate) const COMPLETION: &[u8] = b"devoption-library-build-v1\n";
pub const MAX_PACKAGE_BYTES: usize = 4 * 1024 * 1024;

/// Produce a deterministic uncompressed POSIX ustar source archive. This is a
/// local artifact, not a published Cargo registry `.crate` or portable rlib.
/// All verified files are included; no filesystem reads or ignore rules apply.
pub fn source_package(source: &SourceSnapshot) -> io::Result<Vec<u8>> {
    if !matches!(source.target(), SourceTarget::Rlib { .. }) {
        return Err(rejected());
    }
    let root = format!("{}-{}", source.package(), source.version());
    let mut archive = Vec::new();
    for file in source.files() {
        let path = format!("{root}/{}", file.path());
        let (prefix, name) = if path.len() <= 100 {
            ("", path.as_str())
        } else {
            path.match_indices('/')
                .rev()
                .find_map(|(at, _)| {
                    let (prefix, name) = (&path[..at], &path[at + 1..]);
                    (prefix.len() <= 155 && name.len() <= 100).then_some((prefix, name))
                })
                .ok_or_else(rejected)?
        };
        let padded = file.bytes().len().div_ceil(512) * 512;
        if archive.len() + 512 + padded + 1024 > MAX_PACKAGE_BYTES {
            return Err(rejected());
        }
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        octal(&mut header[100..108], 0o644)?;
        octal(&mut header[108..116], 0)?;
        octal(&mut header[116..124], 0)?;
        octal(&mut header[124..136], file.bytes().len())?;
        octal(&mut header[136..148], 0)?;
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        let checksum = header.iter().map(|&byte| usize::from(byte)).sum();
        octal(&mut header[148..155], checksum)?;
        header[155] = b' ';
        archive.extend_from_slice(&header);
        archive.extend_from_slice(file.bytes());
        archive.resize(archive.len() + padded - file.bytes().len(), 0);
    }
    archive.resize(archive.len() + 1024, 0);
    Ok(archive)
}

fn octal(field: &mut [u8], value: usize) -> io::Result<()> {
    let encoded = format!("{:0width$o}", value, width = field.len() - 1);
    if encoded.len() != field.len() - 1 {
        return Err(rejected());
    }
    field[..encoded.len()].copy_from_slice(encoded.as_bytes());
    Ok(())
}

fn rejected() -> io::Error {
    io::Error::other("library source package requires a bounded rlib snapshot and ustar paths")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_snapshot::tests::ProofFixture;

    #[test]
    fn deterministic_archive_preserves_exact_files_and_checksums() {
        let source = ProofFixture::library("rlib").verify().unwrap();
        let archive = source_package(&source).unwrap();
        assert_eq!(archive, source_package(&source).unwrap());
        let mut at = 0;
        for file in source.files() {
            let header = &archive[at..at + 512];
            let expected = format!("ExampleService-0.1.0/{}", file.path());
            assert_eq!(&header[..expected.len()], expected.as_bytes());
            assert_eq!(&header[257..265], b"ustar\x0000");
            let mut sum_header = header.to_vec();
            sum_header[148..156].fill(b' ');
            let actual: usize = sum_header.iter().map(|&byte| usize::from(byte)).sum();
            assert_eq!(
                std::str::from_utf8(&header[148..154]).unwrap(),
                format!("{actual:06o}")
            );
            at += 512;
            assert_eq!(&archive[at..at + file.bytes().len()], file.bytes());
            at += file.bytes().len().div_ceil(512) * 512;
        }
        assert_eq!(&archive[at..], &[0; 1024]);
        assert!(source_package(&ProofFixture::cdylib().verify().unwrap()).is_err());
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use crate::{
        BuildPlan, EnvironmentEntry,
        approvals::{Approvals, OwnerId},
        jobs::{BuildJob, JobId},
        review_context::{Policy, ReviewContext},
        source_snapshot::tests::ProofFixture,
    };

    fn plan() -> BuildPlan {
        let (catalog, mut input) = crate::tests::fixture();
        input.source = ProofFixture::library("rlib").verify().unwrap();
        input.commit = input.source.commit().into();
        input.dependencies = devoption_registry::CargoMaterialization::new(
            input.source.request().clone(),
            devoption_registry::Sha256::digest(b"vendor"),
            1,
        );
        input.package = "ExampleService".into();
        input.binary = "ExampleService".into();
        input.rust_version = "1.99.0".into();
        input.cargo_executable = "/toolchain/bin/cargo".into();
        input.limits.elapsed_seconds = 60;
        input.limits.log_bytes = 1_048_576;
        input.environment = [
            ("PATH", "/toolchain/bin:/usr/bin"),
            ("RUSTC", "/toolchain/bin/rustc"),
            ("HOME", "/job/home"),
            ("CARGO_HOME", "/job/cargo"),
            ("CARGO_TARGET_DIR", "/job/target"),
            ("TMPDIR", "/job/tmp"),
            ("LANG", "C.UTF-8"),
            ("CARGO_NET_OFFLINE", "true"),
            ("CARGO_INCREMENTAL", "0"),
            ("CARGO_BUILD_JOBS", "1"),
            ("RUSTC_WRAPPER", ""),
            ("RUSTC_WORKSPACE_WRAPPER", ""),
        ]
        .into_iter()
        .map(|(name, value)| EnvironmentEntry {
            name: name.into(),
            value: value.into(),
        })
        .collect();
        BuildPlan::validate(input, &catalog).unwrap()
    }

    #[test]
    fn dedicated_policy_rejects_cross_profile_and_environment_substitution() {
        let plan = plan();
        let mut input = crate::review_context::tests::input(&plan);
        input.rustc.path = "/toolchain/bin/rustc".into();
        input.policy = Policy::LibraryBuildV1;
        let context = ReviewContext::validate(&plan, input.clone()).unwrap();
        for policy in [Policy::WasmBuildV1, Policy::InitialBuildV1] {
            let mut wrong = input.clone();
            wrong.policy = policy;
            assert!(ReviewContext::validate(&plan, wrong).is_err());
        }
        let mut wrong = plan.clone();
        wrong.0.environment[0].value = "/other/bin".into();
        assert!(ReviewContext::validate(&wrong, input).is_err());
        let job = BuildJob::queued(JobId::from_bytes([4; 16]), plan.clone());
        let owner = OwnerId::from_bytes([1; 16]);
        let approvals = Approvals::default();
        let approval = approvals.approve(owner, &job, context.clone()).unwrap();
        // A changed source rev cannot consume the original review action.
        let mut changed = plan;
        changed.0.source = ProofFixture::changed_source().verify().unwrap();
        let changed = BuildJob::queued(job.id(), changed);
        assert!(
            approvals
                .consume(&approval, owner, &changed, &context)
                .is_err()
        );
        assert!(approvals.consume(&approval, owner, &job, &context).is_err());
    }

    #[test]
    fn source_package_rejects_unrepresentable_paths_and_archive_overhead() {
        let source = ProofFixture::library("rlib").verify().unwrap();
        let mut files: Vec<_> = source
            .files()
            .iter()
            .map(|f| (f.path().to_owned(), f.bytes().to_vec()))
            .collect();
        files.push((
            format!(
                "{}/{}/{}/asset",
                "a".repeat(100),
                "b".repeat(100),
                "c".repeat(100)
            ),
            b"asset".to_vec(),
        ));
        let refs: Vec<_> = files
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_slice()))
            .collect();
        let snapshot = ProofFixture::with_files(&refs).verify().unwrap();
        assert!(source_package(&snapshot).is_err());
        files.pop();
        for i in 0..120 {
            files.push((format!("asset-{i:03}"), vec![b'x'; 34400]));
        }
        let refs: Vec<_> = files
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_slice()))
            .collect();
        let snapshot = ProofFixture::with_files(&refs).verify().unwrap();
        assert!(source_package(&snapshot).is_err());
    }
}
