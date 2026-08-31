//! Copyright (C) 2026 Gaultier HUBERT
//! SPDX-License-Identifier: GPL-3.0-or-later

//! Authenticated local IPC server.

use crate::backend::{self, BackendError};
use crate::session::SessionManager;
use hecate_lampad_helper_base::{
    auth_token_ok, encode_frame, generate_ipc_token, read_frame, set_ipc_socket_permissions,
    IpcErrorBody, IpcRequest, IpcResponse,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub async fn run(socket_path: PathBuf) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::net::UnixListener;

        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }
        let auth_token = generate_ipc_token();
        write_ipc_token_at(&socket_path, &auth_token)?;
        let listener = UnixListener::bind(&socket_path)?;
        set_ipc_socket_permissions(&socket_path)?;
        let sessions = SessionManager::default();
        info!(
            socket = %socket_path.display(),
            token = %proxmox_token_path(&socket_path).display(),
            "listening for Proxmox IPC"
        );
        loop {
            let (mut stream, _) = listener.accept().await?;
            #[cfg(unix)]
            {
                if let Err(error) = reject_untrusted_peer(&stream) {
                    warn!(%error, "rejected Proxmox IPC peer");
                    continue;
                }
            }
            let sessions = sessions.clone();
            let auth_token = auth_token.clone();
            tokio::spawn(async move {
                if let Err(error) = handle_connection(&mut stream, sessions, &auth_token).await {
                    warn!(%error, "Proxmox IPC connection failed");
                }
            });
        }
    }

    #[cfg(not(unix))]
    {
        let _ = socket_path;
        anyhow::bail!("the Proxmox IPC server is only available on Unix; Windows builds are supported for validation")
    }
}

async fn handle_connection<S>(
    stream: &mut S,
    sessions: SessionManager,
    expected_token: &str,
) -> anyhow::Result<()>
where
    S: tokio::io::AsyncReadExt + tokio::io::AsyncWriteExt + Unpin,
{
    let (header, _payload) = read_frame(stream).await?;
    let request: IpcRequest = serde_json::from_slice(&header)?;
    let (response, payload) = if !auth_token_ok(request.auth_token.as_deref(), expected_token) {
        error_response(
            &request.id,
            "unauthorized",
            "invalid or missing IPC auth token",
        )
    } else {
        dispatch(&request, &sessions).await
    };
    let frame = encode_frame(&response, &payload)?;
    stream.write_all(&frame).await?;
    Ok(())
}

async fn dispatch(request: &IpcRequest, sessions: &SessionManager) -> (IpcResponse, Vec<u8>) {
    let result: Result<(Value, Vec<u8>), BackendError> = match request.method.as_str() {
        "ping" => Ok((json!({ "pong": true }), Vec::new())),
        "info" => {
            let mut info = backend::helper_info();
            let summary = sessions.summary().await;
            info["active_console"] = summary.clone();
            if let Some(session_id) = summary.get("session_id").and_then(|v| v.as_str()) {
                info["active_sessions"] = json!([session_id]);
            } else {
                info["active_sessions"] = json!([]);
            }
            Ok((info, Vec::new()))
        }
        "vm.list" => backend::list_vms().await.map(|value| (value, Vec::new())),
        "console.open" => sessions
            .open(&request.params)
            .await
            .map(|value| (value, Vec::new())),
        "console.frame" => sessions.frame(&request.params).await.map(|frame| {
            (
                json!({
                    "width": frame.width,
                    "height": frame.height,
                    "format": frame.format,
                    "content_type": "image/png",
                    "filename": "console-frame.png"
                }),
                frame.bytes,
            )
        }),
        "console.input" => sessions
            .input(&request.params)
            .await
            .map(|value| (value, Vec::new())),
        "console.close" => sessions
            .close(&request.params)
            .await
            .map(|value| (value, Vec::new())),
        method => Err(BackendError::Invalid(format!("unknown method: {method}"))),
    };
    match result {
        Ok((result, payload)) => (
            IpcResponse {
                id: request.id.clone(),
                ok: true,
                result,
                error: None,
            },
            payload,
        ),
        Err(error) => error_response(&request.id, backend_error_code(&error), &error.to_string()),
    }
}

fn backend_error_code(error: &BackendError) -> &'static str {
    match error {
        BackendError::Invalid(_) => "invalid_request",
        BackendError::Unavailable(_) => "backend_unavailable",
        BackendError::Protocol(_) => "console_protocol",
        BackendError::Io(_) => "io_error",
        BackendError::Http(_) => "http_error",
    }
}

fn error_response(id: &str, code: &str, message: &str) -> (IpcResponse, Vec<u8>) {
    (
        IpcResponse {
            id: id.into(),
            ok: false,
            result: Value::Null,
            error: Some(IpcErrorBody {
                code: code.into(),
                message: message.into(),
            }),
        },
        Vec::new(),
    )
}

pub fn proxmox_token_path(socket_path: &Path) -> PathBuf {
    socket_path.with_file_name("proxmox.ipc.token")
}

fn write_ipc_token_at(socket_path: &Path, token: &str) -> std::io::Result<()> {
    let path = proxmox_token_path(socket_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() || std::fs::symlink_metadata(&path).is_ok() {
        let _ = std::fs::remove_file(&path);
    }
    {
        use std::io::Write;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o640)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)?;
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
        }
        #[cfg(not(unix))]
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn reject_untrusted_peer(stream: &tokio::net::UnixStream) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    accept_peer_uid(cred.uid)
}

#[cfg(target_os = "macos")]
fn reject_untrusted_peer(stream: &tokio::net::UnixStream) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    accept_peer_uid(uid)
}

#[cfg(unix)]
fn accept_peer_uid(uid: libc::uid_t) -> std::io::Result<()> {
    let self_uid = unsafe { libc::geteuid() };
    if uid == 0 || uid == self_uid || is_named_uid(uid, "hecate-lampad") {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!("peer uid {uid} is not trusted for Proxmox IPC"),
    ))
}

#[cfg(unix)]
fn is_named_uid(uid: libc::uid_t, name: &str) -> bool {
    let Ok(c_name) = std::ffi::CString::new(name) else {
        return false;
    };
    let pwd = unsafe { libc::getpwnam(c_name.as_ptr()) };
    if pwd.is_null() {
        return false;
    }
    unsafe { (*pwd).pw_uid == uid }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn token_does_not_conflict_with_desktop() {
        assert_eq!(
            super::proxmox_token_path(Path::new("/run/hecate-lampad/proxmox.sock")),
            PathBuf::from("/run/hecate-lampad/proxmox.ipc.token")
        );
    }
}
