//! One bounded Cargo index entry from a single current approved policy snapshot.
use super::*;

/// Maximum complete entry returned to one project (not a per-line bound).
pub const MAX_INDEX_ENTRY_BYTES: usize = 1024 * 1024;

impl Publications {
    /// Return all published versions granted to this project for one canonical
    /// lowercase Cargo index path. Includes yanked versions with current flags.
    /// Unknown, malformed, rebound and wholly unauthorized paths share denial.
    /// Any invalid included candidate refuses the whole entry; no partial output
    /// or upstream fallback is returned. Caller must authenticate the owner and
    /// supply fresh catalog state under its catalog/registry serialization gates.
    pub fn index_entry(
        &self,
        actor: OwnerId,
        project: ProjectId,
        catalog: &Catalog,
        candidates: &PackageStore,
        registry: &HostedRegistry,
        path: &str,
    ) -> Result<Vec<u8>, Error> {
        self.actor(actor)?;
        if !catalog.projects().iter().any(|p| p.id() == project) {
            return Err(Error::Denied);
        }
        if path.len() > devoption_registry::MAX_PACKAGE_NAME_BYTES + 6 || !path.is_ascii() {
            return Err(Error::Denied);
        }
        let name = path.rsplit('/').next().ok_or(Error::Denied)?;
        // Coordinates performs the same supported Cargo-name admission as
        // generated packages; the dummy version never selects a record.
        Coordinates::new(registry.clone(), name, "0.0.0").map_err(|_| Error::Denied)?;
        if name != name.to_ascii_lowercase()
            || super::super::cargo_package::index_location(name) != path
        {
            return Err(Error::Denied);
        }
        let state = self.state()?;
        if state.events.len() >= MAX_POLICY_EVENTS {
            return Err(Error::Capacity);
        }
        let versions: Vec<_> = state
            .versions
            .iter()
            .filter(|v| {
                v.coordinates.registry() == registry
                    && v.coordinates.name().to_ascii_lowercase() == name
                    && v.readers.contains(&project)
            })
            .collect();
        if versions.is_empty() {
            return Err(Error::Denied);
        }
        for version in &versions {
            check_source(catalog, version.source, version.repository)?;
        }
        // Stream records after one complete inventory validation, keeping only
        // approved index rows. Never accumulate all source/archive bytes or rescan
        // every archive once per version. Unsafe hidden inventory still refuses.
        let mut rows = std::collections::BTreeMap::new();
        let mut total = 0;
        candidates.visit(|package| -> Result<(), Error> {
            let Some(version) = versions
                .iter()
                .find(|v| v.coordinates == *package.source().coordinates())
            else {
                return Ok(());
            };
            verify_candidate(&package, version)?;
            if package.index_path() != path {
                return Err(Error::Integrity);
            }
            let bytes = line(&package, version.yanked)?;
            if bytes.len() > MAX_INDEX_ENTRY_BYTES - total {
                return Err(Error::Capacity);
            }
            total += bytes.len();
            if rows
                .insert(version.coordinates.version().to_owned(), bytes)
                .is_some()
            {
                return Err(Error::Integrity);
            }
            Ok(())
        })?;
        if rows.len() != versions.len() {
            return Err(Error::Unavailable);
        }
        // Lexical version order is deterministic, not Cargo resolution priority.
        // Cargo interprets each row's SemVer independently.
        assemble(rows.into_values().map(Ok))
    }
}
fn assemble(lines: impl IntoIterator<Item = Result<Vec<u8>, Error>>) -> Result<Vec<u8>, Error> {
    let mut entry = Vec::new();
    for bytes in lines {
        let bytes = bytes?;
        if bytes.len() > MAX_INDEX_ENTRY_BYTES - entry.len() {
            return Err(Error::Capacity);
        }
        entry.extend(bytes);
    }
    Ok(entry)
}
pub(super) fn line(package: &CargoPackage, yanked: bool) -> Result<Vec<u8>, Error> {
    let mut line: Value =
        serde_json::from_slice(package.index_line()).map_err(|_| Error::Integrity)?;
    line["yanked"] = json!(yanked);
    let mut bytes = serde_json::to_vec(&line).map_err(|_| Error::Integrity)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests;
