//! Pinned, operator-supplied SAML metadata. No remote metadata fetch or email linking.
use crate::{identity::IdentityId, owner_store::OwnerStore};
use rustix::fs::{AtFlags, Mode, OFlags, openat, renameat, unlinkat};
use saml_rs::{
    AcsEndpoint, AudienceValidationPolicy, EntityId, IdpDescriptor, InMemoryReplayCache,
    MetadataTrustPolicy, NameIdFormat, PendingAuthnRequest, ResponseSignaturePolicy, Saml, Sp,
    SpConfig, SpValidationPolicy, XmlSignatureProfile,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    sync::Mutex,
    time::{Duration, Instant},
};

pub(crate) const RECORD: &str = "saml.json";
pub(crate) const SESSION_LIFETIME: Duration = Duration::from_secs(15 * 60);
pub(crate) const FLOW_LIFETIME: Duration = Duration::from_secs(5 * 60);
const MAX_BYTES: u64 = 128 * 1024;
pub(crate) fn invalid() -> io::Error {
    io::Error::other("SAML configuration or authentication unavailable")
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    pub version: u8,
    pub revision: u64,
    pub enabled: bool,
    pub local_auth: bool,
    pub idp_entity_id: String,
    pub idp_metadata: String,
    pub group_attribute: String,
    pub required_group: String,
    pub verified: bool,
    pub verified_origin: Option<String>,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            enabled: false,
            local_auth: true,
            idp_entity_id: String::new(),
            idp_metadata: String::new(),
            group_attribute: String::new(),
            required_group: String::new(),
            verified: false,
            verified_origin: None,
        }
    }
}
impl Configuration {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != 1
            || (!self.local_auth
                && (!self.enabled || !self.verified || self.verified_origin.is_none()))
            || self.group_attribute.is_empty() != self.required_group.is_empty()
            || self.idp_metadata.len() > 65536
        {
            return Err(invalid());
        }
        for value in [
            &self.idp_entity_id,
            &self.group_attribute,
            &self.required_group,
        ] {
            if value.len() > 2048 || value.chars().any(char::is_control) {
                return Err(invalid());
            }
        }
        if !self.idp_metadata.is_empty() {
            self.idp()?;
        } else if self.enabled {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn verified_for(&self, origin: Option<&str>) -> bool {
        self.verified && origin.is_some() && self.verified_origin.as_deref() == origin
    }
    pub fn validate_origin(&self, origin: Option<&str>) -> io::Result<()> {
        if (self.enabled && origin.is_none()) || (!self.local_auth && !self.verified_for(origin)) {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn idp(&self) -> io::Result<IdpDescriptor> {
        // Trust comes from authenticated operator import, not an unverified remote URL.
        IdpDescriptor::from_metadata_xml_for(
            EntityId::try_new(&self.idp_entity_id).map_err(|_| invalid())?,
            &self.idp_metadata,
            MetadataTrustPolicy::UnsignedForCompatibility,
        )
        .map_err(|_| invalid())
    }
}
pub(crate) fn sp(origin: &str) -> io::Result<Saml<Sp>> {
    Saml::sp(
        SpConfig::builder(
            EntityId::try_new(format!("{origin}/auth/saml/metadata")).map_err(|_| invalid())?,
        )
        .acs_endpoint(AcsEndpoint::post(format!("{origin}/auth/saml/acs")).map_err(|_| invalid())?)
        .name_id_format(NameIdFormat::Persistent)
        .validation(SpValidationPolicy {
            responses: ResponseSignaturePolicy::RequireSigned,
            xml_signatures: XmlSignatureProfile::StrictRsaSha2,
            audience: AudienceValidationPolicy::Validate,
            ..SpValidationPolicy::recommended()
        })
        .build()
        .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())
}
pub(crate) struct Flow {
    pub relay: String,
    pub cookie: [u8; 32],
    pub pending: PendingAuthnRequest,
    pub expires: Instant,
}
pub(crate) struct State {
    pub configuration: Configuration,
    pub flows: Vec<Flow>,
    pub replay: InMemoryReplayCache,
}
pub(crate) struct Store {
    directory: File,
    pub state: Mutex<State>,
}
fn private(file: &File) -> io::Result<()> {
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err(invalid());
    }
    Ok(())
}
fn read(directory: &File) -> io::Result<Option<Configuration>> {
    let file = match openat(
        directory,
        RECORD,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => File::from(fd),
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    private(&file)?;
    if file.metadata()?.len() > MAX_BYTES {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let c: Configuration = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    c.validate()?;
    Ok(Some(c))
}
impl Store {
    pub fn open(owner: &OwnerStore) -> io::Result<Self> {
        let directory = owner.duplicate_directory()?;
        let configuration = read(&directory)?.unwrap_or_default();
        Ok(Self {
            directory,
            state: Mutex::new(State {
                configuration,
                flows: Vec::new(),
                replay: InMemoryReplayCache::new().with_max_entries(4096),
            }),
        })
    }
    pub fn local_auth(&self) -> bool {
        self.state.lock().is_ok_and(|s| s.configuration.local_auth)
    }
    // Serialize with state and the existing authentication worker. Retain the owner directory.
    pub fn save(&self, c: &Configuration) -> io::Result<()> {
        c.validate()?;
        read(&self.directory)?;
        let bytes = serde_json::to_vec(c).map_err(|_| invalid())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        let m = self.directory.metadata()?;
        if m.uid() != rustix::process::geteuid().as_raw() || m.mode() & 0o077 != 0 || m.nlink() == 0
        {
            return Err(invalid());
        }
        let name = format!(".saml-{}.tmp", crate::browser_session::encode(&random()?));
        let mut f = File::from(openat(
            &self.directory,
            name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        let result = (|| {
            f.write_all(&bytes)?;
            f.sync_all()?;
            renameat(&self.directory, name.as_str(), &self.directory, RECORD)?;
            self.directory.sync_all()
        })();
        let _ = unlinkat(&self.directory, name.as_str(), AtFlags::empty());
        result
    }
}
pub(crate) fn random() -> io::Result<[u8; 32]> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| invalid())?;
    Ok(bytes)
}
pub(crate) fn identity(issuer: &str, subject: &str) -> IdentityId {
    let hash = Sha256::digest(format!("devoption-saml\0{issuer}\0{subject}"));
    IdentityId::from_bytes(hash[..16].try_into().expect("fixed digest length"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[test]
    fn storage_is_private_durable_and_never_resets_unsafe_or_corrupt_configuration() {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let owner = OwnerStore::open(directory.path()).unwrap();
        let store = Store::open(&owner).unwrap();
        let c = Configuration::default();
        store.save(&c).unwrap();
        let path = directory.path().join(RECORD);
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(Store::open(&owner).unwrap().local_auth());
        let valid = std::fs::read(&path).unwrap();
        for invalid in [b"{}".as_slice(), b"null", b"not json"] {
            std::fs::write(&path, invalid).unwrap();
            assert!(Store::open(&owner).is_err());
            assert!(store.save(&c).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), invalid);
        }
        std::fs::write(&path, valid).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Store::open(&owner).is_err());
        assert!(store.save(&c).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let alias = directory.path().join("alias");
        std::fs::hard_link(&path, &alias).unwrap();
        assert!(Store::open(&owner).is_err());
        assert!(store.save(&c).is_err());
        std::fs::remove_file(&path).unwrap();
        symlink(&alias, &path).unwrap();
        assert!(Store::open(&owner).is_err());
        assert!(store.save(&c).is_err());
    }
    #[test]
    fn identity_namespace_and_configuration_invariants_are_explicit() {
        assert_eq!(identity("issuer", "subject"), identity("issuer", "subject"));
        assert_ne!(identity("issuer", "subject"), identity("other", "subject"));
        assert_ne!(identity("issuer", "subject"), identity("issuer", "other"));
        let mut c = Configuration {
            local_auth: false,
            ..Configuration::default()
        };
        assert!(c.validate().is_err());
        c.local_auth = true;
        c.required_group = "operators".into();
        assert!(c.validate().is_err());
        c.required_group.clear();
        c.idp_metadata = "<!DOCTYPE a [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><a>&x;</a>".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn verification_cannot_be_reused_for_another_public_origin() {
        let mut c = Configuration {
            enabled: true,
            local_auth: false,
            verified: true,
            verified_origin: Some("https://platform.example.test".into()),
            ..Configuration::default()
        };
        assert!(
            c.validate_origin(Some("https://platform.example.test"))
                .is_ok()
        );
        assert!(
            c.validate_origin(Some("https://other.example.test"))
                .is_err()
        );
        assert!(c.validate_origin(None).is_err());
        c.local_auth = true;
        assert!(
            c.validate_origin(Some("https://other.example.test"))
                .is_ok()
        );
        assert!(!c.verified_for(Some("https://other.example.test")));
    }
}
