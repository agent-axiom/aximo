use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use tokio::sync::OwnedSemaphorePermit;

#[derive(Clone, Default)]
pub struct SessionManager {
    next_id: Arc<AtomicU64>,
    sessions: Arc<Mutex<HashMap<String, RealtimeSession>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_session(&self, capacity_permit: OwnedSemaphorePermit) -> String {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let session_id = format!("session-{id}");

        self.sessions.lock().expect("session manager lock").insert(
            session_id.clone(),
            RealtimeSession {
                id: session_id.clone(),
                audio_bytes: Vec::new(),
                capacity_permit,
            },
        );

        session_id
    }

    pub fn append_audio(&self, session_id: &str, chunk: &[u8]) -> Result<(), SessionError> {
        let mut sessions = self.sessions.lock().expect("session manager lock");
        let session = sessions
            .get_mut(session_id)
            .ok_or(SessionError::MissingSession)?;

        session.audio_bytes.extend_from_slice(chunk);
        Ok(())
    }

    pub fn finish_session(&self, session_id: &str) -> Result<Vec<u8>, SessionError> {
        let session = self
            .sessions
            .lock()
            .expect("session manager lock")
            .remove(session_id)
            .ok_or(SessionError::MissingSession)?;

        Ok(session.audio_bytes)
    }
}

#[derive(Debug)]
pub enum SessionError {
    MissingSession,
}

#[derive(Debug)]
struct RealtimeSession {
    #[allow(dead_code)]
    id: String,
    audio_bytes: Vec<u8>,
    #[allow(dead_code)]
    capacity_permit: OwnedSemaphorePermit,
}
