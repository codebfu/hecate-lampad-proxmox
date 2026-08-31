//! Copyright (C) 2026 Gaultier HUBERT
//! SPDX-License-Identifier: GPL-3.0-or-later

//! Proxmox VE HTTPS API fallback.

use super::rfb::RfbSession;
use super::{BackendError, ConsoleFrame, ConsoleSession};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

const API_BASE: &str = "https://127.0.0.1:8006/api2/json";
const TOKEN_FILE: &str = "/etc/hecate-lampad/pve-token";

pub struct PveApiBackend {
    inner: RfbSession,
}

impl PveApiBackend {
    pub async fn open(vmid: u32) -> Result<Self, BackendError> {
        let client = client()?;
        let node = node_name()?;
        let response: ApiResponse<VncProxyData> = client
            .post(format!("{API_BASE}/nodes/{node}/qemu/{vmid}/vncproxy"))
            .form(&[("websocket", "0")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let port =
            response.data.port.parse::<u16>().map_err(|error| {
                BackendError::Protocol(format!("invalid API VNC port: {error}"))
            })?;
        Ok(Self {
            inner: RfbSession::connect("127.0.0.1", port, Some(&response.data.ticket), "pve-api")?,
        })
    }
}

impl ConsoleSession for PveApiBackend {
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

pub async fn list_vms() -> Result<Vec<Value>, BackendError> {
    let node = node_name()?;
    let response: ApiResponse<Vec<Value>> = client()?
        .get(format!("{API_BASE}/nodes/{node}/qemu"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response
        .data
        .into_iter()
        .map(|vm| {
            json!({
                "vmid": vm.get("vmid").cloned().unwrap_or(Value::Null),
                "name": vm.get("name").cloned().unwrap_or(Value::Null),
                "status": vm.get("status").cloned().unwrap_or(Value::Null)
            })
        })
        .collect())
}

fn client() -> Result<reqwest::Client, BackendError> {
    ensure_api_base_is_loopback()?;
    let token = read_api_token()?;
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("PVEAPIToken={token}"))
            .map_err(|error| BackendError::Invalid(format!("invalid PVE API token: {error}")))?,
    );
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(5))
        .default_headers(headers)
        .build()
        .map_err(BackendError::from)
}

fn ensure_api_base_is_loopback() -> Result<(), BackendError> {
    let parsed = reqwest::Url::parse(API_BASE)
        .map_err(|error| BackendError::Invalid(format!("invalid PVE API_BASE: {error}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| BackendError::Invalid("PVE API_BASE has no host".into()))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false);
    if loopback {
        Ok(())
    } else {
        Err(BackendError::Invalid(
            "PVE API TLS exemption requires a loopback host".into(),
        ))
    }
}

fn read_api_token() -> Result<String, BackendError> {
    if let Ok(token) = std::env::var("HECATE_PVE_API_TOKEN") {
        return parse_api_token(token);
    }
    let path = Path::new(TOKEN_FILE);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = std::fs::symlink_metadata(path).map_err(|error| {
            BackendError::Unavailable(format!("set HECATE_PVE_API_TOKEN or create {TOKEN_FILE}: {error}"))
        })?;
        if meta.file_type().is_symlink() {
            return Err(BackendError::Invalid(format!(
                "{TOKEN_FILE} must be a regular file (symlink refused)"
            )));
        }
        if meta.uid() != 0 {
            return Err(BackendError::Invalid(format!(
                "{TOKEN_FILE} must be owned by root"
            )));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(BackendError::Invalid(format!(
                "{TOKEN_FILE} must be mode 0600 (or stricter) and not group/world-readable"
            )));
        }
    }
    let token = std::fs::read_to_string(path).map_err(|error| {
        BackendError::Unavailable(format!("set HECATE_PVE_API_TOKEN or create {TOKEN_FILE}: {error}"))
    })?;
    parse_api_token(token)
}

fn parse_api_token(token: String) -> Result<String, BackendError> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(BackendError::Unavailable(format!(
            "set HECATE_PVE_API_TOKEN or create {TOKEN_FILE}"
        )));
    }
    let (identity, secret) = token.split_once('=').ok_or_else(|| {
        BackendError::Invalid("PVE API token must be USER@REALM!TOKENID=SECRET".into())
    })?;
    if !identity.contains('@') || !identity.contains('!') || secret.is_empty() {
        return Err(BackendError::Invalid(
            "PVE API token must be USER@REALM!TOKENID=SECRET".into(),
        ));
    }
    Ok(token)
}

fn node_name() -> Result<String, BackendError> {
    if let Ok(output) = std::process::Command::new("hostname").arg("-s").output() {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return Ok(name);
            }
        }
    }
    std::fs::read_to_string("/etc/hostname")
        .map(|value| value.trim().to_string())
        .map_err(|error| BackendError::Unavailable(format!("cannot determine PVE node: {error}")))
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    data: T,
}

#[derive(Deserialize)]
struct VncProxyData {
    #[serde(deserialize_with = "string_or_number")]
    port: String,
    ticket: String,
}

fn string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(serde::de::Error::custom("expected string or number")),
    }
}
