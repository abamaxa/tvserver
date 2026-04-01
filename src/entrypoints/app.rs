#[cfg(not(feature = "webserver"))]
use crate::{adaptors::setup_frontend_listener, services::{setup_logging, TAURI_LOG}};

#[cfg(not(feature = "webserver"))]
use super::{tauri_api, tvserver::TVServer};
#[cfg(not(feature = "webserver"))]
use std::sync::Arc;

#[cfg(not(feature = "webserver"))]
pub async fn run_tauri() {
  setup_logging(TAURI_LOG);
  // Create and manage the shared state
  let tvserver = TVServer::new().await.expect("failed to initialize TVServer");
  let context = tvserver.get_context().clone();
  let shared_state = Arc::new(context);
  let sender = shared_state.get_messenger().get_sender();

  tauri::Builder::default()
    .setup(|app| {
      //if cfg!(debug_assertions) {
      //  app.handle().plugin(
      //    tauri_plugin_log::Builder::default()
      //      .level(log::LevelFilter::Info)
      //      .build(),
      //  )?;
      //}
      let app_handle = app.handle().clone();
      setup_frontend_listener(app_handle, "remote-player-backend", sender);
      Ok(())
    })
    .manage(shared_state)
    .invoke_handler(tauri_api::register_commands())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
