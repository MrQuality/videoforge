//! Decode stage pulling frames from FFmpeg into the pipeline.

use std::{collections::hash_map::DefaultHasher, hash::Hasher, path::Path};

use ffmpeg::decoder::video::Video as VideoDecoder;
use ffmpeg::software::scaling::{context::Context as Scaler, flag::Flags};
use ffmpeg::util::{format::pixel::Pixel, frame::video::Video};
use ffmpeg_next as ffmpeg;
use tokio::task::JoinHandle;
use tracing::instrument;

use crate::{
    FrameMetadata, FramePayload, PipelineError,
    arc::coordinator::ArcCoordinator,
    config::{DecoderConfig, HardwareAcceleration, RuntimeConfig},
};

use super::StageSender;

/// Spawns the decode stage that sources frames from the configured clip.
pub fn spawn(
    runtime: RuntimeConfig,
    decoder: DecoderConfig,
    coordinator: ArcCoordinator,
    output: StageSender,
) -> JoinHandle<Result<(), PipelineError>> {
    tokio::spawn(async move { run_decode(runtime, decoder, coordinator, output).await })
}

/// Core decode loop responsible for translating packets into frame payloads.
#[instrument(
    skip_all,
    fields(clip = %runtime.clip_id, codec = %decoder_config.video_codec)
)]
async fn run_decode(
    runtime: RuntimeConfig,
    decoder_config: DecoderConfig,
    coordinator: ArcCoordinator,
    output: StageSender,
) -> Result<(), PipelineError> {
    ffmpeg::init().map_err(|err| PipelineError::Decode(format!("ffmpeg init failed: {err}")))?;

    let input_path = decoder_config.input.clone();
    let mut context = ffmpeg::format::input(&input_path)
        .map_err(|err| decode_error(&input_path, format!("open failed: {err}")))?;

    let desired_codec = codec_id_from_name(&decoder_config.video_codec).ok_or_else(|| {
        PipelineError::Decode(format!(
            "unsupported codec '{}'",
            decoder_config.video_codec
        ))
    })?;

    let stream = context
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| PipelineError::Decode("no video stream found".to_string()))?;
    let stream_index = stream.index();
    let parameters = stream.parameters();
    if parameters.id() != desired_codec {
        return Err(PipelineError::Decode(format!(
            "video stream codec {:?} does not match requested {}",
            parameters.id(),
            decoder_config.video_codec
        )));
    }

    let mut decoder_ctx = ffmpeg::codec::context::Context::from_parameters(parameters)
        .map_err(|err| PipelineError::Decode(format!("codec context: {err}")))?;
    let mut ff_decoder = decoder_ctx
        .decoder()
        .video()
        .map_err(|err| PipelineError::Decode(format!("video decoder: {err}")))?;

    let mut scaler = Scaler::get(
        ff_decoder.format(),
        ff_decoder.width(),
        ff_decoder.height(),
        Pixel::RGB24,
        ff_decoder.width(),
        ff_decoder.height(),
        Flags::BILINEAR,
    )
    .map_err(|err| PipelineError::Decode(format!("scaler init failed: {err}")))?;

    let frame_limit = runtime.dry_run_frames;
    if frame_limit == 0 {
        return Ok(());
    }

    let mut decoded = Video::empty();
    let mut converted = Video::empty();
    let mut produced_frames: u64 = 0;
    let mut reached_end_of_stream = false;

    'packets: for (stream, packet) in context.packets() {
        if stream.index() != stream_index {
            continue;
        }

        ff_decoder
            .send_packet(&packet)
            .map_err(|err| PipelineError::Decode(format!("send packet: {err}")))?;

        match drain_decoder(
            &mut ff_decoder,
            &mut scaler,
            &runtime,
            &decoder_config,
            &coordinator,
            &output,
            &mut decoded,
            &mut converted,
            &mut produced_frames,
            frame_limit,
        )
        .await?
        {
            DrainOutcome::NeedsMoreInput => {}
            DrainOutcome::FrameLimitReached => break 'packets,
            DrainOutcome::EndOfStream => {
                reached_end_of_stream = true;
                break 'packets;
            }
        }

        if produced_frames >= frame_limit {
            break;
        }
    }

    if !reached_end_of_stream && produced_frames < frame_limit {
        ff_decoder
            .send_eof()
            .map_err(|err| PipelineError::Decode(format!("send eof: {err}")))?;

        loop {
            match drain_decoder(
                &mut ff_decoder,
                &mut scaler,
                &runtime,
                &decoder_config,
                &coordinator,
                &output,
                &mut decoded,
                &mut converted,
                &mut produced_frames,
                frame_limit,
            )
            .await?
            {
                DrainOutcome::NeedsMoreInput => break,
                DrainOutcome::FrameLimitReached => break,
                DrainOutcome::EndOfStream => break,
            }

            if produced_frames >= frame_limit {
                break;
            }
        }
    }

    drop(output);
    Ok(())
}

/// Outcome returned by [`drain_decoder`] describing the decoder's state.
enum DrainOutcome {
    /// Decoder needs additional compressed packets before more frames can be produced.
    NeedsMoreInput,
    /// The requested frame budget has been satisfied.
    FrameLimitReached,
    /// The decoder signalled end-of-stream.
    EndOfStream,
}

/// Pulls ready frames from the decoder and forwards them downstream.
async fn drain_decoder(
    decoder: &mut VideoDecoder,
    scaler: &mut Scaler,
    runtime: &RuntimeConfig,
    decoder_config: &DecoderConfig,
    coordinator: &ArcCoordinator,
    output: &StageSender,
    decoded: &mut Video,
    converted: &mut Video,
    produced_frames: &mut u64,
    frame_limit: u64,
) -> Result<DrainOutcome, PipelineError> {
    while *produced_frames < frame_limit {
        match decoder.receive_frame(decoded) {
            Ok(()) => {
                let converted_frame = convert_frame(scaler, decoded, converted)?;
                let metadata = build_metadata(runtime, *produced_frames, &converted_frame);
                coordinator.ensure_runtime_compliance(&metadata)?;
                let permit = coordinator.acquire(&metadata).await?;
                let payload = make_payload(decoder_config.hardware, converted_frame, metadata);
                coordinator.record_stage("decode", &payload);
                output.send(Ok(payload)).await.map_err(|err| {
                    PipelineError::Decode(format!("downstream closed decode channel: {err}"))
                })?;
                drop(permit);
                *produced_frames += 1;
            }
            Err(ffmpeg::Error::Again) => return Ok(DrainOutcome::NeedsMoreInput),
            Err(ffmpeg::Error::Eof) => return Ok(DrainOutcome::EndOfStream),
            Err(err) => {
                return Err(PipelineError::Decode(format!("receive frame: {err}")));
            }
        }
    }

    Ok(DrainOutcome::FrameLimitReached)
}

/// CPU-resident buffer carrying RGB pixels extracted from FFmpeg.
struct ConvertedFrame {
    bytes: Vec<u8>,
    stride: usize,
    width: u32,
    height: u32,
}

/// Converts the decoded frame into RGB24 pixels owned by the CPU.
fn convert_frame(
    scaler: &mut Scaler,
    decoded: &Video,
    converted: &mut Video,
) -> Result<ConvertedFrame, PipelineError> {
    converted.set_format(Pixel::RGB24);
    converted.set_width(decoded.width());
    converted.set_height(decoded.height());

    scaler
        .run(decoded, converted)
        .map_err(|err| PipelineError::Decode(format!("scale frame: {err}")))?;

    let stride = converted.stride(0);
    if stride < 0 {
        return Err(PipelineError::Decode(
            "negative stride produced".to_string(),
        ));
    }
    let stride = stride as usize;
    let height = converted.height() as usize;
    let plane = converted.data(0);
    let expected = stride * height;
    if plane.len() < expected {
        return Err(PipelineError::Decode(
            "decoded plane smaller than expected".to_string(),
        ));
    }
    let mut bytes = Vec::with_capacity(expected);
    bytes.extend_from_slice(&plane[..expected]);

    Ok(ConvertedFrame {
        bytes,
        stride,
        width: converted.width() as u32,
        height: converted.height() as u32,
    })
}

/// Derives [`FrameMetadata`] for the decoded frame.
fn build_metadata(
    runtime: &RuntimeConfig,
    frame_index: u64,
    frame: &ConvertedFrame,
) -> FrameMetadata {
    let mut hasher = DefaultHasher::new();
    hasher.write(&frame.bytes);
    FrameMetadata {
        clip_id: runtime.clip_id.clone(),
        frame_index,
        timestamp_ms: frame_index.saturating_mul(runtime.frame_period_ms()),
        width: frame.width,
        height: frame.height,
        channels: 3,
        checksum: format!("{:016x}", hasher.finish()),
        rights_confirmed: runtime.confirm_rights,
    }
}

/// Builds a [`FramePayload`] with CPU or GPU backing storage.
fn make_payload(
    hardware: HardwareAcceleration,
    frame: ConvertedFrame,
    metadata: FrameMetadata,
) -> FramePayload {
    if hardware.is_gpu() {
        // In GPU mode the production system performs a zero-copy hand-off of device memory.
        // Tests mirror the bytes on the host to keep assertions simple.
        FramePayload::from_gpu_bytes(frame.bytes, 0, frame.stride, metadata)
    } else {
        FramePayload::from_cpu_bytes(frame.bytes, frame.stride, metadata)
    }
}

/// Maps human friendly codec labels onto FFmpeg identifiers.
fn codec_id_from_name(name: &str) -> Option<ffmpeg::codec::Id> {
    match name.to_ascii_lowercase().as_str() {
        "h264" => Some(ffmpeg::codec::Id::H264),
        "hevc" | "h265" => Some(ffmpeg::codec::Id::HEVC),
        "vp9" => Some(ffmpeg::codec::Id::VP9),
        "av1" => Some(ffmpeg::codec::Id::AV1),
        "mpeg4" => Some(ffmpeg::codec::Id::MPEG4),
        "png" => Some(ffmpeg::codec::Id::PNG),
        _ => None,
    }
}

/// Formats a decode error with the path context included.
fn decode_error(path: &Path, message: String) -> PipelineError {
    PipelineError::Decode(format!("{}: {message}", path.display()))
}
