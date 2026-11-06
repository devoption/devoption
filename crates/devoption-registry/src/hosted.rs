//! Explicit registry names and canonical Cargo index identities, never access grants.
use crate::RegistryId;
use std::fmt;
use url::Url;

pub const MAX_HOSTED_REGISTRIES: usize = 16;
pub const MAX_INDEX_BYTES: usize = 2048;

/// The manifest registry name is also its logical DevOption origin identity.
/// No Debug implementation implicitly logs a private registry name or endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct HostedRegistry {
    id: RegistryId,
    index: String,
    lock_source: String,
}
impl HostedRegistry {
    pub fn new(id: RegistryId, index: &str) -> Result<Self, Error> {
        if id.as_str() == "crates-io" || index.len() > MAX_INDEX_BYTES {
            return Err(Error::InvalidBinding);
        }
        let (raw, sparse) = match index.strip_prefix("sparse+") {
            Some(raw) => (raw, true),
            None => (index, false),
        };
        let url = Url::parse(raw).map_err(|_| Error::InvalidBinding)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_some_and(|host| host.ends_with('.'))
            || url.query().is_some()
            || url.fragment().is_some()
            || url.as_str() != raw
            || raw.contains('%')
            || !url
                .path()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
            || url.path().contains("//")
            || (sparse && !raw.ends_with('/'))
            || matches!(url.host_str(), Some("crates.io" | "index.crates.io"))
            || (url.host_str() == Some("github.com")
                && [
                    "/rust-lang/crates.io-index",
                    "/rust-lang/crates.io-index.git",
                ]
                .iter()
                .any(|known| url.path().trim_end_matches('/').eq_ignore_ascii_case(known)))
        {
            return Err(Error::InvalidBinding);
        }
        let lock_source = if sparse {
            index.to_owned()
        } else {
            format!("registry+{index}")
        };
        Ok(Self {
            id,
            index: index.into(),
            lock_source,
        })
    }
    pub fn id(&self) -> &RegistryId {
        &self.id
    }
    pub fn index(&self) -> &str {
        &self.index
    }
    pub fn lock_source(&self) -> &str {
        &self.lock_source
    }
}

/// Owned normalized binding set supplied by a trusted integration. Empty means
/// deny hosted sources. Construction performs no discovery, network or auth.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct HostedRegistries(Vec<HostedRegistry>);
impl HostedRegistries {
    pub fn new(mut bindings: Vec<HostedRegistry>) -> Result<Self, Error> {
        if bindings.len() > MAX_HOSTED_REGISTRIES {
            return Err(Error::Capacity);
        }
        bindings.sort_by(|a, b| a.id.cmp(&b.id));
        for (i, binding) in bindings.iter().enumerate() {
            if bindings[..i]
                .iter()
                .any(|old| old.id == binding.id || old.index == binding.index)
            {
                return Err(Error::DuplicateBinding);
            }
        }
        Ok(Self(bindings))
    }
    pub fn bindings(&self) -> &[HostedRegistry] {
        &self.0
    }
    pub(crate) fn named(&self, name: &str) -> Option<&HostedRegistry> {
        self.0.iter().find(|b| b.id.as_str() == name)
    }
    pub(crate) fn source(&self, source: &str) -> Option<&HostedRegistry> {
        self.0.iter().find(|b| b.lock_source == source)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidBinding,
    DuplicateBinding,
    Capacity,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidBinding => "hosted registry requires a distinct canonical HTTPS index identity without credentials",
            Self::DuplicateBinding => "hosted registry names and index identities must be unique",
            Self::Capacity => "hosted registry binding capacity exceeded",
        })
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding(name: &str, index: &str) -> HostedRegistry {
        HostedRegistry::new(RegistryId::new(name).unwrap(), index).unwrap()
    }
    #[test]
    fn cargo_git_and_sparse_index_identities_are_distinct_and_owned() {
        let git = binding("team", "https://packages.example.test/index");
        let sparse = binding("team", "sparse+https://packages.example.test/index/");
        assert_eq!(
            git.lock_source(),
            "registry+https://packages.example.test/index"
        );
        assert_eq!(sparse.lock_source(), sparse.index());
        assert!(git != sparse);
        assert!(HostedRegistries::default().bindings().is_empty());
        let set = HostedRegistries::new(vec![
            sparse.clone(),
            binding("other", "https://other.example.test/index"),
        ])
        .unwrap();
        assert_eq!(set.bindings()[0].id().as_str(), "other");
        assert!(
            set == HostedRegistries::new(vec![
                binding("other", "https://other.example.test/index"),
                sparse
            ])
            .unwrap()
        );
    }
    #[test]
    fn credentials_aliases_reserved_sources_and_noncanonical_urls_are_rejected() {
        for index in [
            "http://packages.example.test/index",
            "file:///tmp/index",
            "ssh://packages.example.test/index",
            "https://user:secret@packages.example.test/index",
            "https://@packages.example.test/index",
            "https://packages.example.test/index?token=secret",
            "https://packages.example.test/index#fragment",
            "https://PACKAGES.example.test/index",
            "https://packages.example.test./index",
            "https://github.com/RUST-LANG/CRATES.IO-INDEX",
            "https://packages.example.test:443/index",
            "https://packages.example.test/../index",
            "https://packages.example.test/%69ndex",
            "https://packages.example.test/in dex",
            "https://packages.example.test//index",
            "sparse+https://packages.example.test/index",
            "registry+https://packages.example.test/index",
            "https://github.com/rust-lang/crates.io-index",
            "https://github.com/rust-lang/crates.io-index.git/",
            "sparse+https://index.crates.io/",
            "https://crates.io/index",
            "https://packages.example.test/😃",
        ] {
            assert!(
                HostedRegistry::new(RegistryId::new("team").unwrap(), index).is_err(),
                "{index}"
            );
        }
        assert!(
            HostedRegistry::new(
                RegistryId::new("crates-io").unwrap(),
                "https://packages.example.test/index"
            )
            .is_err()
        );
        assert!(
            HostedRegistry::new(
                RegistryId::new("team").unwrap(),
                &format!(
                    "https://packages.example.test/{}",
                    "a".repeat(MAX_INDEX_BYTES)
                )
            )
            .is_err()
        );
    }
    #[test]
    fn duplicate_names_endpoints_and_excess_bindings_fail_closed() {
        let b = binding("team", "https://packages.example.test/index");
        for other in [
            binding("team", "https://other.example.test/index"),
            binding("other", b.index()),
        ] {
            assert_eq!(
                HostedRegistries::new(vec![b.clone(), other]).err(),
                Some(Error::DuplicateBinding)
            );
        }
        let bindings = (0..=MAX_HOSTED_REGISTRIES)
            .map(|n| {
                binding(
                    &format!("team{n}"),
                    &format!("https://packages{n}.example.test/index"),
                )
            })
            .collect();
        assert_eq!(HostedRegistries::new(bindings).err(), Some(Error::Capacity));
    }
}
