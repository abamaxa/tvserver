//! # TVServer
//!
//! `TVServer` is the daemon server that provides a REST API for the remote control and more....
//!
//! Currently its very lightly documented as it is very much a work in progress.

extern crate core;

pub mod adaptors;
pub mod domain;
pub mod entrypoints;
pub mod services;
#[cfg(not(feature = "webserver"))]
use entrypoints::run_tauri;
#[cfg(feature = "webserver")]
use entrypoints::run_webserver;

#[cfg(feature = "webserver")]
pub async fn run(port: Option<u16>) -> anyhow::Result<()> {
    run_webserver(port).await
}

#[cfg(not(feature = "webserver"))]
#[cfg_attr(not(mobile), tokio::main)]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    // on desktop, tokio::main handles the async runtime
    // on mobile, mobile_entry_point handles the async runtime
    run_tauri().await;
}