//! Copyright (C) 2026 Gaultier HUBERT
//! SPDX-License-Identifier: GPL-3.0-or-later

//! Local Proxmox backend using `qm`.

use super::rfb::RfbSession;
use super::{BackendError, ConsoleFrame, ConsoleSession};
use serde_json::{json, Value};
use std::process::Command;

pub struct LocalVncBackend {
    inner: RfbSession,
}

impl LocalVncBackend {
    pub fn open(vmid: u32) -> Result<Self, BackendError> {
        let output = Command::new("qm")
            .args(["vncproxy", &vmid.to_string()])
            .output()
            .map_err(|error| BackendError::Unavailable(format!("cannot run qm: {error}")))?;
        if !output.status.success() {
            return Err(BackendError::Unavailable(format!(
                "qm vncproxy exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let proxy = parse_vncproxy_output(&text)?;
        Ok(Self {
            inner: RfbSession::connect(
                "127.0.0.1",
                proxy.port,
                proxy.ticket.as_deref(),
                "local-vnc",
            )?,
        })
    }
}

impl ConsoleSession for LocalVncBackend {
    fn backend_kind(&self) -> &'static str {
        self.inner.backend_kind()
    }

    fn dimensions(&self) -> (u32, u32) {
        self.inner.dimensions()
    }

    fn frame(&mut self) -> Result<ConsoleFrame, BackendError> {
        self.inner.frame()
    }

    fn input(&mut self, events: &[Value]) -> Result<(), BackendError> {
        self.inner.input(events)
    }

    fn close(&mut self) -> Result<(), BackendError> {
        self.inner.close()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct VncProxyOutput {
    pub port: u16,
    pub ticket: Option<String>,
}

pub fn parse_vncproxy_output(text: &str) -> Result<VncProxyOutput, BackendError> {
    let mut port = None;
    let mut ticket = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .or_else(|| line.split_once('='))
            .unwrap_or(("", line));
        let normalized = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_matches('"');
        if normalized.contains("ticket") || normalized.contains("password") {
            ticket = Some(value.to_string());
        } else if normalized.contains("port") {
            port = value.parse().ok();
        } else if port.is_none() {
            port = value
                .split_whitespace()
                .find_map(|part| part.parse::<u16>().ok().filter(|value| *value >= 5900));
        }
    }
    let port = port.ok_or_else(|| {
        BackendError::Protocol(format!(
            "qm vncproxy output contained no port: {}",
            text.trim()
        ))
    })?;
    Ok(VncProxyOutput { port, ticket })
}

pub fn list_vms() -> Result<Vec<Value>, BackendError> {
    let output = Command::new("qm")
        .arg("list")
        .output()
        .map_err(|error| BackendError::Unavailable(format!("cannot run qm: {error}")))?;
    if !output.status.success() {
        return Err(BackendError::Unavailable(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let mut vms = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(vmid) = fields.first().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        vms.push(json!({
            "vmid": vmid,
            "name": fields.get(1).copied().unwrap_or(""),
            "status": fields.get(2).copied().unwrap_or("unknown")
        }));
    }
    Ok(vms)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_qm_vncproxy_output() {
        let parsed =
            super::parse_vncproxy_output("PORT: 5901\nTICKET: PVEVNC:65ABCD::signed-ticket\n")
                .unwrap();
        assert_eq!(parsed.port, 5901);
        assert_eq!(
            parsed.ticket.as_deref(),
            Some("PVEVNC:65ABCD::signed-ticket")
        );
    }

    #[test]
    fn parses_bare_port() {
        assert_eq!(super::parse_vncproxy_output("5902\n").unwrap().port, 5902);
    }
}
