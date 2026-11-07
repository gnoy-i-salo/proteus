use super::*;
#[test]
fn clear_cancels_inflight_provider_and_blocks_new_work_until_deletion_finishes() {
    let registry = Operations::default();
    let a = registry.begin("a", JobProvider::Telegram).unwrap();
    let b = registry.begin("b", JobProvider::Github).unwrap();
    let barrier = registry.clear(JobProvider::Telegram).unwrap();
    assert!(a.check().is_err());
    assert!(a.cancelled().load(Ordering::SeqCst));
    assert!(b.check().is_ok());
    assert!(registry.begin("c", JobProvider::Telegram).is_err());
    assert!(registry.clear(JobProvider::Telegram).is_err());
    drop(barrier);
    assert!(a.check().is_err());
    assert!(registry
        .begin("c", JobProvider::Telegram)
        .unwrap()
        .check()
        .is_ok());
}
#[test]
fn completed_operations_release_capacity_and_ids_without_reviving_cancelled_work() {
    let registry = Operations::default();
    let mut active = Vec::new();
    for i in 0..MAX_ACTIVE {
        active.push(registry.begin(&i.to_string(), JobProvider::Vk).unwrap());
    }
    assert!(registry.begin("overflow", JobProvider::Vk).is_err());
    let first = active.remove(0);
    registry.cancel("0").unwrap();
    assert!(first.check().is_err());
    assert!(registry.begin("0", JobProvider::Vk).is_err());
    drop(first);
    assert!(registry.begin("0", JobProvider::Vk).is_ok());
    drop(active);
    assert!(registry.0.lock().unwrap().entries.is_empty());
}
#[test]
fn shutdown_is_irreversible_even_when_a_provider_clear_guard_is_dropped() {
    let registry = Operations::default();
    let a = registry.begin("a", JobProvider::Telegram).unwrap();
    let barrier = registry.clear(JobProvider::Github).unwrap();
    registry.shutdown();
    drop(barrier);
    assert!(a.check().is_err());
    assert!(registry.begin("b", JobProvider::Github).is_err());
    assert!(registry.begin("c", JobProvider::Telegram).is_err());
}
#[test]
fn cancellation_from_another_thread_reaches_the_same_worker_flag() {
    let registry = Operations::default();
    let a = registry.begin("a", JobProvider::Telegram).unwrap();
    let b = registry.begin("b", JobProvider::Telegram).unwrap();
    let copy = registry.clone();
    std::thread::spawn(move || copy.cancel("a").unwrap())
        .join()
        .unwrap();
    assert!(a.check().is_err());
    assert!(b.check().is_ok());
    registry.cancel("missing").unwrap();
}
