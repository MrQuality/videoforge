//! Temporal consistency stage enforcing monotonic timestamps.

use std::sync::Arc;

use tokio::{sync::Mutex, task::JoinHandle};

use crate::PipelineError;

use super::{StageReceiver, StageSender, spawn_stage};
use crate::arc::telemetry::TelemetrySink;

/// Spawns the temporal smoothing stage.
pub fn spawn(
    telemetry: TelemetrySink,
    input: StageReceiver,
    output: StageSender,
) -> JoinHandle<Result<(), PipelineError>> {
    let last_timestamp = Arc::new(Mutex::new(None::<u64>));
    spawn_stage("temporal", input, output, move |frame| {
        let telemetry = telemetry.clone();
        let state = last_timestamp.clone();
        async move {
            telemetry.record_stage("temporal", &frame);
            let mut guard = state.lock().await;
            if let Some(prev) = *guard {
                if frame.metadata.timestamp_ms < prev {
                    return Err(PipelineError::Temporal(format!(
                        "timestamp regression: {} -> {}",
                        prev, frame.metadata.timestamp_ms
                    )));
                }
            }
            *guard = Some(frame.metadata.timestamp_ms);
            Ok(frame)
        }
    })
}
