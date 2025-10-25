//! Hardware actuators invoked by ARC decisions.

use async_trait::async_trait;

use crate::PipelineError;

use super::estimator::ResourceEstimate;

/// Trait implemented by GPU/CPU actuators.
#[async_trait]
pub trait ArcActuator: Send + Sync {
    async fn apply(&self, estimate: &ResourceEstimate) -> Result<(), PipelineError>;
}

/// No-op actuator used for environments without hardware bindings.
pub struct NullActuator;

impl Default for NullActuator {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl ArcActuator for NullActuator {
    async fn apply(&self, _estimate: &ResourceEstimate) -> Result<(), PipelineError> {
        Ok(())
    }
}
