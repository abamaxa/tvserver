use super::{tauri_api, tvserver::TVServer};
use std::sync::Arc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[tokio::main]
pub async fn run_tauri() {
  // Create and manage the shared state
  let tvserver = TVServer::new().await.unwrap();
  let context = tvserver.get_context().clone();
  let shared_state = Arc::new(context);

  tauri::Builder::default()
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    //.plugin(messaging_plugin())
    .manage(shared_state)
    .invoke_handler(tauri_api::register_commands())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
