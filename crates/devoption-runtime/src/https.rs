//! Host-owned authorization for one fixed HTTPS POST endpoint. Never Debug.
use super::*;
use serde::Deserialize;
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::Path,
};
use zeroize::Zeroizing;

const REQUEST: usize = 32 * 1024;
const RESPONSE: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(3);
type Reply = Result<
    (
        Response<WasiBody>,
        Box<dyn Future<Output = Result<(), Error>> + Send>,
    ),
    Error,
>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    project: String,
    environment: String,
    secret: String,
    revision: u64,
    endpoint: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    policy: Policy,
    addresses: Vec<Ipv4Addr>,
    value: Zeroizing<String>,
}
pub(crate) struct Grant {
    packet: Packet,
    client: reqwest::Client,
}

fn public(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..128).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && (b == 168 || (b == 0 && (c == 0 || c == 2))))
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113))
}
fn endpoint(value: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(value).ok()?;
    let host = url.host_str()?;
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.query().is_none()
        && url.fragment().is_none()
        && host.len() <= 253
        && host.contains('.')
        && host
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'-')
        && host
            .split('.')
            .all(|s| !s.is_empty() && s.len() <= 63 && !s.starts_with('-') && !s.ends_with('-'))
        && host.parse::<std::net::IpAddr>().is_err()
        && value == url.as_str()
        && url.path().len() <= 256
        && url
            .path()
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/-_".contains(&c)))
    .then_some(url)
}
impl Grant {
    pub(crate) fn load(path: &Path) -> wasmtime::Result<Self> {
        use std::io::Read;
        let file = std::fs::File::open(path)
            .map_err(|_| wasmtime::Error::msg("HTTPS credential unavailable"))?;
        wasmtime::ensure!(
            file.metadata()?.is_file() && file.metadata()?.len() <= 32768,
            "invalid HTTPS credential"
        );
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(32769).read_to_end(&mut bytes)?;
        let packet: Packet = serde_json::from_slice(&bytes)
            .map_err(|_| wasmtime::Error::msg("invalid HTTPS credential"))?;
        let url = endpoint(&packet.policy.endpoint)
            .ok_or_else(|| wasmtime::Error::msg("invalid HTTPS endpoint"))?;
        let policy = &packet.policy;
        wasmtime::ensure!(
            policy.project.len() == 32
                && policy
                    .project
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                && ["development", "preview", "production"].contains(&policy.environment.as_str())
                && !policy.secret.is_empty()
                && policy.secret.len() <= 64
                && policy
                    .secret
                    .bytes()
                    .enumerate()
                    .all(|(i, c)| c.is_ascii_uppercase()
                        || c == b'_'
                        || (i > 0 && c.is_ascii_digit()))
                && policy.revision > 0
                && !packet.value.is_empty()
                && packet.value.len() <= 8192
                && packet.value.bytes().all(|c| (33..=126).contains(&c))
                && !packet.addresses.is_empty()
                && packet.addresses.len() <= 16
                && packet.addresses.iter().copied().all(public),
            "invalid HTTPS grant"
        );
        let addresses: Vec<_> = packet
            .addresses
            .iter()
            .map(|ip| SocketAddr::from((*ip, 443)))
            .collect();
        let client = client_builder(&url, &addresses)
            .build()
            .map_err(|_| wasmtime::Error::msg("HTTPS client unavailable"))?;
        Ok(Self { packet, client })
    }
    async fn send(
        &self,
        request: Request<WasiBody>,
        body_finished: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Reply {
        let uri = request.uri();
        let url = endpoint(&self.packet.policy.endpoint).ok_or(Error::HttpRequestDenied)?;
        if request.method() != hyper::Method::POST || uri.to_string() != self.packet.policy.endpoint
        {
            return Err(Error::HttpRequestDenied);
        }
        let headers = request.headers();
        if headers.get("content-type").map(|v| v.as_bytes()) != Some(b"application/json".as_slice())
            || headers.iter().any(|(name, _)| {
                !["host", "content-length", "content-type", "idempotency-key"]
                    .contains(&name.as_str())
            })
        {
            return Err(Error::HttpRequestDenied);
        }
        let retry = headers
            .get("idempotency-key")
            .ok_or(Error::HttpRequestDenied)?
            .clone();
        if retry.as_bytes().is_empty()
            || retry.as_bytes().len() > 128
            || !retry
                .as_bytes()
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || b"._:/-".contains(c))
        {
            return Err(Error::HttpRequestDenied);
        }
        let bytes = Limited::new(request.into_body(), REQUEST)
            .collect()
            .await
            .map_err(|_| Error::HttpRequestDenied)?
            .to_bytes();
        Box::into_pin(body_finished).await?;
        if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
            return Err(Error::HttpRequestDenied);
        }
        let mut response = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .header("idempotency-key", retry)
            .bearer_auth(self.packet.value.as_str())
            .body(bytes)
            .send()
            .await
            .map_err(|_| Error::ConnectionTerminated)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(Error::HttpRequestDenied);
        }
        if response
            .headers()
            .iter()
            .map(|(n, v)| n.as_str().len() + v.as_bytes().len())
            .sum::<usize>()
            > 16 * 1024
        {
            return Err(Error::HttpRequestDenied);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::ConnectionTerminated)?
        {
            if bytes.len() + chunk.len() > RESPONSE {
                return Err(Error::HttpRequestDenied);
            }
            bytes.extend_from_slice(&chunk);
        }
        // Never return an accidental literal credential echo from the trusted provider.
        if bytes
            .windows(self.packet.value.len())
            .any(|w| w == self.packet.value.as_bytes())
        {
            return Err(Error::HttpRequestDenied);
        }
        let response = Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(
                Full::new(Bytes::from(bytes))
                    .map_err(|never| match never {})
                    .boxed_unsync(),
            )
            .map_err(|_| Error::HttpProtocolError)?;
        Ok((response, Box::new(async { Ok(()) })))
    }
}
fn client_builder(url: &reqwest::Url, addresses: &[SocketAddr]) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(TIMEOUT)
        .connect_timeout(Duration::from_secs(2))
        .pool_max_idle_per_host(0)
        .resolve_to_addrs(url.host_str().expect("validated host"), addresses)
}
#[derive(Default)]
pub(crate) struct Hooks {
    pub(crate) grant: Option<Arc<Grant>>,
    used: bool,
}
impl Hooks {
    pub(crate) fn new(grant: Option<Arc<Grant>>) -> Self {
        Self { grant, used: false }
    }
}
impl WasiHttpHooks for Hooks {
    fn is_forbidden_header(&mut self, name: &hyper::header::HeaderName) -> bool {
        wasmtime_wasi_http::DEFAULT_FORBIDDEN_HEADERS.contains(name)
            || ["authorization", "cookie", "proxy-authorization"].contains(&name.as_str())
    }
    fn send_request(
        &mut self,
        request: Request<WasiBody>,
        _: Option<RequestOptions>,
        finished: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Box<dyn Future<Output = Reply> + Send> {
        let grant = if self.used {
            None
        } else {
            self.used = true;
            self.grant.clone()
        };
        Box::new(async move {
            let grant = grant.ok_or(Error::HttpRequestDenied)?;
            tokio::time::timeout(TIMEOUT, grant.send(request, finished))
                .await
                .map_err(|_| Error::ConnectionTimeout)?
        })
    }
    fn p2_outgoing_body_chunk_size(&mut self) -> usize {
        16 * 1024
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_and_addresses_fail_closed() {
        assert!(endpoint("https://api.example.com/events").is_some());
        for url in [
            "http://api.example.com/events",
            "https://127.0.0.1/events",
            "https://user@api.example.com/events",
            "https://api.example.com/events?q=x",
            "https://api.example.com/events#x",
            "https://api.example.com:444/events",
            "https://api.example.com/a/../events",
            "https://api.example.com/%2fprivate",
        ] {
            assert!(endpoint(url).is_none(), "{url}");
        }
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "192.0.2.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!public(ip.parse().unwrap()), "{ip}");
        }
        assert!(public("1.1.1.1".parse().unwrap()));
    }
}

#[cfg(test)]
mod proof {
    use super::*;
    #[tokio::test]
    #[ignore = "explicit real WASIp2 and disposable TLS endpoint proof; no production grants"]
    async fn manual_https_component() {
        let component = std::fs::read(
            std::env::var_os("DEVOPTION_HTTPS_PROOF_COMPONENT").expect("explicit component"),
        )
        .unwrap();
        let ca =
            std::fs::read(std::env::var_os("DEVOPTION_HTTPS_PROOF_CA").expect("explicit test CA"))
                .unwrap();
        let address: SocketAddr = std::env::var("DEVOPTION_HTTPS_PROOF_ADDRESS")
            .expect("explicit test endpoint")
            .parse()
            .unwrap();
        assert!(address.ip().is_loopback());
        let url = endpoint("https://api.example.test/events").unwrap();
        // Test-only transport injection. Production Grant::load rejects loopback
        // and has no additional-CA or resolver-override configuration surface.
        let packet = || Packet {
            policy: Policy {
                project: "a".repeat(32),
                environment: "preview".into(),
                secret: "SERVICE_API_KEY".into(),
                revision: 1,
                endpoint: url.to_string(),
            },
            addresses: vec![],
            value: Zeroizing::new("synthetic-https-proof-token".into()),
        };
        let grant = Arc::new(Grant {
            packet: packet(),
            client: client_builder(&url, &[address])
                .add_root_certificate(reqwest::Certificate::from_pem(&ca).unwrap())
                .build()
                .unwrap(),
        });
        let pre = prepare(&component).unwrap();
        let request = |path: &str| {
            Request::builder()
                .uri(path)
                .header("host", "app.example.test")
                .body(Full::new(Bytes::new()))
                .unwrap()
        };
        for (path, expected) in [
            ("/isolation", "guest isolated"),
            ("/send", "200 {\"accepted\":true}"),
            ("/twice", "200 {\"accepted\":true};denied"),
            ("/wrong-host", "denied"),
            ("/wrong-path", "denied"),
            ("/no-retry-key", "denied"),
            ("/bad-json", "denied"),
            ("/redirect", "denied"),
            ("/large", "denied"),
            ("/echo", "denied"),
            ("/headers", "denied"),
            ("/disconnect", "unavailable"),
            ("/slow", "unavailable"),
        ] {
            let result = tokio::time::timeout(
                DEADLINE,
                invoke(&pre, request(path), None, Some(grant.clone())),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(result.body().as_ref(), expected.as_bytes(), "{path}");
        }
        let oversized = invoke(&pre, request("/oversize"), None, Some(grant.clone())).await;
        assert!(oversized.is_err() || oversized.unwrap().body().as_ref() == b"denied");
        let other_url = endpoint("https://other.example.test/events").unwrap();
        let mut other_packet = packet();
        other_packet.policy.endpoint = other_url.to_string();
        let wrong_name = Arc::new(Grant {
            packet: other_packet,
            client: client_builder(&other_url, &[address])
                .add_root_certificate(reqwest::Certificate::from_pem(&ca).unwrap())
                .build()
                .unwrap(),
        });
        let result = invoke(&pre, request("/wrong-host"), None, Some(wrong_name))
            .await
            .unwrap();
        assert_eq!(result.body().as_ref(), b"unavailable");
        let untrusted = Arc::new(Grant {
            packet: packet(),
            client: client_builder(&url, &[address]).build().unwrap(),
        });
        let result = invoke(&pre, request("/send"), None, Some(untrusted))
            .await
            .unwrap();
        assert_eq!(result.body().as_ref(), b"unavailable");
        let result = invoke(&pre, request("/send"), None, None).await.unwrap();
        assert_eq!(result.body().as_ref(), b"denied");
    }
}
