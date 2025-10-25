//! Mask stage generating matte predictions.

use tokio::task::JoinHandle;

use crate::{FramePayload, PipelineError, config::PolicyLimits};

use super::{StageReceiver, StageSender, spawn_stage};
use crate::arc::telemetry::TelemetrySink;

/// Spawns the mask inference stage.
pub fn spawn(
    policy: PolicyLimits,
    telemetry: TelemetrySink,
    input: StageReceiver,
    output: StageSender,
) -> JoinHandle<Result<(), PipelineError>> {
    spawn_stage("mask", input, output, move |frame| {
        let telemetry = telemetry.clone();
        let policy = policy.clone();
        async move {
            frame.metadata.validate(&policy)?;
            telemetry.record_stage("mask", &frame);
            Ok(frame)
        }
    })
}
