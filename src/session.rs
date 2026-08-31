//! Copyright (C) 2026 Gaultier HUBERT
//! SPDX-License-Identifier: GPL-3.0-or-later

//! Single-console session state machine.

use crate::backend::{self, BackendError, ConsoleFrame, ConsoleSession};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct SessionManager {
    active: Arc<Mutex<Option<ActiveSession>>>,
}

struct ActiveSession {
    id: String,
    vmid: u32,
    console: Box<dyn ConsoleSession>,
}

impl SessionManager {
    pub async fn open(&self, params: &Value) -> Result<Value, BackendError> {
        let vmid = params
            .get("vmid")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| BackendError::Invalid("vmid is required".into()))?;
        let console = backend::open_console(vmid).await?;
        self.open_with_console(vmid, console).await
    }

    async fn open_with_console(
        &self,
        vmid: u32,
        console: Box<dyn ConsoleSession>,
    ) -> Result<Value, BackendError> {
        let mut active = self.active.lock().await;
        if active.is_some() {
            return Err(BackendError::Invalid(
                "one console is already active; close it before opening another".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let backend = console.backend_kind();
        let (width, height) = console.dimensions();
        *active = Some(ActiveSession {
            id: id.clone(),
            vmid,
            console,
        });
        Ok(json!({
            "session_id": id,
            "vmid": vmid,
            "backend": backend,
            "width": width,
            "height": height
        }))
    }

    pub async fn frame(&self, params: &Value) -> Result<ConsoleFrame, BackendError> {
        let mut active = self.active.lock().await;
        let session = matching_session(active.as_mut(), params)?;
        session.console.frame()
    }

    pub async fn input(&self, params: &Value) -> Result<Value, BackendError> {
        let events = params
            .get("events")
            .and_then(Value::as_array)
            .ok_or_else(|| BackendError::Invalid("events array is required".into()))?;
        let mut active = self.active.lock().await;
        let session = matching_session(active.as_mut(), params)?;
        session.console.input(events)?;
        Ok(json!({ "accepted": events.len() }))
    }

    pub async fn close(&self, params: &Value) -> Result<Value, BackendError> {
        let mut active = self.active.lock().await;
        {
            let session = matching_session(active.as_mut(), params)?;
            session.console.close()?;
        }
        let closed = active.take().expect("session was checked");
        Ok(json!({
            "session_id": closed.id,
            "vmid": closed.vmid,
            "closed": true
        }))
    }

    pub async fn summary(&self) -> Value {
        match self.active.lock().await.as_ref() {
            Some(session) => json!({
                "session_id": session.id,
                "vmid": session.vmid,
                "backend": session.console.backend_kind()
            }),
            None => Value::Null,
        }
    }
}

fn matching_session<'a>(
    active: Option<&'a mut ActiveSession>,
    params: &Value,
) -> Result<&'a mut ActiveSession, BackendError> {
    let session = active.ok_or_else(|| BackendError::Invalid("no active console".into()))?;
    if let Some(requested) = params.get("session_id").and_then(Value::as_str) {
        if requested != session.id {
            return Err(BackendError::Invalid(
                "session_id does not match active console".into(),
            ));
        }
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::SessionManager;
    use crate::backend::mock::MockBackend;
    use serde_json::json;

    #[tokio::test]
    async fn enforces_one_active_console_and_allows_reopen_after_close() {
        let manager = SessionManager::default();
        let opened = manager
            .open_with_console(100, Box::new(MockBackend::open(100).unwrap()))
            .await
            .unwrap();
        assert!(manager
            .open_with_console(101, Box::new(MockBackend::open(101).unwrap()))
            .await
            .is_err());
        let id = opened["session_id"].as_str().unwrap();
        manager.close(&json!({ "session_id": id })).await.unwrap();
        assert!(manager
            .open_with_console(101, Box::new(MockBackend::open(101).unwrap()))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn frame_requires_an_active_session() {
        let manager = SessionManager::default();
        assert!(manager.frame(&json!({})).await.is_err());
    }
}
