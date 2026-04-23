use std::{sync::Arc, time::Duration};

use aximo_core::{
    PartialSchedule, RealtimePartialLimits, RealtimeSessionLimits, SessionError, SessionManager,
};
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

#[test]
fn session_marks_partial_dirty_when_new_partial_arrives_inflight() {
    let manager = SessionManager::new();
    let session_id = manager.start_session(
        session_permit(),
        RealtimeSessionLimits {
            max_bytes: 4096,
            max_duration: Duration::from_secs(60),
        },
    );
    let partial_limits = RealtimePartialLimits {
        min_interval: Duration::ZERO,
        min_chunk_bytes: 4,
    };

    manager.append_audio(&session_id, &[1, 2, 3, 4]).unwrap();
    let first_schedule = manager
        .maybe_begin_partial(&session_id, partial_limits)
        .unwrap();
    assert_eq!(first_schedule, PartialSchedule::StartNow);

    manager.append_audio(&session_id, &[5, 6, 7, 8]).unwrap();
    let second_schedule = manager
        .maybe_begin_partial(&session_id, partial_limits)
        .unwrap();
    assert_eq!(second_schedule, PartialSchedule::Skip);

    let follow_up = manager.complete_partial(&session_id).unwrap();
    assert_eq!(follow_up, PartialSchedule::StartNow);
}

#[test]
fn session_skips_follow_up_partial_when_not_dirty() {
    let manager = SessionManager::new();
    let session_id = manager.start_session(
        session_permit(),
        RealtimeSessionLimits {
            max_bytes: 4096,
            max_duration: Duration::from_secs(60),
        },
    );
    let partial_limits = RealtimePartialLimits {
        min_interval: Duration::ZERO,
        min_chunk_bytes: 4,
    };

    manager.append_audio(&session_id, &[1, 2, 3, 4]).unwrap();
    let first_schedule = manager
        .maybe_begin_partial(&session_id, partial_limits)
        .unwrap();
    assert_eq!(first_schedule, PartialSchedule::StartNow);

    let follow_up = manager.complete_partial(&session_id).unwrap();
    assert_eq!(follow_up, PartialSchedule::Skip);
}
