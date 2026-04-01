use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine;
use std::net::{IpAddr, SocketAddr};

use crate::domain::config::get_auth_credentials;

/// Middleware that restricts access to localhost and private 192.168.* IPs.
/// Remote clients can authenticate via HTTP Basic Auth (header or query param).
pub async fn restrict_access(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let remote_ip = addr.ip();

    // Check X-Real-IP header first
    let ip_from_header = request
        .headers()
        .get("X-Real-IP")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            // Fall back to X-Forwarded-For (use first IP if multiple)
            request
                .headers()
                .get("X-Forwarded-For")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim().to_string())
        });

    // Allow if IP is local
    if is_allowed_ip(&remote_ip) && is_header_ip_allowed(ip_from_header.as_deref()) {
        return next.run(request).await;
    }

    // IP is not local — check for Basic Auth
    if check_basic_auth(&request) || check_query_auth(&request) {
        return next.run(request).await;
    }

    tracing::warn!(
        "Unauthorized access attempt - remote_ip: {}, ip_from_header: {:?}, uri: {}",
        remote_ip,
        ip_from_header,
        request.uri()
    );

    make_unauthorized_response()
}

/// Check the Authorization: Basic header for valid credentials.
fn check_basic_auth(request: &Request<Body>) -> bool {
    let Some(auth_value) = request.headers().get("Authorization") else {
        return false;
    };
    let Ok(auth_str) = auth_value.to_str() else {
        return false;
    };
    let Some(encoded) = auth_str.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded_bytes) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
        return false;
    };
    let Ok(decoded) = String::from_utf8(decoded_bytes) else {
        return false;
    };
    decoded == get_auth_credentials()
}

/// Check the `auth` query parameter for valid base64-encoded credentials.
/// Used for WebSocket connections where browsers cannot set custom headers.
fn check_query_auth(request: &Request<Body>) -> bool {
    let Some(query) = request.uri().query() else {
        return false;
    };
    for param in query.split('&') {
        if let Some(value) = param.strip_prefix("auth=") {
            let value = urlencoding::decode(value).unwrap_or_else(|_| value.into());
            let Ok(decoded_bytes) = base64::engine::general_purpose::STANDARD.decode(value.as_ref()) else {
                return false;
            };
            let Ok(decoded) = String::from_utf8(decoded_bytes) else {
                return false;
            };
            return decoded == get_auth_credentials();
        }
    }
    false
}

/// Build a 401 response with the WWW-Authenticate header.
fn make_unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("WWW-Authenticate", "Basic realm=\"tvserver\"")],
    )
        .into_response()
}

/// Check if an IP address is allowed (localhost or private 192.168.* subnet)
fn is_allowed_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            // Allow localhost
            if ipv4.is_loopback() {
                return true;
            }
            // Allow 192.168.* subnet
            let octets = ipv4.octets();
            octets[0] == 192 && octets[1] == 168
        }
        IpAddr::V6(ipv6) => {
            // Allow IPv6 loopback (::1)
            ipv6.is_loopback()
        }
    }
}

/// Check if header IP is allowed (empty is allowed, otherwise check same rules)
fn is_header_ip_allowed(ip_str: Option<&str>) -> bool {
    match ip_str {
        None | Some("") => true,
        Some(s) => {
            // Parse the IP string
            if let Ok(ip) = s.parse::<IpAddr>() {
                is_allowed_ip(&ip)
            } else {
                // If we can't parse it, deny access
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ensure_test_credentials() {
        std::env::set_var("AUTH_CREDENTIALS", "emma:Giraffe12");
    }

    #[test]
    fn test_localhost_ipv4_is_allowed() {
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert!(is_allowed_ip(&ip));
    }

    #[test]
    fn test_localhost_ipv6_is_allowed() {
        let ip: IpAddr = "::1".parse().unwrap();
        assert!(is_allowed_ip(&ip));
    }

    #[test]
    fn test_private_192_168_is_allowed() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        assert!(is_allowed_ip(&ip));

        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1));
        assert!(is_allowed_ip(&ip2));
    }

    #[test]
    fn test_other_private_ips_denied() {
        // 10.0.0.0/8 should be denied
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert!(!is_allowed_ip(&ip));

        // 172.16.0.0/12 should be denied
        let ip2 = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));
        assert!(!is_allowed_ip(&ip2));
    }

    #[test]
    fn test_public_ip_denied() {
        let ip = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        assert!(!is_allowed_ip(&ip));
    }

    #[test]
    fn test_empty_header_allowed() {
        assert!(is_header_ip_allowed(None));
        assert!(is_header_ip_allowed(Some("")));
    }

    #[test]
    fn test_header_ip_parsing() {
        assert!(is_header_ip_allowed(Some("127.0.0.1")));
        assert!(is_header_ip_allowed(Some("192.168.1.50")));
        assert!(!is_header_ip_allowed(Some("8.8.8.8")));
        assert!(!is_header_ip_allowed(Some("invalid")));
    }

    fn make_request_with_auth(header_value: &str) -> Request<Body> {
        Request::builder()
            .header("Authorization", header_value)
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn test_valid_basic_auth() {
        ensure_test_credentials();
        // base64("emma:Giraffe12") = "ZW1tYTpHaXJhZmZlMTI="
        let request = make_request_with_auth("Basic ZW1tYTpHaXJhZmZlMTI=");
        assert!(check_basic_auth(&request));
    }

    #[test]
    fn test_invalid_basic_auth_wrong_credentials() {
        ensure_test_credentials();
        // base64("wrong:wrong") = "d3Jvbmc6d3Jvbmc="
        let request = make_request_with_auth("Basic d3Jvbmc6d3Jvbmc=");
        assert!(!check_basic_auth(&request));
    }

    #[test]
    fn test_malformed_basic_auth() {
        // Not Basic scheme
        let request = make_request_with_auth("Bearer some-token");
        assert!(!check_basic_auth(&request));

        // No header at all
        let request = Request::builder().body(Body::empty()).unwrap();
        assert!(!check_basic_auth(&request));

        // Invalid base64
        let request = make_request_with_auth("Basic !!!invalid!!!");
        assert!(!check_basic_auth(&request));
    }

    fn make_request_with_query(query: &str) -> Request<Body> {
        Request::builder()
            .uri(format!("/api/remote/ws?{}", query))
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn test_valid_query_auth() {
        ensure_test_credentials();
        let request = make_request_with_query("auth=ZW1tYTpHaXJhZmZlMTI=");
        assert!(check_query_auth(&request));
    }

    #[test]
    fn test_invalid_query_auth() {
        let request = make_request_with_query("auth=d3Jvbmc6d3Jvbmc=");
        assert!(!check_query_auth(&request));
    }

    #[test]
    fn test_missing_query_auth() {
        let request = Request::builder()
            .uri("/api/remote/ws")
            .body(Body::empty())
            .unwrap();
        assert!(!check_query_auth(&request));
    }

    #[test]
    fn test_query_auth_with_other_params() {
        ensure_test_credentials();
        let request = make_request_with_query("foo=bar&auth=ZW1tYTpHaXJhZmZlMTI=&baz=qux");
        assert!(check_query_auth(&request));
    }

    #[test]
    fn test_unauthorized_response_has_www_authenticate_header() {
        let response = make_unauthorized_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get("WWW-Authenticate").unwrap(),
            "Basic realm=\"tvserver\""
        );
    }
}
