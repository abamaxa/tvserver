fn main() {
    println!("cargo:rerun-if-changed=migrations");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        // NDK r26 defaults to 4 KB. Match the 16 KB-aligned player/Python
        // prebuilts, including RELRO boundaries, without changing host builds.
        println!("cargo:rustc-link-arg-cdylib=-Wl,-z,max-page-size=16384");
        println!("cargo:rustc-link-arg-cdylib=-Wl,-z,common-page-size=16384");
    }
    #[cfg(not(feature = "webserver"))]
    tauri_build::build()
}
