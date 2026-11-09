//! Private durable Cargo candidates; no publication, serving or access grant.
use super::{
    MAX_PACKAGE_BYTES,
    cargo_package::{CargoPackage, MAX_INDEX_LINE_BYTES},
    record_store::{Record, RecordStore},
    staging::{Coordinates, Error, StageOutcome},
};

pub const MAX_STORED_PACKAGES: usize = super::record_store::MAX_RECORDS;

/// Distinct from source staging. An occupied version never changes, including its
/// index metadata and source provenance. Callers must separately authorize source
/// selection and destination; storage is neither publication nor build approval.
pub struct PackageStore(RecordStore<CargoPackage>);
impl PackageStore {
    pub fn open(path: &str) -> Result<Self, Error> {
        RecordStore::open(path).map(Self)
    }
    pub fn stage(&mut self, package: &CargoPackage) -> Result<StageOutcome, Error> {
        self.0.stage(package)
    }
    pub fn read(&self, coordinates: &Coordinates) -> Result<Option<CargoPackage>, Error> {
        self.0.read(coordinates)
    }
    /// Operator-selected exact complete candidate only. Refuses partial/corrupt
    /// evidence and conflicts, retaining files for investigation. This completes
    /// a private storage write, never publication or a registry index update.
    pub fn recover(path: &str, expected: &CargoPackage) -> Result<StageOutcome, Error> {
        RecordStore::recover(path, expected)
    }
}
impl Record for CargoPackage {
    const LOCK: &'static str = ".cargo-candidates.lock";
    const PENDING: &'static str = ".cargo-candidates.pending";
    const SUFFIX: &'static str = ".cargo";
    const MAX_BYTES: usize = 2 * MAX_PACKAGE_BYTES + MAX_INDEX_LINE_BYTES + 8192;
    fn coordinates(&self) -> &Coordinates {
        self.source().coordinates()
    }
    fn encode(&self) -> Vec<u8> {
        self.encode()
    }
    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        Self::decode(bytes)
    }
}

#[cfg(test)]
mod tests;
