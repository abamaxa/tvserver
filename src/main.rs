// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "webserver")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = match args.get(1) {
        Some(port) => Some(port.parse::<u16>()
            .map_err(|e| anyhow::anyhow!("invalid port '{}': {}", port, e))?),
        None => None,
    };
    
    app_lib::run(port).await
}

#[cfg(not(feature = "webserver"))]
fn main() {
  app_lib::run();
}
