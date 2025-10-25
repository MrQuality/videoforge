//! Decode stage converting CPU ingress buffers into GPU-ready payloads.

use tokio::task::JoinHandle;

use crate::{FrameBuffer, FramePayload, PipelineError, arc::coordinator::ArcCoordinator};

use super::{StageReceiver, StageSender, spawn_stage};

/// Spawns the decode stage.
pub fn spawn(
    coordinator: ArcCoordinator,
    input: StageReceiver,
    output: StageSender,
) -> JoinHandle<Result<(), PipelineError>> {
    spawn_stage("decode", input, output, move |frame| {
        let coordinator = coordinator.clone();
        async move {
            coordinator.ensure_runtime_compliance(&frame.metadata)?;
            let pitch = (frame.metadata.width as usize * frame.metadata.channels as usize).max(1);
            if frame.buffer.is_gpu() {
                coordinator.record_stage("decode", &frame);
                Ok(frame)
            } else {
                let payload = FramePayload::from_gpu_bytes(
                    frame.buffer.data().to_vec(),
                    0,
                    pitch,
                    frame.metadata.clone(),
                );
                coordinator.record_stage("decode", &payload);
                Ok(payload)
            }
        }
    })
}
