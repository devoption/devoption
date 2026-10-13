//! One preseeded .crate archive, verified against an exact dependency request.
//! No disk/network I/O, archive parsing, resolution or execution permissions.

use crate::{
    CargoDependencyRequest, CargoMaterialization, CargoOrigin, CargoRegistryProvider, Error,
    Sha256, prepare_dependencies,
};

/// Input bound for this narrow provider, not a production resource permission.
pub const MAX_SEED_BYTES: usize = 1024 * 1024;

/// Owns verified immutable bytes for one exact request. The caller must establish
/// the request/checksum's provenance separately. No mutable cache is trusted.
pub struct SeededCargoProvider {
    materialization: CargoMaterialization,
    archive: Box<[u8]>,
}

impl SeededCargoProvider {
    pub fn new(request: CargoDependencyRequest, archive: &[u8]) -> Result<Self, Error> {
        if request.packages().len() != 1 || request.packages()[0].origin != CargoOrigin::CratesIo {
            return Err(Error::UnsupportedSeed);
        }
        if archive.is_empty() {
            return Err(Error::Unavailable);
        }
        if archive.len() > MAX_SEED_BYTES {
            return Err(Error::SeedTooLarge);
        }
        let digest = Sha256::digest(archive);
        if digest != request.packages()[0].archive_sha256 {
            return Err(Error::Integrity);
        }
        Ok(Self {
            materialization: CargoMaterialization::new(request, digest, archive.len() as u64),
            archive: archive.into(),
        })
    }

    /// The bundle is exactly the raw archive bytes, with no index or configuration.
    /// Consumer context must be established separately; these bytes grant no right
    /// to unpack or run a package. The test consumer uses Cargo's local registry.
    pub fn bundle(&self, request: &CargoDependencyRequest) -> Result<SeededBundle<'_>, Error> {
        Ok(SeededBundle {
            materialization: prepare_dependencies(self, request)?,
            archive: &self.archive,
        })
    }
}

impl CargoRegistryProvider for SeededCargoProvider {
    fn materialize(&self, request: &CargoDependencyRequest) -> Result<CargoMaterialization, Error> {
        if request != self.materialization.request() {
            return Err(Error::MismatchedRequest);
        }
        Ok(self.materialization.clone())
    }
}

/// Read-only bytes tied to their actual digest/size and checked request.
pub struct SeededBundle<'a> {
    materialization: CargoMaterialization,
    archive: &'a [u8],
}

impl SeededBundle<'_> {
    pub fn materialization(&self) -> &CargoMaterialization {
        &self.materialization
    }

    pub fn archive(&self) -> &[u8] {
        self.archive
    }
}
