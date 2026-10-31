#[cfg(not(feature = "webserver"))]
use crate::{
    adaptors::setup_frontend_listener,
    services::{setup_logging, TAURI_LOG},
};

#[cfg(not(feature = "webserver"))]
use super::{tauri_api, tvserver::TVServer};
#[cfg(not(feature = "webserver"))]
use std::sync::Arc;
#[cfg(not(feature = "webserver"))]
use tauri::Manager;

#[cfg(not(feature = "webserver"))]
pub fn run_tauri() {
    setup_logging(TAURI_LOG);

    tauri::Builder::default()
        .setup(|app| {
            #[cfg(target_os = "android")]
            {
                let media_root = std::env::var_os("TVSERVER_MEDIA_ROOT")
                    .ok_or_else(|| anyhow::anyhow!("Android media storage is unavailable"))?;
                let storage = super::app_storage::AppStorage::initialize(
                    app.path().app_data_dir()?,
                    app.path().app_cache_dir()?,
                    std::path::PathBuf::from(media_root),
                )?;
                storage.configure_environment();
                crate::adaptors::android_platform::initialize(app)?;
                tauri::async_runtime::spawn(async {
                    if let Err(error) = crate::adaptors::youtube_runtime::update_android_on_startup().await {
                        tracing::error!("Bundled YouTube runtime could not initialize: {error}");
                    }
                });
                for directory in storage.media_directories() {
                    app.asset_protocol_scope()
                        .allow_directory(directory, true)?;
                }
            }

            // Resolve platform storage before starting services or opening the database.
            // Tauri owns the runtime so it also remains available to IPC callbacks.
            let app_handle = app.handle().clone();
            let tvserver = tauri::async_runtime::block_on(async move {
                let tvserver = TVServer::new().await?;
                let sender = tvserver.get_context().get_messenger().get_sender();
                setup_frontend_listener(app_handle, "remote-player-backend", sender);
                Ok::<_, anyhow::Error>(tvserver)
            })?;
            app.manage(Arc::new(tvserver.get_context().clone()));
            // Keep the service handles alive for the lifetime of the application.
            app.manage(tvserver);
            Ok(())
        })
        .invoke_handler(tauri_api::register_commands())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
