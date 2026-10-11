use std::{io, net::Ipv4Addr};

use axum::{
    Router,
    extract::Request,
    http::{StatusCode, header::HOST},
    middleware::{self, Next},
    response::{Html, IntoResponse},
    routing::get,
};
use devoption_ui::Page;
use tokio::{net::TcpListener, runtime::Builder};

pub fn run(port: u16) -> io::Result<()> {
    Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
            let address = listener.local_addr()?;
            println!("Listening on http://{address} (Ctrl+C to stop)");
            let app = Router::new()
                .route(
                    "/",
                    get(|| async { Html(devoption_ui::render(Page::Home)) }),
                )
                .route(
                    "/projects",
                    get(|| async { Html(devoption_ui::render(Page::Projects)) }),
                )
                .route(
                    "/about",
                    get(|| async { Html(devoption_ui::render(Page::About)) }),
                )
                .route(
                    "/settings/appearance",
                    get(|| async { Html(devoption_ui::render(Page::Appearance)) }),
                )
                .layer(middleware::from_fn(
                    move |request: Request, next: Next| async move {
                        if valid_authority(&request, address.port()) {
                            next.run(request).await
                        } else {
                            StatusCode::BAD_REQUEST.into_response()
                        }
                    },
                ));
            axum::serve(listener, app).await
        })
}

fn valid_authority(request: &Request, port: u16) -> bool {
    // Require one explicit Host even for HTTP/1.0; never infer it from proxy headers.
    let mut hosts = request.headers().get_all(HOST).iter();
    let Some(host) = hosts
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| local_host(value, port))
    else {
        return false;
    };
    if hosts.next().is_some() {
        return false;
    }

    // RFC 9112 §3.2: absolute-form carries its own authority. Validate both,
    // rejecting conflicting authorities rather than letting Host mask the target.
    let uri = request.uri();
    match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => {
            scheme.eq_ignore_ascii_case("http")
                && local_host(authority.as_str(), port) == Some(host)
        }
        (None, None) => uri.path().starts_with('/') && !uri.path().starts_with("//"),
        _ => false,
    }
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
            assert_eq!(valid_authority(&request, port), allowed, "{target} {host}");
        }
    }
}
