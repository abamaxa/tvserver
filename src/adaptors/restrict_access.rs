use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use std::net::{IpAddr, SocketAddr};

/// Middleware that restricts access to localhost and private 192.168.* IPs.
///
/// This checks both the remote address and X-Real-IP/X-Forwarded-For headers
/// to ensure the request originates from an allowed IP.
pub async fn restrict_access(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
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

    // Check if both IPs are localhost/allowed
    if is_allowed_ip(&remote_ip) && is_header_ip_allowed(ip_from_header.as_deref()) {
        return Ok(next.run(request).await);
    }

    tracing::warn!(
        "Unauthorized access attempt - remote_ip: {}, ip_from_header: {:?}, uri: {}",
        remote_ip,
        ip_from_header,
        request.uri()
    );

    Err(StatusCode::UNAUTHORIZED)
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
}
