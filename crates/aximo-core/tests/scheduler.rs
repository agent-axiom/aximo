use aximo_core::Scheduler;

#[test]
fn scheduler_rejects_when_realtime_capacity_is_exhausted() {
    let scheduler = Scheduler::new(1, 1, 1, 1);
    let first = scheduler.try_acquire_realtime_session();
    let second = scheduler.try_acquire_realtime_session();

    assert!(first.is_ok());
    assert!(second.is_err());
}

#[test]
fn scheduler_limits_realtime_inference_separately_from_session_capacity() {
    let scheduler = Scheduler::new(1, 2, 1, 1);
    let first_session = scheduler.try_acquire_realtime_session();
    let second_session = scheduler.try_acquire_realtime_session();
    let first_inference = scheduler.try_acquire_realtime_inference();
    let second_inference = scheduler.try_acquire_realtime_inference();

    assert!(first_session.is_ok());
    assert!(second_session.is_ok());
    assert!(first_inference.is_ok());
    assert!(second_inference.is_err());
}
