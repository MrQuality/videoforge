//! Pipeline orchestration utilities and stage wiring.

use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{instrument, warn};

use crate::{
    FramePayload, FrameResult, PipelineError,
    arc::{coordinator::ArcCoordinator, telemetry::TelemetrySink},
    config::AppConfig,
};

pub mod decode;
pub mod encode;
pub mod inpaint;
pub mod mask;
pub mod temporal;

/// Sender type alias for pipeline stages.
pub type StageSender = mpsc::Sender<FrameResult>;
/// Receiver type alias for pipeline stages.
pub type StageReceiver = mpsc::Receiver<FrameResult>;

/// Creates a bounded channel for connecting two pipeline stages.
pub fn channel(capacity: usize) -> (StageSender, StageReceiver) {
    mpsc::channel(capacity)
}

/// Spawns a processing stage that transforms frames and forwards the result.
pub fn spawn_stage<F, Fut>(
    stage_name: &'static str,
    mut input: StageReceiver,
    output: StageSender,
    mut handler: F,
) -> JoinHandle<Result<(), PipelineError>>
where
    F: FnMut(FramePayload) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = FrameResult> + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(message) = input.recv().await {
            match message {
                Ok(frame) => match handler(frame).await {
                    Ok(next_frame) => {
                        if output.send(Ok(next_frame)).await.is_err() {
                            warn!(
                                target = "pipeline",
                                stage = stage_name,
                                "downstream dropped"
                            );
                            break;
                        }
                    }
                    Err(error) => {
                        if output.send(Err(error.clone())).await.is_err() {
                            warn!(
                                target = "pipeline",
                                stage = stage_name,
                                "downstream dropped error"
                            );
                        }
                        return Err(error);
                    }
                },
                Err(error) => {
                    if output.send(Err(error.clone())).await.is_err() {
                        warn!(
                            target = "pipeline",
                            stage = stage_name,
                            "downstream dropped propagated error"
                        );
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    })
}

/// Executes the configured pipeline using synthetic frames to validate wiring.
#[instrument(skip_all)]
pub async fn execute_pipeline(
    config: AppConfig,
    coordinator: ArcCoordinator,
    telemetry: TelemetrySink,
) -> Result<(), PipelineError> {
    let capacity = config.runtime.channel_capacity as usize;
    let (decode_tx, decode_rx) = channel(capacity);
    let (mask_tx, mask_rx) = channel(capacity);
    let (inpaint_tx, inpaint_rx) = channel(capacity);
    let (temporal_tx, temporal_rx) = channel(capacity);
    let (encode_tx, encode_rx) = channel(capacity);
    let (sink_tx, mut sink_rx) = channel(capacity);

    let decode_handle = decode::spawn(coordinator.clone(), decode_rx, mask_tx);
    let mask_handle = mask::spawn(
        config.policy.clone(),
        telemetry.clone(),
        mask_rx,
        inpaint_tx,
    );
    let inpaint_handle = inpaint::spawn(telemetry.clone(), inpaint_rx, temporal_tx);
    let temporal_handle = temporal::spawn(telemetry.clone(), temporal_rx, encode_tx);
    let encode_handle = encode::spawn(
        telemetry.clone(),
        encode_rx,
        sink_tx,
        config.output_path.clone(),
    );

    for frame_index in 0..config.runtime.dry_run_frames {
        let metadata = crate::FrameMetadata {
            clip_id: config.runtime.clip_id.clone(),
            frame_index,
            timestamp_ms: frame_index * config.runtime.frame_period_ms(),
            width: config.runtime.width,
            height: config.runtime.height,
            channels: config.runtime.channels,
            checksum: format!("{:08x}", frame_index),
            rights_confirmed: config.runtime.confirm_rights,
        };
        metadata.validate(&config.policy)?;
        coordinator.ensure_runtime_compliance(&metadata)?;
        let stride = config.runtime.width as usize * config.runtime.channels as usize;
        let payload = FramePayload::from_cpu_bytes(
            vec![0; stride * config.runtime.height as usize],
            stride,
            metadata,
        );
        decode_tx
            .send(Ok(payload))
            .await
            .map_err(|err| PipelineError::Io(err.to_string()))?;
    }
    drop(decode_tx);

    // Drain results emitted by the encode stage.
    while let Some(result) = sink_rx.recv().await {
        match result {
            Ok(frame) => telemetry.record_completed(&frame),
            Err(err) => return Err(err),
        }
    }

    decode_handle.await??;
    mask_handle.await??;
    inpaint_handle.await??;
    temporal_handle.await??;
    encode_handle.await??;

    Ok(())
}
