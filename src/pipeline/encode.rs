//! Encode stage preparing payloads for persistence.

use std::{path::PathBuf, sync::Arc};

use tokio::{fs, task::JoinHandle};

use crate::{FramePayload, PipelineError};

use super::{StageReceiver, StageSender, spawn_stage};
use crate::arc::telemetry::TelemetrySink;

/// Spawns the encode stage, optionally writing the last frame to disk.
pub fn spawn(
    telemetry: TelemetrySink,
    input: StageReceiver,
    output: StageSender,
    output_path: Option<PathBuf>,
) -> JoinHandle<Result<(), PipelineError>> {
    let output_path = Arc::new(output_path);
    spawn_stage("encode", input, output, move |frame| {
        let telemetry = telemetry.clone();
        let output_path = output_path.clone();
        async move {
            telemetry.record_stage("encode", &frame);
            let stride = (frame.metadata.width as usize * frame.metadata.channels as usize).max(1);
            let encoded = FramePayload::from_cpu_bytes(
                frame.buffer.data().to_vec(),
                stride,
                frame.metadata.clone(),
            );
            if let Some(path) = output_path.as_ref().cloned() {
                fs::write(path, encoded.buffer.data())
                    .await
                    .map_err(|err| PipelineError::Io(err.to_string()))?;
            }
            Ok(encoded)
        }
    })
}
