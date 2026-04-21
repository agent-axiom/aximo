use aximo_core::Scheduler;

#[test]
fn scheduler_rejects_when_realtime_capacity_is_exhausted() {
    let scheduler = Scheduler::new(1, 1);
    let first = scheduler.try_acquire_realtime();
    let second = scheduler.try_acquire_realtime();

    assert!(first.is_ok());
    assert!(second.is_err());
}
