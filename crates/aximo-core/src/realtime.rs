use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
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

    pub fn start_session(
        &self,
        capacity_permit: OwnedSemaphorePermit,
        limits: RealtimeSessionLimits,
    ) -> String {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let session_id = format!("session-{id}");

        self.sessions.lock().expect("session manager lock").insert(
            session_id.clone(),
            RealtimeSession {
                id: session_id.clone(),
                audio_bytes: Vec::new(),
                started_at: Instant::now(),
                limits,
                last_partial_at: None,
                bytes_since_partial: 0,
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

        if session.started_at.elapsed() > session.limits.max_duration {
            return Err(SessionError::SessionTooLong);
        }

        if session.audio_bytes.len().saturating_add(chunk.len()) > session.limits.max_bytes {
            return Err(SessionError::SessionTooLarge);
        }

        session.audio_bytes.extend_from_slice(chunk);
        session.bytes_since_partial = session.bytes_since_partial.saturating_add(chunk.len());
        Ok(())
    }

    pub fn should_schedule_partial(
        &self,
        session_id: &str,
        limits: RealtimePartialLimits,
    ) -> Result<bool, SessionError> {
        let sessions = self.sessions.lock().expect("session manager lock");
        let session = sessions
            .get(session_id)
            .ok_or(SessionError::MissingSession)?;

        let enough_audio = session.bytes_since_partial >= limits.min_chunk_bytes.max(1);
        let enough_time = session
            .last_partial_at
            .map(|last_partial_at| last_partial_at.elapsed() >= limits.min_interval)
            .unwrap_or(true);

        Ok(enough_audio && enough_time)
    }

    pub fn mark_partial_started(&self, session_id: &str) -> Result<(), SessionError> {
        let mut sessions = self.sessions.lock().expect("session manager lock");
        let session = sessions
            .get_mut(session_id)
            .ok_or(SessionError::MissingSession)?;

        session.last_partial_at = Some(Instant::now());
        session.bytes_since_partial = 0;
        Ok(())
    }

    pub fn audio_snapshot(&self, session_id: &str) -> Result<Vec<u8>, SessionError> {
        let sessions = self.sessions.lock().expect("session manager lock");
        let session = sessions
            .get(session_id)
            .ok_or(SessionError::MissingSession)?;

        Ok(session.audio_bytes.clone())
    }

    pub fn recent_audio_snapshot(
        &self,
        session_id: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, SessionError> {
        let sessions = self.sessions.lock().expect("session manager lock");
        let session = sessions
            .get(session_id)
            .ok_or(SessionError::MissingSession)?;

        let audio = &session.audio_bytes;
        let recent_len = audio.len().min(max_bytes.max(1));
        let start = audio.len().saturating_sub(recent_len);

        Ok(audio[start..].to_vec())
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
    SessionTooLarge,
    SessionTooLong,
}

#[derive(Debug, Clone, Copy)]
pub struct RealtimeSessionLimits {
    pub max_bytes: usize,
    pub max_duration: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct RealtimePartialLimits {
    pub min_interval: Duration,
    pub min_chunk_bytes: usize,
}

#[derive(Debug)]
struct RealtimeSession {
    #[allow(dead_code)]
    id: String,
    audio_bytes: Vec<u8>,
    started_at: Instant,
    limits: RealtimeSessionLimits,
    last_partial_at: Option<Instant>,
    bytes_since_partial: usize,
    #[allow(dead_code)]
    capacity_permit: OwnedSemaphorePermit,
}
