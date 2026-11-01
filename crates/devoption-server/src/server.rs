use std::{io, net::Ipv4Addr, path::Path, sync::Arc};

use crate::{identity::IdentityId, session::slot::SessionSlot};
use axum::{
    Router,
    extract::Request,
    http::{
        HeaderMap, Method, StatusCode,
        header::{CACHE_CONTROL, HOST, LOCATION, ORIGIN},
    },
    middleware::{self, Next},
    response::{Html, IntoResponse},
    routing::get,
};
use devoption_ui::Page;
use tokio::{net::TcpListener, runtime::Builder};

mod origin;
pub use origin::PublicOrigin;
use origin::RequestOrigin;

type SharedSession = Arc<SessionSlot>;

#[cfg(unix)]
mod auth;
#[cfg(unix)]
mod input;
#[cfg(unix)]
mod mcp;
#[cfg(unix)]
mod overview;
#[cfg(unix)]
mod platform_updates;
#[cfg(unix)]
mod profile;
#[cfg(unix)]
mod projects;
#[cfg(all(test, unix))]
mod session_tests;

/// Run the loopback service from synchronous startup code, owning its Tokio runtime.
/// Validate optional existing storage before binding and announce the address on stdout.
/// Return startup or server I/O errors; no session is issued at startup.
pub fn run(port: u16, data_directory: Option<&Path>) -> io::Result<()> {
    run_with_repository_root(port, data_directory, None)
}

/// Optional repository access is granted only by explicit local configuration.
/// The browser can select a relative checkout, never the permitted root.
pub fn run_with_repository_root(
    port: u16,
    data_directory: Option<&Path>,
    repository_root: Option<&Path>,
) -> io::Result<()> {
    run_with_roots(
        port,
        data_directory,
        repository_root,
        false,
        None,
        None,
        ExecutionMode::Disabled.into(),
    )
}

/// Explicit creation capability in a selected private repository root. Existing
/// entries remain untouched; this grants no build, hook or execution authority.
pub fn run_with_managed_repository_root(
    port: u16,
    data_directory: Option<&Path>,
    repository_root: &Path,
) -> io::Result<()> {
    run_with_roots(
        port,
        data_directory,
        Some(repository_root),
        true,
        None,
        None,
        ExecutionMode::Disabled.into(),
    )
}

/// Recover explicitly selected history before listening, retaining its writer lock.
/// This records interrupted supervision, not process cleanup or execution admission.
pub fn run_with_build_history(
    port: u16,
    data_directory: &Path,
    repository_root: Option<&Path>,
    managed: bool,
    history_directory: &Path,
) -> io::Result<()> {
    if managed && repository_root.is_none() {
        return Err(io::Error::other("managed repository mode requires a root"));
    }
    run_with_roots(
        port,
        Some(data_directory),
        repository_root,
        managed,
        Some(history_directory),
        None,
        ExecutionMode::Disabled.into(),
    )
}

/// Serve one explicitly configured HTTPS origin behind a local TLS proxy.
/// The listener remains loopback-only and an existing owner is mandatory.
pub fn run_with_public_origin(
    port: u16,
    data_directory: &Path,
    repository_root: Option<&Path>,
    managed: bool,
    history_directory: Option<&Path>,
    origin: PublicOrigin,
) -> io::Result<()> {
    if managed && repository_root.is_none() {
        return Err(io::Error::other("managed repository mode requires a root"));
    }
    run_with_roots(
        port,
        Some(data_directory),
        repository_root,
        managed,
        history_directory,
        Some(origin),
        ExecutionMode::Disabled.into(),
    )
}

pub fn run_with_execution(
    port: u16,
    data_directory: &Path,
    repository_root: &Path,
    managed: bool,
    history_directory: &Path,
    public_origin: Option<PublicOrigin>,
) -> io::Result<()> {
    run_with_roots(
        port,
        Some(data_directory),
        Some(repository_root),
        managed,
        Some(history_directory),
        public_origin,
        ExecutionMode::Builds.into(),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExecutionMode {
    Disabled,
    Builds,
    Deployments,
}

pub fn run_with_deployments(
    port: u16,
    data_directory: &Path,
    repository_root: &Path,
    managed: bool,
    history_directory: &Path,
    public_origin: Option<PublicOrigin>,
) -> io::Result<()> {
    run_with_roots(
        port,
        Some(data_directory),
        Some(repository_root),
        managed,
        Some(history_directory),
        public_origin,
        ExecutionMode::Deployments.into(),
    )
}

/// Explicit installation-key opt-in; no credentials are created by startup.
pub struct ProjectSecretsOptions<'a> {
    pub key_file: &'a Path,
    pub build_execution: bool,
    pub app_deployment: bool,
}

/// Enable encrypted management alongside the existing optional workloads.
pub fn run_with_project_secrets(
    port: u16,
    data: &Path,
    repositories: Option<&Path>,
    managed: bool,
    history: Option<&Path>,
    origin: Option<PublicOrigin>,
    secrets: ProjectSecretsOptions<'_>,
) -> io::Result<()> {
    if (managed && repositories.is_none())
        || (secrets.app_deployment && !secrets.build_execution)
        || (secrets.build_execution && (repositories.is_none() || history.is_none()))
    {
        return Err(io::Error::other(
            "invalid project secrets startup capabilities",
        ));
    }
    let execution = if secrets.app_deployment {
        ExecutionMode::Deployments
    } else if secrets.build_execution {
        ExecutionMode::Builds
    } else {
        ExecutionMode::Disabled
    };
    run_with_roots(
        port,
        Some(data),
        repositories,
        managed,
        history,
        origin,
        Capabilities {
            execution,
            key: Some(secrets.key_file),
        },
    )
}

struct Capabilities<'a> {
    execution: ExecutionMode,
    key: Option<&'a Path>,
}
impl From<ExecutionMode> for Capabilities<'_> {
    fn from(execution: ExecutionMode) -> Self {
        Self {
            execution,
            key: None,
        }
    }
}

fn run_with_roots(
    port: u16,
    data_directory: Option<&Path>,
    repository_root: Option<&Path>,
    managed: bool,
    history_directory: Option<&Path>,
    public_origin: Option<PublicOrigin>,
    capabilities: Capabilities<'_>,
) -> io::Result<()> {
    let Capabilities { execution, key } = capabilities;
    // Validate storage before binding; only an absent record is unconfigured.
    // Retain the same descriptor through the service. Selecting storage grants
    // no enrollment permission; only an already-present owner can sign in.
    #[cfg(unix)]
    let owner_store = data_directory
        .map(crate::owner_store::OwnerStore::open_absolute)
        .transpose()
        .map_err(|error| io::Error::other(format!("data directory unavailable: {error}")))?;
    #[cfg(unix)]
    let owner_identity = owner_store
        .as_ref()
        .map(|store| {
            store
                .load()
                .map(|record| record.map(|record| record.identity()))
        })
        .transpose()
        .map_err(|error| io::Error::other(format!("data directory unavailable: {error}")))?
        .flatten();
    #[cfg(unix)]
    let configured = owner_identity.is_some();
    #[cfg(unix)]
    if public_origin.is_some() && !configured {
        return Err(io::Error::other(
            "public origin requires an existing configured owner",
        ));
    }
    #[cfg(not(unix))]
    let _ = managed;
    #[cfg(not(unix))]
    if data_directory.is_some()
        || repository_root.is_some()
        || history_directory.is_some()
        || public_origin.is_some()
        || key.is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "data-directory storage requires Unix",
        ));
    }
    #[cfg(unix)]
    let mut projects = if configured {
        Some(
            if managed {
                projects::Projects::open_with_managed_root(
                    owner_store.as_ref().expect("configured owner has a store"),
                    repository_root.expect("managed root supplied"),
                )
            } else {
                projects::Projects::open_with_root(
                    owner_store.as_ref().expect("configured owner has a store"),
                    repository_root,
                )
            }
            .map_err(|error| io::Error::other(format!("project catalog unavailable: {error}")))?,
        )
    } else {
        if repository_root.is_some() {
            return Err(io::Error::other(
                "repository root requires an existing configured owner",
            ));
        }
        None
    };
    #[cfg(unix)]
    if let Some(key) = key {
        let owner = owner_store
            .as_ref()
            .ok_or_else(|| io::Error::other("project secrets require owner storage"))?;
        let identity = owner_identity
            .ok_or_else(|| io::Error::other("project secrets require an existing owner"))?;
        let forbidden: Vec<&Path> = [repository_root, history_directory]
            .into_iter()
            .flatten()
            .collect();
        let store = crate::project_secrets::Store::open(
            owner.duplicate_directory()?,
            key,
            identity.to_bytes(),
            &forbidden,
        )
        .map_err(|_| io::Error::other("project secrets key or storage is unavailable"))?;
        projects
            .as_mut()
            .expect("configured projects")
            .enable_secrets(store)?;
    }
    #[cfg(unix)]
    let profile = owner_identity
        .map(|identity| {
            crate::profile::ProfileStore::new(
                owner_store.as_ref().expect("configured owner has a store"),
                identity,
            )
        })
        .transpose()
        .map_err(|error| io::Error::other(format!("profile unavailable: {error}")))?;
    #[cfg(unix)]
    let mut build_history = match history_directory {
        Some(path) => {
            if !configured {
                return Err(io::Error::other(
                    "build history requires an existing configured owner",
                ));
            }
            let path = path
                .to_str()
                .ok_or_else(|| io::Error::other("build history path requires UTF-8"))?;
            let history = devoption_builds::history::History::open(path)
                .map_err(|error| io::Error::other(format!("build history unavailable: {error}")))?;
            Some(history)
        }
        None => None,
    };
    #[cfg(unix)]
    if execution != ExecutionMode::Disabled {
        let config = devoption_builds::execution::Configuration::open()?;
        config.check_storage(
            data_directory.ok_or_else(|| io::Error::other("builds require owner storage"))?,
            repository_root.ok_or_else(|| io::Error::other("builds require repository storage"))?,
            history_directory.ok_or_else(|| io::Error::other("builds require history storage"))?,
        )?;
        config.stop_previous()?;
        let identity =
            owner_identity.ok_or_else(|| io::Error::other("builds require an existing owner"))?;
        projects = Some(
            projects
                .take()
                .ok_or_else(|| io::Error::other("builds require repository storage"))?
                .with_execution(config, identity),
        );
    }
    #[cfg(not(unix))]
    if execution != ExecutionMode::Disabled {
        return Err(io::Error::other("build execution requires Linux"));
    }
    #[cfg(unix)]
    let mcp_config = owner_store
        .as_ref()
        .map(mcp::Configuration::open)
        .transpose()?
        .flatten();
    #[cfg(unix)]
    if mcp_config.is_some() && (public_origin.is_none() || !configured) {
        return Err(io::Error::other(
            "MCP requires an existing owner and explicit HTTPS public origin",
        ));
    }
    #[cfg(unix)]
    let owner_auth = owner_store
        .map(|store| {
            if configured {
                crate::owner_auth::service::OwnerAuthService::with_users(store)
                    .map(|auth| (auth, true))
            } else {
                Ok((
                    crate::owner_auth::service::OwnerAuthService::new(store),
                    false,
                ))
            }
        })
        .transpose()?;
    Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            // Recovery precedes routes/listening; routes retain the shared writer.
            #[cfg(unix)]
            if let Some(mut history) = build_history.take() {
                let configured_projects = projects
                    .take()
                    .expect("history requires configured projects");
                configured_projects.recover_build_history(&mut history)?;
                projects = Some(configured_projects.with_history(history));
            }
            #[cfg(unix)]
            if execution == ExecutionMode::Deployments {
                projects
                    .as_mut()
                    .expect("configured projects")
                    .enable_deployment(
                        data_directory.expect("checked owner"),
                        repository_root.expect("checked root"),
                        history_directory.expect("checked history"),
                        public_origin.as_ref().map(PublicOrigin::as_str),
                        port,
                    )?;
            }
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
            let address = listener.local_addr()?;
            println!("Listening on http://{address} (Ctrl+C to stop)");
            #[cfg(unix)]
            let configured = owner_auth
                .as_ref()
                .is_some_and(|(_, configured)| *configured);
            #[cfg(not(unix))]
            let configured = false;
            let session = Arc::new(SessionSlot::default());
            #[cfg(unix)]
            let overview_projects = projects.clone();
            #[cfg(unix)]
            let project_routes = projects
                .map(projects::routes)
                .unwrap_or_default()
                .merge(profile.map(profile::routes).unwrap_or_default());
            #[cfg(not(unix))]
            let project_routes = Router::new();
            let installed = platform_installation(
                data_directory,
                repository_root,
                port,
                public_origin.is_some(),
            );
            let app = page_routes(
                configured,
                Arc::clone(&session),
                project_routes,
                installed,
                {
                    #[cfg(unix)]
                    {
                        overview_projects.clone()
                    }
                    #[cfg(not(unix))]
                    {
                        None
                    }
                },
            );
            let origin = public_origin
                .clone()
                .map(RequestOrigin::Public)
                .unwrap_or(RequestOrigin::Local(address.port()));
            let app = protect(app, origin.clone(), Arc::clone(&session));
            #[cfg(unix)]
            let app = if let Some(config) = mcp_config {
                let (auth, _) = owner_auth.as_ref().expect("MCP requires owner");
                mcp::routes(
                    config,
                    public_origin.clone().expect("MCP requires HTTPS"),
                    auth.clone(),
                    session.clone(),
                    overview_projects.clone().expect("configured projects"),
                )
                .merge(app)
            } else {
                app
            };
            #[cfg(unix)]
            let app = match &owner_auth {
                Some((auth, true)) => {
                    // Merge the protected router last so its guarded fallback
                    // still handles unknown paths, including absent setup routes.
                    auth::routes(auth.clone(), origin, session).merge(app)
                }
                // Unconfigured storage never opens first-visitor enrollment.
                _ => app,
            };
            axum::serve(listener, app).await
        })
}

fn platform_installation(
    data: Option<&Path>,
    repositories: Option<&Path>,
    port: u16,
    public: bool,
) -> bool {
    data == Some(Path::new("/var/lib/devoption/data"))
        && repositories == Some(Path::new("/var/lib/devoption/repositories"))
        && port == 3000
        && public
}

#[cfg(unix)]
type OverviewProjects = projects::Projects;
#[cfg(not(unix))]
type OverviewProjects = ();

fn page_routes(
    configured: bool,
    session: SharedSession,
    project_routes: Router,
    _installed: bool,
    _overview: Option<OverviewProjects>,
) -> Router {
    let mut pages = Router::new();
    for (path, page) in [
        ("/", Page::Home),
        ("/projects", Page::Projects),
        ("/releases", Page::Releases),
        ("/infrastructure", Page::Infrastructure),
        ("/settings/appearance", Page::Appearance),
        ("/settings/profile", Page::Profile),
    ] {
        #[cfg(unix)]
        if configured && matches!(page, Page::Home) {
            let projects = _overview.clone();
            pages = pages.route(
                path,
                get(move |query: axum::extract::RawQuery| {
                    overview::home(projects.clone(), _installed, query.0)
                }),
            );
            continue;
        }
        if configured && matches!(page, Page::Projects) {
            continue;
        }
        let html = Arc::new(devoption_ui::render_with_account(page, configured));
        pages = pages.route(
            path,
            get(move || {
                let html = Arc::clone(&html);
                async move { ([(CACHE_CONTROL, "no-store")], Html((*html).clone())) }
            }),
        );
    }
    pages = pages.merge(project_routes);
    // This startup decision is fixed even if the owner record later disappears.
    // Layer only the protected routes; Sign in is added afterward and stays public.
    if configured {
        pages = pages.route_layer(middleware::from_fn(
            move |mut request: Request, next: Next| {
                let session = Arc::clone(&session);
                async move {
                    if matches!(*request.method(), Method::GET | Method::HEAD) {
                        let Some(identity) = page_identity(request.headers(), &session) else {
                            return (
                                StatusCode::SEE_OTHER,
                                [(LOCATION, "/sign-in"), (CACHE_CONTROL, "no-store")],
                            )
                                .into_response();
                        };
                        request.extensions_mut().insert(identity);
                    }
                    next.run(request).await
                }
            },
        ));
    }
    pages.route(
        "/sign-in",
        get(move || async move {
            (
                [(CACHE_CONTROL, "no-store")],
                Html(devoption_ui::render_with_account(Page::SignIn, configured)),
            )
        }),
    )
}

fn page_identity(headers: &HeaderMap, session: &SharedSession) -> Option<IdentityId> {
    #[cfg(unix)]
    return crate::browser_session::authenticate_read(headers, session);
    #[cfg(not(unix))]
    {
        let _ = (headers, session);
        None
    }
}

fn protect(app: Router, origin: impl Into<RequestOrigin>, session: SharedSession) -> Router {
    let app = app.layer(middleware::from_fn(
        move |mut request: Request, next: Next| {
            let session = Arc::clone(&session);
            async move {
                if !safe_method(request.method()) {
                    let Some(identity) = authenticated_identity(request.headers(), &session) else {
                        return (StatusCode::UNAUTHORIZED, [(CACHE_CONTROL, "no-store")])
                            .into_response();
                    };
                    // Downstream permission checks must use this server-owned ID,
                    // never a username/role asserted in headers, the URL, or a body.
                    request.extensions_mut().insert(identity);
                }
                next.run(request).await
            }
        },
    ));
    // The outer guard always runs before session validation or the handler.
    protect_origin(app, origin)
}

// Also shared by pre-authentication routes, which cannot require an
// authenticated session. They must enforce their own CSRF/enrollment boundary.
fn protect_origin(app: Router, origin: impl Into<RequestOrigin>) -> Router {
    let origin = origin.into();
    app.layer(middleware::from_fn(move |request: Request, next: Next| {
        let result = origin.check(&request);
        async move {
            if let Err(status) = result {
                return (status, [(CACHE_CONTROL, "no-store")]).into_response();
            }
            next.run(request).await
        }
    }))
}

fn safe_method(method: &Method) -> bool {
    // RFC 9110 safe methods; extensions require Origin and a session.
    // Page-read authorization is separate; all safe methods must never mutate.
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

fn authenticated_identity(headers: &HeaderMap, session: &SharedSession) -> Option<IdentityId> {
    // The HTTP service only issues browser credentials. Keep its mutation
    // boundary on the strict cookie/CSRF adapter, including mixed-header rejection.
    // Future API authentication needs its own explicit transport contract.
    #[cfg(unix)]
    return crate::browser_session::authenticate(headers, session);
    #[cfg(not(unix))]
    {
        let _ = (headers, session);
        None
    }
}

fn validated_host(request: &Request, port: u16) -> Option<&'static str> {
    // Require one explicit Host even for HTTP/1.0; never infer it from proxy headers.
    let mut hosts = request.headers().get_all(HOST).iter();
    let host = hosts
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| local_host(value, port))?;
    if hosts.next().is_some() {
        return None;
    }

    // RFC 9112 §3.2: absolute-form carries its own authority. Validate both,
    // rejecting conflicting authorities rather than letting Host mask the target.
    let uri = request.uri();
    let valid = match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => {
            scheme.eq_ignore_ascii_case("http")
                && local_host(authority.as_str(), port) == Some(host)
        }
        (None, None) => uri.path().starts_with('/') && !uri.path().starts_with("//"),
        _ => false,
    };
    valid.then_some(host)
}

fn same_origin(request: &Request, host: &str, port: u16) -> bool {
    let mut origins = request.headers().get_all(ORIGIN).iter();
    let Some(origin) = origins.next().and_then(|value| value.to_str().ok()) else {
        return false;
    };
    if origins.next().is_some() {
        return false;
    }
    // An Origin has only scheme and authority, never URL paths or userinfo.
    // Do not infer a missing Origin from Referer, proxy headers, or Fetch Metadata.
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    scheme.eq_ignore_ascii_case("http") && local_host(authority, port) == Some(host)
}

fn local_host(authority: &str, bound_port: u16) -> Option<&'static str> {
    // This allowlist needs no DNS lookup or general URL normalization. In
    // particular, userinfo, encoded names, IPv6, suffixes, and lists cannot match.
    let host = match authority.split_once(':') {
        Some((host, port)) => {
            if port.is_empty()
                || !port.bytes().all(|byte| byte.is_ascii_digit())
                || port.parse::<u16>().ok() != Some(bound_port)
            {
                return None;
            }
            host
        }
        None if bound_port == 80 => authority,
        None => return None,
    };
    if host.eq_ignore_ascii_case("localhost") {
        Some("localhost")
    } else if host == "127.0.0.1" {
        Some("127.0.0.1")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    #[test]
    fn omitted_ports_mean_http_port_80_not_the_listener_default() {
        // Exercise port 80 without requiring a privileged listener in CI.
        for (target, host, port, allowed) in [
            ("/", "localhost", 80, true),
            ("/", "127.0.0.1", 80, true),
            ("http://LOCALHOST/", "localhost:80", 80, true),
            ("http://127.0.0.1:80/", "127.0.0.1", 80, true),
            ("/", "localhost", 3000, false),
            ("http://localhost/", "localhost:3000", 3000, false),
            ("/", "localhost:", 80, false),
            ("http://localhost:/", "localhost:80", 80, false),
        ] {
            let request = Request::builder()
                .uri(target)
                .header(HOST, host)
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                validated_host(&request, port).is_some(),
                allowed,
                "{target} {host}"
            );
        }
    }

    #[test]
    fn origin_default_port_and_host_aliases_are_compared_as_origins() {
        for (origin, host, port, allowed) in [
            ("http://localhost", "localhost:80", 80, true),
            ("HTTP://LOCALHOST:080", "localhost", 80, true),
            ("http://127.0.0.1:80", "127.0.0.1", 80, true),
            ("http://localhost", "localhost:3000", 3000, false),
            ("http://localhost:", "localhost:80", 80, false),
            ("http://localhost/", "localhost:80", 80, false),
            ("https://localhost:80", "localhost:80", 80, false),
            ("http://127.0.0.1", "localhost:80", 80, false),
        ] {
            let request = Request::builder()
                .method(Method::POST)
                .uri("/")
                .header(HOST, host)
                .header(ORIGIN, origin)
                .body(Body::empty())
                .unwrap();
            let host = validated_host(&request, port).unwrap();
            assert_eq!(same_origin(&request, host, port), allowed, "{origin}");
        }
    }
}
