use videoforge::{
    FrameMetadata,
    arc::{estimator::ResourceEstimator, policy},
    config::ArcPolicy,
};

fn metadata() -> FrameMetadata {
    FrameMetadata {
        clip_id: "clip".into(),
        frame_index: 0,
        timestamp_ms: 0,
        width: 640,
        height: 480,
        channels: 3,
        checksum: "deadbeef".into(),
        rights_confirmed: true,
    }
}

#[test]
fn policy_allows_under_limits() {
    let arc_policy = ArcPolicy {
        max_concurrent_jobs: 4,
        max_gpu_bandwidth_mb: 500.0,
        max_frame_compute_ms: 50.0,
    };
    let estimator = ResourceEstimator;
    let estimate = estimator.estimate(&metadata());
    let decision = policy::evaluate(&arc_policy, &estimate, 0);
    assert!(decision.allowed);
}

#[test]
fn policy_blocks_concurrency_overflow() {
    let arc_policy = ArcPolicy {
        max_concurrent_jobs: 1,
        max_gpu_bandwidth_mb: 500.0,
        max_frame_compute_ms: 50.0,
    };
    let estimator = ResourceEstimator;
    let estimate = estimator.estimate(&metadata());
    let decision = policy::evaluate(&arc_policy, &estimate, 1);
    assert!(!decision.allowed);
}
