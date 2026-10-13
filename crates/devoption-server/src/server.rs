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

type SharedSession = Arc<SessionSlot>;

#[cfg(unix)]
mod auth;
#[cfg(unix)]
mod input;
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
    #[cfg(not(unix))]
    if data_directory.is_some() || repository_root.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "data-directory storage requires Unix",
        ));
    }
    #[cfg(unix)]
    let projects = if configured {
        Some(
            projects::Projects::open_with_root(
                owner_store.as_ref().expect("configured owner has a store"),
                repository_root,
            )
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
    let owner_auth = owner_store.map(|store| {
        (
            crate::owner_auth::service::OwnerAuthService::new(store),
            configured,
        )
    });
    Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
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
            let project_routes = projects
                .map(projects::routes)
                .unwrap_or_default()
                .merge(profile.map(profile::routes).unwrap_or_default());
            #[cfg(not(unix))]
            let project_routes = Router::new();
            let app = page_routes(configured, Arc::clone(&session), project_routes);
            let app = protect(app, address.port(), Arc::clone(&session));
            #[cfg(unix)]
            let app = match &owner_auth {
                Some((auth, true)) => {
                    // Merge the protected router last so its guarded fallback
                    // still handles unknown paths, including absent setup routes.
                    auth::routes(auth.clone(), address.port(), session).merge(app)
                }
                // Unconfigured storage never opens first-visitor enrollment.
                _ => app,
            };
            axum::serve(listener, app).await
        })
}

fn page_routes(configured: bool, session: SharedSession, project_routes: Router) -> Router {
    let mut pages = Router::new();
    for (path, page) in [
        ("/", Page::Home),
        ("/projects", Page::Projects),
        ("/releases", Page::Releases),
        ("/infrastructure", Page::Infrastructure),
        ("/settings/appearance", Page::Appearance),
        ("/settings/profile", Page::Profile),
    ] {
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

fn protect(app: Router, port: u16, session: SharedSession) -> Router {
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
    protect_origin(app, port)
}

// Also shared by pre-authentication routes, which cannot require an
// authenticated session. They must enforce their own CSRF/enrollment boundary.
fn protect_origin(app: Router, port: u16) -> Router {
    app.layer(middleware::from_fn(
        move |request: Request, next: Next| async move {
            let Some(host) = validated_host(&request, port) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if !safe_method(request.method()) && !same_origin(&request, host, port) {
                return StatusCode::FORBIDDEN.into_response();
            }
            next.run(request).await
        },
    ))
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
