use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};

use bidmart_auction_service_rust::http::router::METRICS;

fn metrics_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn record_request_increments_total() {
    let _guard = metrics_test_lock();
    // Read current state, increment, verify delta
    let before = METRICS.total_requests.load(Ordering::Relaxed);
    METRICS.record_request(100_000, false); // 100ms
    let after = METRICS.total_requests.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn record_request_error_increments_error_counter() {
    let _guard = metrics_test_lock();
    let before = METRICS.total_errors.load(Ordering::Relaxed);
    METRICS.record_request(1_000, true);
    let after = METRICS.total_errors.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn record_request_non_error_does_not_increment_error() {
    let _guard = metrics_test_lock();
    let before = METRICS.total_errors.load(Ordering::Relaxed);
    METRICS.record_request(1_000, false);
    let after = METRICS.total_errors.load(Ordering::Relaxed);
    assert_eq!(after - before, 0);
}

#[test]
fn apdex_satisfied_for_fast_request() {
    let _guard = metrics_test_lock();
    // 200ms = 200_000us → satisfied (≤500ms)
    let before = METRICS.apdex_satisfied.load(Ordering::Relaxed);
    METRICS.record_request(200_000, false);
    let after = METRICS.apdex_satisfied.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn apdex_satisfied_for_boundary_500ms() {
    let _guard = metrics_test_lock();
    // Exactly 500ms = 500_000us → satisfied
    let before = METRICS.apdex_satisfied.load(Ordering::Relaxed);
    METRICS.record_request(500_000, false);
    let after = METRICS.apdex_satisfied.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn apdex_tolerating_for_slow_request() {
    let _guard = metrics_test_lock();
    // 1000ms = 1_000_000us → tolerating (>500ms, ≤2000ms)
    let before_sat = METRICS.apdex_satisfied.load(Ordering::Relaxed);
    let before_tol = METRICS.apdex_tolerating.load(Ordering::Relaxed);
    METRICS.record_request(1_000_000, false);
    let after_sat = METRICS.apdex_satisfied.load(Ordering::Relaxed);
    let after_tol = METRICS.apdex_tolerating.load(Ordering::Relaxed);
    assert_eq!(after_sat - before_sat, 0);
    assert_eq!(after_tol - before_tol, 1);
}

#[test]
fn apdex_frustrated_for_very_slow_request() {
    let _guard = metrics_test_lock();
    // 3000ms = 3_000_000us → frustrated (>2000ms)
    let before = METRICS.apdex_frustrated.load(Ordering::Relaxed);
    METRICS.record_request(3_000_000, false);
    let after = METRICS.apdex_frustrated.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn histogram_buckets_populated_correctly_for_5ms() {
    let _guard = metrics_test_lock();
    let before = METRICS.latency_le_5ms.load(Ordering::Relaxed);
    METRICS.record_request(3_000, false); // 3ms
    let after = METRICS.latency_le_5ms.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn histogram_inf_bucket_always_incremented() {
    let _guard = metrics_test_lock();
    let before = METRICS.latency_le_inf.load(Ordering::Relaxed);
    METRICS.record_request(999_999_999, false); // very slow
    let after = METRICS.latency_le_inf.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn latency_sum_accumulates() {
    let _guard = metrics_test_lock();
    let before = METRICS.latency_sum_us.load(Ordering::Relaxed);
    METRICS.record_request(50_000, false);
    METRICS.record_request(30_000, false);
    let after = METRICS.latency_sum_us.load(Ordering::Relaxed);
    assert_eq!(after - before, 80_000);
}

#[test]
fn per_endpoint_counters_increment_independently() {
    let _guard = metrics_test_lock();
    let bids_before = METRICS.bids_placed.load(Ordering::Relaxed);
    let created_before = METRICS.auctions_created.load(Ordering::Relaxed);
    let closed_before = METRICS.auctions_closed.load(Ordering::Relaxed);

    METRICS.bids_placed.fetch_add(1, Ordering::Relaxed);
    METRICS.auctions_created.fetch_add(1, Ordering::Relaxed);
    METRICS.auctions_closed.fetch_add(1, Ordering::Relaxed);

    assert_eq!(METRICS.bids_placed.load(Ordering::Relaxed) - bids_before, 1);
    assert_eq!(
        METRICS.auctions_created.load(Ordering::Relaxed) - created_before,
        1
    );
    assert_eq!(
        METRICS.auctions_closed.load(Ordering::Relaxed) - closed_before,
        1
    );
}
