//! App-owned session transport; no platform identity or session validation.
use hyper::{HeaderMap, Method, header};

pub(crate) const COOKIE: &str = "__Host-devoption-app-session";

fn token(value: &str) -> bool {
    value.len() <= 512
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

/// Only this bounded opaque cookie crosses the app boundary. Empty means logout
/// or no session. Duplicates/invalid app tokens fail instead of choosing a value.
pub(crate) fn request_cookie(raw: &str) -> Result<Option<String>, u16> {
    let mut found = None;
    for part in raw.split(';') {
        let Some((name, value)) = part.trim().split_once('=') else {
            continue;
        };
        if name == COOKIE {
            if found.is_some() || !token(value) {
                return Err(400);
            }
            found = Some(value);
        }
    }
    Ok(found
        .filter(|v| !v.is_empty())
        .map(|v| format!("{COOKIE}={v}")))
}

/// Browser session mutations require the app's exact HTTPS origin. Apps still
/// validate their own CSRF token, login origin, audience, expiry and revocation.
pub(crate) fn check_origin(method: &Method, headers: &HeaderMap) -> Result<(), u16> {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
        || !headers.contains_key(header::COOKIE)
    {
        return Ok(());
    }
    let one = |name| {
        let mut values = headers.get_all(name).iter();
        let value = values.next()?.to_str().ok()?;
        values.next().is_none().then_some(value)
    };
    let host = one(header::HOST).ok_or(403u16)?;
    let origin = one(header::ORIGIN).ok_or(403u16)?;
    if origin != format!("https://{host}") {
        return Err(403);
    }
    Ok(())
}

/// The guest may set/clear only the app cookie, with strict host-only browser
/// scope. Fixed attributes prevent Domain/path/cookie-prefix scope confusion.
pub(crate) fn response_cookie(raw: &str) -> bool {
    let mut parts = raw.split(';').map(str::trim);
    let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else {
        return false;
    };
    if name != COOKIE || !token(value) {
        return false;
    }
    let mut secure = false;
    let mut http_only = false;
    let mut path = false;
    let mut same_site = false;
    let mut max_age = false;
    for part in parts {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        match key.to_ascii_lowercase().as_str() {
            "secure" if !secure && value.is_empty() => secure = true,
            "httponly" if !http_only && value.is_empty() => http_only = true,
            "path" if !path && value == "/" => path = true,
            "samesite" if !same_site && matches!(value, "Strict" | "Lax") => same_site = true,
            "max-age"
                if !max_age
                    && !value.is_empty()
                    && value.bytes().all(|b| b.is_ascii_digit())
                    && value.parse::<u32>().is_ok() =>
            {
                max_age = true
            }
            _ => return false,
        }
    }
    secure && http_only && path && same_site
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bounded_app_cookie_survives_mixed_control_credentials() {
        assert_eq!(
            request_cookie("__Host-devoption=secret; other=secret"),
            Ok(None)
        );
        assert_eq!(
            request_cookie(&format!(
                "private=secret; {COOKIE}=opaque_123-ABC; __Host-devoption=secret"
            )),
            Ok(Some(format!("{COOKIE}=opaque_123-ABC")))
        );
        for raw in [
            format!("{COOKIE}=first; {COOKIE}=second"),
            format!("{COOKIE}=bad value"),
            format!("{COOKIE}={}", "x".repeat(513)),
        ] {
            assert_eq!(request_cookie(&raw), Err(400));
        }
        assert_eq!(request_cookie(&format!("{COOKIE}=")), Ok(None));
    }

    #[test]
    fn app_cookie_mutations_require_exact_single_https_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "app.example.test".parse().unwrap());
        headers.insert(header::COOKIE, format!("{COOKIE}=value").parse().unwrap());
        for origin in [
            None,
            Some("null"),
            Some("http://app.example.test"),
            Some("https://other.example.test"),
            Some("https://app.example.test/"),
        ] {
            headers.remove(header::ORIGIN);
            if let Some(value) = origin {
                headers.insert(header::ORIGIN, value.parse().unwrap());
            }
            assert_eq!(check_origin(&Method::POST, &headers), Err(403));
        }
        headers.insert(header::ORIGIN, "https://app.example.test".parse().unwrap());
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert_eq!(check_origin(&method, &headers), Ok(()));
        }
        headers.append(header::ORIGIN, "https://app.example.test".parse().unwrap());
        assert_eq!(check_origin(&Method::POST, &headers), Err(403));
        assert_eq!(check_origin(&Method::GET, &headers), Ok(()));
        headers.remove(header::COOKIE);
        assert_eq!(check_origin(&Method::POST, &headers), Ok(()));
    }

    #[test]
    fn response_scope_cannot_set_control_domain_or_unsafe_session_cookies() {
        let good = format!("{COOKIE}=value; Secure; HttpOnly; Path=/; SameSite=Strict");
        assert!(response_cookie(&good));
        assert!(response_cookie(&format!(
            "{COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0"
        )));
        for bad in [
            good.replace(COOKIE, "__Host-devoption"),
            good.replace("; Secure", ""),
            good.replace("; HttpOnly", ""),
            good.replace("Path=/", "Path=/account"),
            good.replace("Strict", "None"),
            format!("{good}; Domain=example.test"),
            format!("{good}; Secure"),
            format!("{good}; Max-Age=-1"),
            good.replace("value", "bad,value"),
        ] {
            assert!(!response_cookie(&bad), "{bad}");
        }
    }
}
