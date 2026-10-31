//! WASIp2 HTTP host for the separately supervised private app worker.
//! No owner store, inherited environment or filesystem preopens. Optional HTTPS
//! authorization stays in the host and requires an explicit deployment grant.
mod https;
mod records;
mod telemetry;

use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, Response, body::Bytes};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use wasmtime::{
    Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, Linker, ResourceTable},
};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView, p2::pipe::MemoryOutputPipe};
use wasmtime_wasi_http::p2::bindings::{ProxyPre, http::types::Scheme};
use wasmtime_wasi_http::{
    Error, RequestOptions, WasiBody, WasiHttpCtx, WasiHttpCtxView, WasiHttpHooks, WasiHttpView,
};

fn log(message: &str) {
    use std::io::Write;
    // systemd supplies this host-only path; WASI contexts inherit no environment.
    let directory = std::env::var_os("RUNTIME_DIRECTORY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/run/devoption-app".into());
    let path = directory.join("worker.log");
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true);
    let rotate =
        std::fs::metadata(&path).is_ok_and(|m| m.len() + message.len() as u64 + 1 > 1024 * 1024);
    if rotate {
        options.truncate(true);
    } else {
        options.append(true);
    }
    if let Ok(mut file) = options.open(path) {
        if rotate {
            let _ = writeln!(file, "[earlier worker output discarded at 1 MiB limit]");
        }
        let _ = writeln!(file, "{message}");
    }
}

const HEADERS: usize = 16 * 1024;
const INPUT: usize = 64 * 1024;
const OUTPUT: usize = 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(5);
const ADMISSION_WAIT: Duration = Duration::from_millis(250);

async fn admit(admission: &Semaphore) -> Option<tokio::sync::SemaphorePermit<'_>> {
    tokio::time::timeout(ADMISSION_WAIT, admission.acquire())
        .await
        .ok()?
        .ok()
}

struct State {
    wasi: WasiCtx,
    http: WasiHttpCtx,
    table: ResourceTable,
    limits: StoreLimits,
    hooks: https::Hooks,
    records: Option<Arc<records::Grant>>,
    record_call_used: bool,
}
impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
impl WasiHttpView for State {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.hooks,
        }
    }
}

/// Compilation and world validation only; this does not instantiate guest code.
fn prepare(bytes: &[u8]) -> wasmtime::Result<ProxyPre<State>> {
    prepare_with_records(bytes, false)
}

fn prepare_with_records(bytes: &[u8], records_enabled: bool) -> wasmtime::Result<ProxyPre<State>> {
    wasmtime::ensure!(
        bytes.len() <= 4 * 1024 * 1024 && bytes.starts_with(b"\0asm\x0d\0\x01\0"),
        "expected bounded component bytes"
    );
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true).epoch_interruption(true);
    let engine = Engine::new(&config)?;
    let component = Component::new(&engine, bytes)?;
    let mut linker = Linker::new(&engine);
    // Rust std imports standard WASI interfaces too. Their contexts grant
    // no files, environment or network; unknown non-WASI imports still fail.
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)?;
    if records_enabled {
        records::devoption::app::records::add_to_linker::<
            State,
            wasmtime::component::HasSelf<State>,
        >(&mut linker, |s| s)?;
    }
    ProxyPre::new(linker.instantiate_pre(&component)?)
}

async fn invoke(
    pre: &ProxyPre<State>,
    request: Request<Full<Bytes>>,
    records: Option<Arc<records::Grant>>,
    https: Option<Arc<https::Grant>>,
) -> wasmtime::Result<Response<Bytes>> {
    let stdout = MemoryOutputPipe::new(32 * 1024);
    let stderr = MemoryOutputPipe::new(32 * 1024);
    let mut wasi = WasiCtx::builder();
    wasi.stdout(stdout.clone())
        .stderr(stderr.clone())
        .allow_tcp(false)
        .allow_udp(false)
        .allow_ip_name_lookup(false);
    let mut http = WasiHttpCtx::new();
    http.set_field_size_limit(64 * 1024);
    let mut store = Store::new(
        pre.engine(),
        State {
            wasi: wasi.build(),
            http,
            table: ResourceTable::new(),
            hooks: https::Hooks::new(https),
            records,
            record_call_used: false,
            limits: StoreLimitsBuilder::new()
                .memory_size(64 * 1024 * 1024)
                .memories(1)
                .table_elements(10_000)
                .tables(16)
                .instances(16)
                .build(),
        },
    );
    store.limiter(|s| &mut s.limits);
    store.set_fuel(10_000_000)?;
    store.set_epoch_deadline(50);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let request = store
        .data_mut()
        .http()
        .new_incoming_request(Scheme::Http, request)?;
    let response = store.data_mut().http().new_response_outparam(sender)?;
    let run = async {
        let proxy = pre.instantiate_async(&mut store).await?;
        proxy
            .wasi_http_incoming_handler()
            .call_handle(&mut store, request, response)
            .await
    };
    let collect = async {
        let response = receiver.await??;
        let (parts, body) = response.into_parts();
        wasmtime::ensure!(
            parts
                .headers
                .iter()
                .map(|(k, v)| k.as_str().len() + v.len() + 4)
                .sum::<usize>()
                <= HEADERS,
            "response headers exceeded"
        );
        let body = Limited::new(body, OUTPUT)
            .collect()
            .await
            .map_err(|_| wasmtime::format_err!("response body exceeded or failed"))?
            .to_bytes();
        Ok::<_, wasmtime::Error>(Response::from_parts(parts, body))
    };
    let result = tokio::try_join!(run, collect);
    // Log only bounded guest output. Never include request headers or bodies.
    for pipe in [stdout, stderr] {
        let bytes = pipe.contents();
        if !bytes.is_empty() {
            log(&format!("guest: {}", String::from_utf8_lossy(&bytes)));
        }
        if bytes.len() == 32 * 1024 {
            log("[guest output limit reached]");
        }
    }
    result.map(|(_, response)| response)
}

async fn receive(stream: &mut TcpStream) -> Result<Request<Full<Bytes>>, u16> {
    let mut bytes = Vec::new();
    let end = loop {
        if let Some(i) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            break i + 4;
        }
        if bytes.len() >= HEADERS {
            return Err(431);
        }
        let mut chunk = [0; 1024];
        let n = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&chunk[..n]);
    };
    if end > HEADERS {
        return Err(431);
    }
    let mut headers = [httparse::EMPTY_HEADER; 100];
    let mut parsed = httparse::Request::new(&mut headers);
    parsed.parse(&bytes[..end]).map_err(|_| 400u16)?;
    let uri = parsed.path.ok_or(400u16)?;
    if !uri.starts_with('/') || uri.starts_with("//") {
        return Err(400);
    }
    let mut request = Request::builder()
        .method(parsed.method.ok_or(400u16)?)
        .uri(uri);
    let mut length = None;
    for h in parsed.headers {
        if h.name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(400);
        }
        if h.name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(400);
            }
            let text = std::str::from_utf8(h.value).map_err(|_| 400u16)?;
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err(400);
            }
            length = Some(text.parse::<usize>().map_err(|_| 413u16)?);
        }
        if h.name.eq_ignore_ascii_case("cookie")
            && let Ok(raw) = std::str::from_utf8(h.value)
            && let Some(id) = telemetry::cookie_id(raw)
        {
            request = request.extension(telemetry::ActivityCookie(id.to_owned()));
        }
        if ![
            "authorization",
            "cookie",
            "proxy-authorization",
            "connection",
        ]
        .iter()
        .any(|v| h.name.eq_ignore_ascii_case(v))
            && !h.name.to_ascii_lowercase().starts_with("x-devoption-")
        {
            request = request.header(h.name, h.value);
        }
    }
    let length = length.unwrap_or(0);
    if length > INPUT {
        return Err(413);
    }
    while bytes.len() - end < length {
        let mut chunk = [0; 4096];
        let n = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    request
        .body(Full::new(Bytes::copy_from_slice(&bytes[end..end + length])))
        .map_err(|_| 400)
}
fn failure(status: u16) -> Response<Bytes> {
    Response::builder()
        .status(status)
        .body(Bytes::from_static(b"Application request unavailable\n"))
        .expect("fixed response")
}
async fn connection(
    mut stream: TcpStream,
    pre: ProxyPre<State>,
    admission: Arc<Semaphore>,
    records: Option<Arc<records::Grant>>,
    https: Option<Arc<https::Grant>>,
    telemetry: Arc<std::sync::Mutex<telemetry::Telemetry>>,
) {
    let started = std::time::Instant::now();
    let mut visitor = None;
    let mut fresh = false;
    let mut probe = false;
    let response = match tokio::time::timeout(DEADLINE, receive(&mut stream)).await {
        Err(_) => failure(408),
        Ok(Err(code)) => failure(code),
        Ok(Ok(mut request)) => {
            probe = request.uri().path() == "/healthz";
            // Health probes and bots without HTML navigation do not create users.
            let navigation = request
                .headers()
                .get("accept")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("text/html"));
            // receive() strips all control cookies before building guest headers,
            // retaining only the validated activity identifier in a host extension.
            let cookie = request
                .extensions_mut()
                .remove::<telemetry::ActivityCookie>();
            if !probe && (navigation || cookie.is_some()) {
                let (id, new) = telemetry
                    .lock()
                    .expect("telemetry lock")
                    .visitor(cookie.as_ref().map(|c| c.0.as_str()));
                visitor = Some(id);
                fresh = new;
            }
            match admit(&admission).await {
                None => failure(503),
                Some(_permit) => {
                    match tokio::time::timeout(DEADLINE, invoke(&pre, request, records, https))
                        .await
                    {
                        Err(_) => failure(504),
                        Ok(Err(error))
                            if matches!(
                                error.downcast_ref::<wasmtime::Trap>(),
                                Some(wasmtime::Trap::Interrupt)
                            ) =>
                        {
                            failure(504)
                        }
                        Ok(Err(_)) => failure(502),
                        Ok(Ok(response)) => response,
                    }
                }
            }
        }
    };
    if !probe {
        telemetry.lock().expect("telemetry lock").record(
            telemetry::now(),
            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            response.status().is_server_error(),
            visitor.clone(),
        );
    }
    log(&format!("request completed: {}", response.status()));
    let invocation = std::env::var("INVOCATION_ID").unwrap_or_default();
    let invocation = if invocation.len() == 32 && invocation.bytes().all(|b| b.is_ascii_hexdigit())
    {
        invocation
    } else {
        String::new()
    };
    let mut wire = format!(
        "HTTP/1.1 {}\r\nConnection: close\r\nContent-Length: {}\r\nX-DevOption-Invocation: {invocation}\r\n",
        response.status(),
        response.body().len()
    )
    .into_bytes();
    for (name, value) in response.headers() {
        if [
            "connection",
            "content-length",
            "transfer-encoding",
            "upgrade",
        ]
        .contains(&name.as_str())
            || name.as_str().starts_with("x-devoption-")
        {
            continue;
        }
        wire.extend_from_slice(name.as_str().as_bytes());
        wire.extend_from_slice(b": ");
        wire.extend_from_slice(value.as_bytes());
        wire.extend_from_slice(b"\r\n");
    }
    if fresh && let Some(id) = visitor {
        wire.extend_from_slice(
            format!(
                "Set-Cookie: {}={id}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=86400\r\n",
                telemetry::COOKIE
            )
            .as_bytes(),
        );
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(response.body());
    let _ = tokio::time::timeout(DEADLINE, stream.write_all(&wire)).await;
}

/// Invoked only in the separately supervised worker. Listener is loopback-only.
pub fn run(path: &std::path::Path, port: u16, validate_only: bool) -> wasmtime::Result<()> {
    run_inner(path, port, validate_only, None, None)
}

/// Compatibility entry point for explicitly granted record workers.
pub fn run_with_records(
    path: &std::path::Path,
    port: u16,
    validate_only: bool,
    grant: &std::path::Path,
) -> wasmtime::Result<()> {
    let records = Arc::new(records::Grant::load(grant)?);
    run_inner(path, port, validate_only, Some(records), None)
}

/// Operator-supervised capability worker. No credential path is passed to the guest.
pub fn run_with_capabilities(
    path: &std::path::Path,
    port: u16,
    validate_only: bool,
    records: Option<&std::path::Path>,
    https: Option<&std::path::Path>,
) -> wasmtime::Result<()> {
    let records = records.map(records::Grant::load).transpose()?.map(Arc::new);
    let https = if validate_only {
        None
    } else {
        https.map(https::Grant::load).transpose()?.map(Arc::new)
    };
    run_inner(path, port, validate_only, records, https)
}

fn run_inner(
    path: &std::path::Path,
    port: u16,
    validate_only: bool,
    records: Option<Arc<records::Grant>>,
    https: Option<Arc<https::Grant>>,
) -> wasmtime::Result<()> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    wasmtime::ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() <= 4 * 1024 * 1024,
        "invalid component file"
    );
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    let pre = if records.is_some() {
        prepare_with_records(&bytes, true)?
    } else {
        prepare(&bytes)?
    };
    if validate_only {
        return Ok(());
    }
    let engine = pre.engine().clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));
            engine.increment_epoch();
        }
    });
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
            let telemetry = telemetry::start();
            let connections = Arc::new(Semaphore::new(16));
            let admission = Arc::new(Semaphore::new(1));
            loop {
                let (stream, _) = listener.accept().await?;
                if let Ok(permit) = connections.clone().try_acquire_owned() {
                    let pre = pre.clone();
                    let admission = admission.clone();
                    let records = records.clone();
                    let https = https.clone();
                    let telemetry = telemetry.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        connection(stream, pre, admission, records, https, telemetry).await;
                    });
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), wasmtime::Error>(())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn activity_identifier_survives_ingress_only_as_host_metadata() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            receive(&mut stream).await.unwrap()
        });
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(b"GET / HTTP/1.1\r\nHost: app\r\nAccept: text/html\r\nCookie: private=secret; __Host-devoption-activity=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nAuthorization: Bearer secret\r\n\r\n").await.unwrap();
        let request = server.await.unwrap();
        assert!(request.headers().get("cookie").is_none());
        assert!(request.headers().get("authorization").is_none());
        assert_eq!(
            request
                .extensions()
                .get::<telemetry::ActivityCookie>()
                .unwrap()
                .0,
            "a".repeat(32)
        );
    }

    #[tokio::test]
    async fn short_burst_waits_and_keeps_guest_execution_serial() {
        let admission = Arc::new(Semaphore::new(1));
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let initial = admission.acquire().await.unwrap();
        let mut requests = Vec::new();
        for _ in 0..7 {
            let admission = admission.clone();
            let active = active.clone();
            requests.push(tokio::spawn(async move {
                let _permit = admit(&admission).await.expect("short burst admitted");
                assert_eq!(active.fetch_add(1, std::sync::atomic::Ordering::SeqCst), 0);
                tokio::task::yield_now().await;
                assert_eq!(active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst), 1);
            }));
        }
        tokio::task::yield_now().await;
        assert!(requests.iter().all(|request| !request.is_finished()));
        drop(initial);
        for request in requests {
            request.await.unwrap();
        }
        assert_eq!(admission.available_permits(), 1);
    }

    #[tokio::test]
    async fn admission_expires_without_stealing_or_leaking_the_execution_permit() {
        let admission = Semaphore::new(1);
        let active = admission.acquire().await.unwrap();
        assert!(admit(&admission).await.is_none());
        assert_eq!(admission.available_permits(), 0);
        drop(active);
        drop(admit(&admission).await.expect("recovers after overload"));
        assert_eq!(admission.available_permits(), 1);
        admission.close();
        assert!(admit(&admission).await.is_none());
    }

    #[tokio::test]
    async fn cancelled_waiter_does_not_block_following_requests() {
        let admission = Arc::new(Semaphore::new(1));
        let active = admission.acquire().await.unwrap();
        let waiting = admission.clone();
        let request = tokio::spawn(async move { admit(&waiting).await.is_some() });
        tokio::task::yield_now().await;
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        drop(active);
        assert!(admit(&admission).await.is_some());
    }
    #[test]
    fn rejects_native_core_modules_and_oversized_artifacts_before_compilation() {
        for bytes in [
            b"#!/bin/sh\n".to_vec(),
            b"\0asm\x01\0\0\0".to_vec(),
            vec![0; 4 * 1024 * 1024 + 1],
        ] {
            assert!(prepare(&bytes).is_err());
        }
    }
    #[test]
    fn unknown_component_imports_fail_before_instantiation() {
        let mut types = wasm_encoder::ComponentTypeSection::new();
        types
            .function()
            .params(std::iter::empty::<(&str, wasm_encoder::ComponentValType)>())
            .result(None);
        let mut imports = wasm_encoder::ComponentImportSection::new();
        imports.import(
            "unapproved-operation",
            wasm_encoder::ComponentTypeRef::Func(0),
        );
        let mut component = wasm_encoder::Component::new();
        component.section(&types).section(&imports);
        let error = prepare(&component.finish())
            .err()
            .expect("unknown import rejected");
        assert!(error.to_string().contains("unapproved-operation"));
    }
    #[test]
    fn http_parser_rejects_ambiguous_lengths_transfer_encoding_and_large_bodies() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            for wire in [
                "POST / HTTP/1.1\r\nHost: app\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
                "POST / HTTP/1.1\r\nHost: app\r\nTransfer-Encoding: chunked\r\n\r\n",
                "POST / HTTP/1.1\r\nHost: app\r\nContent-Length: 65537\r\n\r\n",
                "GET http://elsewhere/ HTTP/1.1\r\nHost: app\r\n\r\n",
            ] {
                let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address=listener.local_addr().unwrap();
                let client=async { let mut socket=TcpStream::connect(address).await.unwrap(); socket.write_all(wire.as_bytes()).await.unwrap(); };
                let server=async { let (mut socket,_)=listener.accept().await.unwrap(); assert!(receive(&mut socket).await.is_err()); };
                tokio::join!(client,server);
            }
        });
    }
}
