//! Audited public static-site closure, authenticated as complete upstream archives.
//!
//! This is deliberately not a feature resolver or a general build-script policy.
//! Root manifests cannot request features. The exact outgoing graph keeps optional
//! proc macros, async, rustc workspace and bitflags integrations inactive. Cargo's
//! locked/offline feature selection uses these unchanged upstream manifests.
use super::*;

const PINS: [(&str, &str, &str); 3] = [
    (
        "wasip2",
        "1.0.1+wasi-0.2.4",
        "0562428422c63773dad2c345a1882263bbf4d65cf3f42e90921f787ef5ad58e7",
    ),
    (
        "wit-bindgen",
        "0.46.0",
        "f17a85883d4e6d00e8a97c586de764dabcc06133f7f1d55dce5cdc070ad7fe59",
    ),
    (
        "bitflags",
        "2.13.2",
        "3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06",
    ),
];

fn pinned(package: &LockedPackage, at: usize) -> bool {
    let (name, version, checksum) = PINS[at];
    package.origin == CargoOrigin::CratesIo
        && package.name == name
        && package.version == version
        && hex(package.archive_sha256) == checksum
}

/// `false` selects the ordinary restricted manifest validator. A recognized
/// public coordinate never falls back after a checksum or graph mismatch.
pub(super) fn check(
    archive: &[u8],
    package: &LockedPackage,
    edges: &[LockedPackage],
) -> Result<bool, Error> {
    let Some(at) = PINS.iter().position(|(name, version, _)| {
        package.origin == CargoOrigin::CratesIo
            && package.name == *name
            && package.version == *version
    }) else {
        return Ok(false);
    };
    if !pinned(package, at) || Sha256::digest(archive) != package.archive_sha256 {
        return Err(Error::Integrity);
    }
    // wasip2 defaults select std and wit-bindgen/bitflags, with wit-bindgen's
    // defaults disabled. Its only active optional edge is bitflags. The exact
    // wit-bindgen archive includes build.rs and src/rt/libwit_bindgen_cabi.a;
    // that script copies/links this allocator and has no build dependencies.
    let valid = match at {
        0 => edges.len() == 1 && pinned(&edges[0], 1),
        1 => edges.len() == 1 && pinned(&edges[0], 2),
        2 => edges.is_empty(),
        _ => unreachable!(),
    };
    if !valid {
        return Err(Error::Invalid);
    }
    Ok(true)
}

/// The allocator runtime is admitted only through wasip2's audited dependency
/// declaration (which disables wit-bindgen defaults). Direct/default wit-bindgen
/// would activate macros/async, even if a supplied lock falsely omits their edges.
pub(super) fn context(
    root: &[u8],
    graph: &[devoption_registry::cargo_inputs::LockedDependencies],
) -> Result<(), Error> {
    let manifest: toml::Table = std::str::from_utf8(root)
        .map_err(|_| Error::Invalid)?
        .parse()
        .map_err(|_| Error::Invalid)?;
    if let Some(dep) = manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|deps| deps.get("wit-bindgen"))
        && (dep.is_str() || dep.as_table().is_some_and(|t| !t.contains_key("registry")))
    {
        return Err(Error::Invalid);
    }
    for node in graph {
        if node.dependencies().iter().any(|p| pinned(p, 1)) && !pinned(node.package(), 0) {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}
