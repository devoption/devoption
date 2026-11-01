//! Operator-owned client allowlist and bounded private audit sink. No persisted tokens.
use super::*;
use rustix::fs::{Mode, OFlags, openat};
use serde::Deserialize;
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct Client {
    pub client_id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    clients: Vec<Client>,
}
pub(in crate::server) struct Configuration {
    pub(super) clients: Vec<Client>,
    pub(super) audit: File,
}
fn private(file: &File) -> io::Result<()> {
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err(io::Error::other(
            "MCP file must be private, owned and singly linked",
        ));
    }
    Ok(())
}
impl Configuration {
    pub(in crate::server) fn open(
        owner: &crate::owner_store::OwnerStore,
    ) -> io::Result<Option<Self>> {
        let directory = owner.duplicate_directory().map_err(io::Error::other)?;
        let file = match openat(
            &directory,
            "mcp-clients.json",
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        private(&file)?;
        if file.metadata()?.len() > 32768 {
            return Err(io::Error::other("MCP configuration too large"));
        }
        let mut bytes = Vec::new();
        file.take(32769).read_to_end(&mut bytes)?;
        let clients = parse(&bytes)?;
        let audit = File::from(openat(
            &directory,
            "mcp-audit.jsonl",
            OFlags::WRONLY
                | OFlags::APPEND
                | OFlags::CREATE
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        private(&audit)?;
        Ok(Some(Self { clients, audit }))
    }
}
fn parse(bytes: &[u8]) -> io::Result<Vec<Client>> {
    let invalid = || io::Error::other("invalid MCP client allowlist");
    if bytes.len() > 32768 {
        return Err(invalid());
    }
    let doc: Document = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if doc.version != 1 || doc.clients.is_empty() || doc.clients.len() > 16 {
        return Err(invalid());
    }
    for (i, c) in doc.clients.iter().enumerate() {
        if !identifier(&c.client_id)
            || c.name.is_empty()
            || c.name.len() > 80
            || c.name.chars().any(char::is_control)
            || c.redirect_uris.is_empty()
            || c.redirect_uris.len() > 8
            || doc.clients[..i].iter().any(|p| p.client_id == c.client_id)
        {
            return Err(invalid());
        }
        for uri in &c.redirect_uris {
            let u = url::Url::parse(uri).map_err(|_| invalid())?;
            let loopback = u.scheme() == "http"
                && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
            if uri.len() > 2048
                || u.host_str().is_none()
                || (!loopback && u.scheme() != "https")
                || !u.username().is_empty()
                || u.password().is_some()
                || u.fragment().is_some()
                || u.as_str() != uri
            {
                return Err(invalid());
            }
        }
    }
    Ok(doc.clients)
}
pub(super) fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
pub(super) fn audit(
    file: &mut File,
    identity: Option<IdentityId>,
    client: &str,
    operation: &str,
    outcome: &str,
) -> Result<(), StatusCode> {
    let result = (|| -> io::Result<()> {
        private(file)?;
        let event = json!({"at": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(io::Error::other)?.as_secs(), "user": identity.map(|id| hex(&id.to_bytes())), "client": client, "operation": operation, "outcome": outcome});
        let mut bytes = serde_json::to_vec(&event)?;
        bytes.push(b'\n');
        if file.metadata()?.len().saturating_add(bytes.len() as u64) > 8 * 1024 * 1024 {
            return Err(io::Error::other("MCP audit full"));
        }
        file.write_all(&bytes)?;
        file.sync_data()
    })();
    result.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_requires_exact_safe_callbacks_and_bounded_unique_clients() {
        for uri in [
            "https://client.example/callback",
            "http://127.0.0.1:1234/callback",
        ] {
            assert!(parse(&serde_json::to_vec(&json!({"version":1,"clients":[{"client_id":"test","name":"Test","redirect_uris":[uri]}]})).unwrap()).is_ok());
        }
        for uri in [
            "http://remote.example/callback",
            "https://user@client.example/",
            "https://client.example/#fragment",
            "file:///tmp/test",
            "https://client.example",
        ] {
            assert!(parse(&serde_json::to_vec(&json!({"version":1,"clients":[{"client_id":"test","name":"Test","redirect_uris":[uri]}]})).unwrap()).is_err(), "{uri}");
        }
    }
}
