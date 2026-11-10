//! Supported dependency-manifest profile, checked against exact lock edges.
use super::*;
use semver::{Version, VersionReq};
use toml::{Table, Value};
const PUBLIC: &str = "https://github.com/rust-lang/crates.io-index";

pub(super) fn check(
    files: &BTreeMap<String, Vec<u8>>,
    package: &LockedPackage,
    edges: &[LockedPackage],
    request: &CargoDependencyRequest,
) -> Result<(), Error> {
    let bytes = files.get("Cargo.toml").ok_or(Error::Invalid)?;
    if bytes.len() > devoption_registry::cargo_inputs::MAX_MANIFEST_BYTES {
        return Err(Error::Capacity);
    }
    let manifest: Table = std::str::from_utf8(bytes)
        .map_err(|_| Error::Invalid)?
        .parse()
        .map_err(|_| Error::Invalid)?;
    if manifest.keys().any(|k| {
        ![
            "package",
            "lib",
            "bin",
            "example",
            "test",
            "bench",
            "dependencies",
            "dev-dependencies",
            "build-dependencies",
            "target",
            "features",
            "lints",
            "badges",
        ]
        .contains(&k.as_str())
    }) {
        return Err(Error::Invalid);
    }
    let metadata = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or(Error::Invalid)?;
    if metadata.get("name").and_then(Value::as_str) != Some(package.name.as_str())
        || metadata.get("version").and_then(Value::as_str) != Some(package.version.as_str())
    {
        return Err(Error::Integrity);
    }
    if metadata.contains_key("links")
        || metadata.contains_key("workspace")
        || !matches!(
            metadata.get("edition").and_then(Value::as_str),
            Some("2015" | "2018" | "2021" | "2024")
        )
    {
        return Err(Error::Invalid);
    }
    match metadata.get("build") {
        Some(Value::Boolean(false)) => {}
        None if !files.contains_key("build.rs") => {}
        _ => return Err(Error::Invalid),
    }
    let library = match manifest.get("lib") {
        Some(v) => Some(v.as_table().ok_or(Error::Invalid)?),
        None => None,
    };
    if library.is_none()
        && metadata
            .get("autolib")
            .is_some_and(|v| v.as_bool() == Some(false))
    {
        return Err(Error::Invalid);
    }
    if let Some(lib) = library {
        if lib.keys().any(|k| {
            ![
                "name",
                "path",
                "crate-type",
                "proc-macro",
                "test",
                "bench",
                "doc",
                "doctest",
                "harness",
            ]
            .contains(&k.as_str())
        }) {
            return Err(Error::Invalid);
        }
        if lib
            .get("proc-macro")
            .is_some_and(|v| v.as_bool() != Some(false))
        {
            return Err(Error::Invalid);
        }
        if let Some(types) = lib.get("crate-type")
            && types.as_array().map(Vec::as_slice)
                != Some([Value::String("rlib".into())].as_slice())
        {
            return Err(Error::Invalid);
        }
    }
    let path = library
        .and_then(|l| l.get("path"))
        .map(|v| v.as_str().ok_or(Error::Invalid))
        .transpose()?
        .unwrap_or("src/lib.rs");
    safe_path(path)?;
    if !files.contains_key(path) {
        return Err(Error::Invalid);
    }
    if let Some(features) = manifest.get("features") {
        for value in features.as_table().ok_or(Error::Invalid)?.values() {
            if value.as_array().is_none_or(|v| !v.is_empty()) {
                return Err(Error::Invalid);
            }
        }
    }
    empty(manifest.get("build-dependencies"))?;
    if let Some(targets) = manifest.get("target") {
        for target in targets.as_table().ok_or(Error::Invalid)?.values() {
            let t = target.as_table().ok_or(Error::Invalid)?;
            if t.keys().any(|k| {
                !["dependencies", "dev-dependencies", "build-dependencies"].contains(&k.as_str())
            }) {
                return Err(Error::Invalid);
            }
            empty(t.get("dependencies"))?;
            empty(t.get("build-dependencies"))?;
        }
    }
    let mut found = BTreeSet::new();
    if let Some(dependencies) = manifest.get("dependencies") {
        for (alias, value) in dependencies.as_table().ok_or(Error::Invalid)? {
            let (name, requirement, origin, optional) = match value {
                Value::String(req) if package.origin == CargoOrigin::CratesIo => {
                    (alias.as_str(), req.as_str(), CargoOrigin::CratesIo, false)
                }
                Value::Table(dep) => {
                    if dep.keys().any(|k| {
                        ![
                            "version",
                            "package",
                            "registry-index",
                            "default-features",
                            "optional",
                            "features",
                        ]
                        .contains(&k.as_str())
                    }) {
                        return Err(Error::Invalid);
                    }
                    for key in ["optional", "default-features"] {
                        if dep.get(key).is_some_and(|v| v.as_bool().is_none()) {
                            return Err(Error::Invalid);
                        }
                    }
                    if dep
                        .get("features")
                        .is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()))
                    {
                        return Err(Error::Invalid);
                    }
                    let origin = match dep.get("registry-index") {
                        None if package.origin == CargoOrigin::CratesIo => CargoOrigin::CratesIo,
                        Some(Value::String(index)) if index == PUBLIC => CargoOrigin::CratesIo,
                        Some(Value::String(index)) => CargoOrigin::DevOption(
                            request
                                .hosted_registries()
                                .bindings()
                                .iter()
                                .find(|b| b.index() == index)
                                .ok_or(Error::Invalid)?
                                .id()
                                .clone(),
                        ),
                        _ => return Err(Error::Invalid),
                    };
                    (
                        dep.get("package")
                            .map(|v| v.as_str().ok_or(Error::Invalid))
                            .transpose()?
                            .unwrap_or(alias),
                        dep.get("version")
                            .and_then(Value::as_str)
                            .ok_or(Error::Invalid)?,
                        origin,
                        dep.get("optional")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    )
                }
                _ => return Err(Error::Invalid),
            };
            let req = VersionReq::parse(requirement).map_err(|_| Error::Invalid)?;
            let matches: Vec<_> = edges
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.name == name
                        && p.origin == origin
                        && Version::parse(&p.version).is_ok_and(|v| req.matches(&v))
                })
                .collect();
            if optional {
                // This profile enables no features: an optional edge in the
                // active lock graph needs a future explicit feature profile.
                if !matches.is_empty() {
                    return Err(Error::Invalid);
                }
                continue;
            }
            let [(at, _)] = matches.as_slice() else {
                return Err(Error::Integrity);
            };
            if !found.insert(*at) {
                return Err(Error::Integrity);
            }
        }
    }
    if found.len() != edges.len() {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn empty(value: Option<&Value>) -> Result<(), Error> {
    if value.is_some_and(|v| v.as_table().is_none_or(|t| !t.is_empty())) {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}
