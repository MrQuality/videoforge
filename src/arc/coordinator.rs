//! ARC coordinator ensuring runtime compliance with policy limits.

use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::instrument;

use crate::{
    FrameMetadata, FramePayload, PipelineError,
    config::{ArcPolicy, PolicyLimits},
};

use super::{
    actuators::{ArcActuator, NullActuator},
    estimator::{ResourceEstimate, ResourceEstimator},
    policy,
    telemetry::TelemetrySink,
};

/// Coordinator wiring together telemetry, estimation, and policy enforcement.
#[derive(Clone)]
pub struct ArcCoordinator {
    telemetry: TelemetrySink,
    policy_limits: PolicyLimits,
    arc_policy: ArcPolicy,
    estimator: ResourceEstimator,
    semaphore: Arc<Semaphore>,
    actuator: Arc<dyn ArcActuator>,
}

impl ArcCoordinator {
    pub fn new(
        telemetry: TelemetrySink,
        policy_limits: PolicyLimits,
        arc_policy: ArcPolicy,
    ) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(arc_policy.max_concurrent_jobs as usize)),
            telemetry,
            policy_limits,
            arc_policy,
            estimator: ResourceEstimator::default(),
            actuator: Arc::new(NullActuator::default()),
        }
    }

    pub fn with_actuator<A: ArcActuator + 'static>(mut self, actuator: A) -> Self {
        self.actuator = Arc::new(actuator);
        self
    }

    pub fn telemetry(&self) -> TelemetrySink {
        self.telemetry.clone()
    }

    pub fn ensure_runtime_compliance(&self, metadata: &FrameMetadata) -> Result<(), PipelineError> {
        metadata.validate(&self.policy_limits)
    }

    pub fn record_stage(&self, stage: &'static str, frame: &FramePayload) {
        self.telemetry.record_stage(stage, frame);
    }

    pub fn active_jobs(&self) -> usize {
        self.arc_policy.max_concurrent_jobs as usize - self.semaphore.available_permits()
    }

    #[instrument(skip_all, fields(clip = %metadata.clip_id, frame = metadata.frame_index))]
    pub async fn acquire(&self, metadata: &FrameMetadata) -> Result<ArcPermit, PipelineError> {
        let estimate = self.estimator.estimate(metadata);
        self.evaluate_policy(&estimate)?;
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| PipelineError::Io("semaphore closed".to_string()))?;
        self.actuator.apply(&estimate).await?;
        Ok(ArcPermit {
            permit: Some(permit),
            estimate,
        })
    }

    fn evaluate_policy(&self, estimate: &ResourceEstimate) -> Result<(), PipelineError> {
        let decision = policy::evaluate(&self.arc_policy, estimate, self.active_jobs());
        policy::enforce(decision)
    }
}

/// Permit releasing ARC capacity on drop.
pub struct ArcPermit {
    permit: Option<OwnedSemaphorePermit>,
    estimate: ResourceEstimate,
}

impl ArcPermit {
    pub fn estimate(&self) -> &ResourceEstimate {
        &self.estimate
    }
}

impl Drop for ArcPermit {
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            drop(permit);
        }
    }
}
