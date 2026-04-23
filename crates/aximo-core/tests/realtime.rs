use std::{sync::Arc, time::Duration};

use aximo_core::{RealtimeSessionLimits, SessionError, SessionManager};
use tokio::sync::Semaphore;

fn session_permit() -> tokio::sync::OwnedSemaphorePermit {
    Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap()
}

#[test]
fn session_rejects_audio_when_byte_limit_is_exceeded() {
    let manager = SessionManager::new();
    let session_id = manager.start_session(
        session_permit(),
        RealtimeSessionLimits {
            max_bytes: 4,
            max_duration: Duration::from_secs(60),
        },
    );

    assert!(manager.append_audio(&session_id, &[1, 2, 3, 4]).is_ok());
    let error = manager.append_audio(&session_id, &[5]).unwrap_err();

    assert!(matches!(error, SessionError::SessionTooLarge));
}

#[test]
fn session_rejects_audio_when_duration_limit_is_exceeded() {
    let manager = SessionManager::new();
    let session_id = manager.start_session(
        session_permit(),
        RealtimeSessionLimits {
            max_bytes: 1024,
            max_duration: Duration::from_millis(10),
        },
    );

    std::thread::sleep(Duration::from_millis(20));

    let error = manager.append_audio(&session_id, &[1, 2]).unwrap_err();

    assert!(matches!(error, SessionError::SessionTooLong));
}
