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
        "object-src 'none'",
        "base-uri 'none'",
        "frame-ancestors 'none'",
    ] {
        assert!(policy.contains(directive), "missing {directive}");
    }
    assert!(!policy.contains("'unsafe-eval'"));
    assert!(!policy.contains("script-src 'self' blob:"));
}
