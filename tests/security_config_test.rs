use serde_json::Value;

#[test]
fn tauri_csp_is_restrictive_and_reader_compatible() {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let policy = config["app"]["security"]["csp"]
        .as_str()
        .expect("CSP must be a string");
    for directive in [
        "default-src 'self' ipc: http://ipc.localhost",
        "script-src 'self'",
        "worker-src 'self' blob:",
        "frame-src blob:",
        "img-src 'self' asset: http://asset.localhost data: blob:",
        "connect-src 'self' ipc: http://ipc.localhost asset: http://asset.localhost http://127.0.0.1:4081 ws: wss:",
        "object-src 'none'",
        "base-uri 'none'",
        "frame-ancestors 'none'",
    ] {
        assert!(policy.contains(directive), "missing {directive}");
    }

    let directives: Vec<_> = policy
        .split(';')
        .map(str::trim)
        .filter(|directive| !directive.is_empty())
        .collect();
    let script_directives: Vec<_> = directives
        .iter()
        .filter(|directive| directive.split_whitespace().next() == Some("script-src"))
        .collect();
    assert_eq!(
        script_directives.len(),
        1,
        "CSP must contain exactly one script-src directive"
    );
    let script_sources: Vec<_> = script_directives[0].split_whitespace().skip(1).collect();
    assert_eq!(script_sources, ["'self'"], "script-src must allow only 'self'");
}
