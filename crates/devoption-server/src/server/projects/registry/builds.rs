//! Current policy and bytes for the existing build review, never a cached grant.
use super::*;
use devoption_builds::{
    library::{
        publication::dependencies::{LockedArchives, PublicArchive},
        vendor::VendorInputs,
    },
    source_snapshot::SourceSnapshot,
};
use devoption_registry::{CargoOrigin, HostedRegistries};
pub(in crate::server::projects) struct Prepared {
    captured: LockedArchives,
    pub vendor: Arc<VendorInputs>,
}
impl Engine {
    pub(in crate::server::projects) fn bindings(&self) -> Result<HostedRegistries, StatusCode> {
        self.root
            .verify()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        HostedRegistries::new(vec![self.root.binding.clone()])
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
    }
    pub(in crate::server::projects) fn prepare_dependencies(
        &self,
        catalog: &Catalog,
        project: ProjectId,
        source: &SourceSnapshot,
    ) -> Result<Prepared, StatusCode> {
        self.root
            .verify()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let policy = self
            .policy
            .as_ref()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let bytes = self
            .root
            .public_archives(source.request())
            .map_err(|_| {
                eprintln!("Registry dependency preparation: public archives unavailable or checksum mismatch; verify operator-supplied archives against the committed lockfile.");
                StatusCode::CONFLICT
            })?;
        let public: Vec<_> = source
            .request()
            .packages()
            .iter()
            .filter(|p| p.origin == CargoOrigin::CratesIo)
            .zip(&bytes)
            .map(|(package, bytes)| PublicArchive { package, bytes })
            .collect();
        let captured = policy
            .locked_archives(
                self.owner,
                project,
                catalog,
                &self.candidates,
                source,
                &public,
            )
            .map_err(|error| {
                eprintln!("Registry dependency preparation: {error}; check current publication, consumer reader grants and committed source, then prepare a fresh review.");
                StatusCode::CONFLICT
            })?;
        let vendor = Arc::new(
            policy
                .vendor_inputs(self.owner, catalog, &self.candidates, source, &captured)
                .map_err(|error| {
                    eprintln!("Registry dependency preparation: {error}; verify archive integrity, complete lock edges and the supported manifest or pinned WASIp2 closure policy (docs/cargo-vendor-bundles.md).");
                    StatusCode::CONFLICT
                })?,
        );
        self.root
            .verify()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        Ok(Prepared { captured, vendor })
    }
    pub(in crate::server::projects) fn revalidate_dependencies(
        &self,
        catalog: &Catalog,
        project: ProjectId,
        source: &SourceSnapshot,
        prepared: &Prepared,
    ) -> Result<(), StatusCode> {
        let current = self.prepare_dependencies(catalog, project, source)?;
        if current.captured.fingerprint() != prepared.captured.fingerprint()
            || current.vendor.materialization() != prepared.vendor.materialization()
        {
            return Err(StatusCode::CONFLICT);
        }
        prepared
            .vendor
            .revalidate(
                self.policy
                    .as_ref()
                    .ok_or(StatusCode::SERVICE_UNAVAILABLE)?,
                self.owner,
                catalog,
                &self.candidates,
                source,
                &prepared.captured,
            )
            .map_err(|_| StatusCode::CONFLICT)
    }
}
