//! Policy evaluation helpers used by the ARC coordinator.

use crate::{PipelineError, config::ArcPolicy};

use super::estimator::ResourceEstimate;

/// Result of a policy evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl PolicyDecision {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

/// Evaluates the estimator output against configured constraints.
pub fn evaluate(
    arc_policy: &ArcPolicy,
    estimate: &ResourceEstimate,
    active_jobs: usize,
) -> PolicyDecision {
    if active_jobs >= arc_policy.max_concurrent_jobs as usize {
        return PolicyDecision::deny("concurrency limit reached");
    }
    if estimate.bandwidth_mb > arc_policy.max_gpu_bandwidth_mb {
        return PolicyDecision::deny("predicted GPU bandwidth budget exceeded");
    }
    if estimate.compute_cost_ms as f32 > arc_policy.max_frame_compute_ms {
        return PolicyDecision::deny("estimated compute cost above threshold");
    }
    PolicyDecision::allow()
}

/// Converts a denied decision into a runtime error.
pub fn enforce(decision: PolicyDecision) -> Result<(), PipelineError> {
    if decision.allowed {
        Ok(())
    } else {
        Err(PipelineError::PolicyViolation(
            decision
                .reason
                .unwrap_or_else(|| "policy violation".to_string()),
        ))
    }
}
