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
pub async fn run() -> anyhow::Result<()> {
    run_webserver().await
}

#[cfg(not(feature = "webserver"))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_tauri();
}