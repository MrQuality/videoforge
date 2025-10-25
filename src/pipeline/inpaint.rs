//! Inpaint stage filling masked regions with model predictions.

use tokio::task::JoinHandle;

use crate::{FramePayload, PipelineError};

use super::{StageReceiver, StageSender, spawn_stage};
use crate::arc::telemetry::TelemetrySink;

/// Spawns the inpaint stage.
pub fn spawn(
    telemetry: TelemetrySink,
    input: StageReceiver,
    output: StageSender,
) -> JoinHandle<Result<(), PipelineError>> {
    spawn_stage("inpaint", input, output, move |frame| {
        let telemetry = telemetry.clone();
        async move {
            telemetry.record_stage("inpaint", &frame);
            let stride = (frame.metadata.width as usize * frame.metadata.channels as usize).max(1);
            if frame.buffer.is_gpu() {
                Ok(FramePayload::from_cpu_bytes(
                    frame.buffer.data().to_vec(),
                    stride,
                    frame.metadata.clone(),
                ))
            } else {
                Ok(frame)
            }
        }
    })
}
