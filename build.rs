// This build script handles platform-specific setup
fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    
    // Special handling for Android builds to avoid OpenSSL dependency issues
    if target_os == "android" {
        println!("cargo:rustc-cfg=feature=\"rustls-tls\"");
        println!("cargo:rustc-cfg=no_openssl");
        println!("cargo:rustc-cfg=ossl_no_verify");
    }
    
    tauri_build::build()
}
