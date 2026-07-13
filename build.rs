#[cfg(not(feature = "webserver"))]
fn main() {
    tauri_build::build()
}

#[cfg(feature = "webserver")]
fn main() {}
