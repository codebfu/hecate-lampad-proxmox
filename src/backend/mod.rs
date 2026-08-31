//! Copyright (C) 2026 Gaultier HUBERT
//! SPDX-License-Identifier: GPL-3.0-or-later

//! Console backends and backend selection.

pub mod local_vnc;
pub mod mock;
pub mod pve_api;
pub mod rfb;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("backend unavailable: {0}")]
    Unavailable(String),
    #[error("console protocol error: {0}")]
    Protocol(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleFrame {
    pub width: u32,
    pub height: u32,
    pub format: String,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

pub trait ConsoleSession: Send {
    fn backend_kind(&self) -> &'static str;
    fn dimensions(&self) -> (u32, u32);
    fn frame(&mut self) -> Result<ConsoleFrame, BackendError>;
    fn input(&mut self, events: &[Value]) -> Result<(), BackendError>;
    fn close(&mut self) -> Result<(), BackendError>;
}

pub async fn open_console(vmid: u32) -> Result<Box<dyn ConsoleSession>, BackendError> {
    let mut failures = Vec::new();
    match local_vnc::LocalVncBackend::open(vmid) {
        Ok(session) => return Ok(Box::new(session)),
        Err(error) => failures.push(format!("local qm: {error}")),
    }
    match pve_api::PveApiBackend::open(vmid).await {
        Ok(session) => return Ok(Box::new(session)),
        Err(error) => failures.push(format!("PVE API: {error}")),
    }
    if std::env::var("HECATE_PROXMOX_MOCK").as_deref() == Ok("1") {
        return Ok(Box::new(mock::MockBackend::open(vmid)?));
    }
    Err(BackendError::Unavailable(format!(
        "unable to open VM {vmid} console; {}",
        failures.join("; ")
    )))
}

pub async fn list_vms() -> Result<Value, BackendError> {
    match local_vnc::list_vms() {
        Ok(vms) => Ok(json!({ "vms": vms, "source": "qm" })),
        Err(local_error) => match pve_api::list_vms().await {
            Ok(vms) => Ok(json!({ "vms": vms, "source": "pve-api" })),
            Err(api_error) => Err(BackendError::Unavailable(format!(
                "VM discovery failed; qm: {local_error}; PVE API: {api_error}"
            ))),
        },
    }
}

pub fn helper_info() -> Value {
    let pve_tools = which_exists("qm") || which_exists("pvesh");
    let node = std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "unknown".into());
    json!({
        "helper": "hecate-lampad-proxmox",
        "helper_version": env!("CARGO_PKG_VERSION"),
        "node": node,
        "pve_tools_present": pve_tools,
        "preferred_backend": "local_vnc",
        "fallback_backend": "pve_api",
        "active_sessions": [],
        "mock_mode": std::env::var("HECATE_PROXMOX_MOCK").as_deref() == Ok("1"),
        "socket": crate::default_socket_path(),
        "token_file": "proxmox.ipc.token",
        "methods": [
            "ping", "info", "vm.list", "console.open", "console.frame",
            "console.input", "console.close"
        ],
        "backends": ["local-vnc", "pve-api", "mock"],
        "one_active_console": true
    })
}

fn which_exists(bin: &str) -> bool {
    std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn info_has_stable_shape() {
        let info = super::helper_info();
        assert_eq!(info["helper"], "hecate-lampad-proxmox");
        assert!(info["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "console.open"));
        assert_eq!(info["token_file"], "proxmox.ipc.token");
    }
}
