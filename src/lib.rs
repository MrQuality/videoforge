//! Core library for the Videoforge pipeline.
//!
//! The crate exposes data models shared across the asynchronous pipeline stages,
//! configuration loading utilities, and the orchestration entry point used by
//! the CLI application.

pub mod arc;
pub mod config;
pub mod pipeline;

use std::{fmt::Display, sync::Arc};

use thiserror::Error;
use tracing::instrument;

/// Convenient alias for a shared byte buffer.
pub type SharedBytes = Arc<[u8]>;

/// Enumeration covering the supported frame buffer memory locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameBuffer {
    /// CPU resident planar or packed pixel buffer.
    Cpu {
        /// Frame bytes stored on the heap.
        data: SharedBytes,
        /// Number of bytes between the start of two consecutive rows.
        stride: usize,
    },
    /// GPU resident linear buffer. For testing the bytes are mirrored on the CPU.
    Gpu {
        /// Mirrored host copy allowing validation without CUDA access.
        data: SharedBytes,
        /// CUDA device identifier hosting the actual allocation.
        device_id: u32,
        /// Pitch (bytes per row) in device memory.
        pitch: usize,
    },
}

impl FrameBuffer {
    /// Returns the length in bytes of the mirrored buffer.
    pub fn len(&self) -> usize {
        match self {
            FrameBuffer::Cpu { data, .. } | FrameBuffer::Gpu { data, .. } => data.len(),
        }
    }

    /// Returns true when the mirrored buffer has no bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Indicates whether the buffer is resident on a GPU device.
    pub fn is_gpu(&self) -> bool {
        matches!(self, FrameBuffer::Gpu { .. })
    }

    /// Helper to access the mirrored bytes.
    pub fn data(&self) -> &[u8] {
        match self {
            FrameBuffer::Cpu { data, .. } | FrameBuffer::Gpu { data, .. } => data,
        }
    }
}

/// Metadata describing a single frame travelling through the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameMetadata {
    pub clip_id: String,
    pub frame_index: u64,
    pub timestamp_ms: u64,
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub checksum: String,
    pub rights_confirmed: bool,
}

impl FrameMetadata {
    /// Validates the metadata against runtime policy constraints.
    pub fn validate(&self, policy: &config::PolicyLimits) -> Result<(), PipelineError> {
        if !self.rights_confirmed {
            return Err(PipelineError::PolicyViolation(
                "content rights must be confirmed before processing".to_string(),
            ));
        }
        if self.width > policy.max_width || self.height > policy.max_height {
            return Err(PipelineError::PolicyViolation(format!(
                "resolution {}x{} exceeds policy bound {}x{}",
                self.width, self.height, policy.max_width, policy.max_height
            )));
        }
        if self.frame_index >= policy.max_frames_per_clip {
            return Err(PipelineError::PolicyViolation(format!(
                "frame index {} exceeds policy clip length {}",
                self.frame_index, policy.max_frames_per_clip
            )));
        }
        Ok(())
    }
}

/// Payload travelling across the video pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePayload {
    pub buffer: FrameBuffer,
    pub metadata: FrameMetadata,
}

impl FramePayload {
    /// Helper for constructing CPU payloads.
    pub fn from_cpu_bytes(data: Vec<u8>, stride: usize, metadata: FrameMetadata) -> Self {
        Self {
            buffer: FrameBuffer::Cpu {
                data: data.into(),
                stride,
            },
            metadata,
        }
    }

    /// Helper for constructing GPU payloads (mirrored for testing).
    pub fn from_gpu_bytes(
        data: Vec<u8>,
        device_id: u32,
        pitch: usize,
        metadata: FrameMetadata,
    ) -> Self {
        Self {
            buffer: FrameBuffer::Gpu {
                data: data.into(),
                device_id,
                pitch,
            },
            metadata,
        }
    }
}

/// Errors returned by asynchronous pipeline stages.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PipelineError {
    #[error("I/O failure: {0}")]
    Io(String),
    #[error("decode failure: {0}")]
    Decode(String),
    #[error("mask failure: {0}")]
    Mask(String),
    #[error("inpaint failure: {0}")]
    Inpaint(String),
    #[error("temporal failure: {0}")]
    Temporal(String),
    #[error("encode failure: {0}")]
    Encode(String),
    #[error("policy violation: {0}")]
    PolicyViolation(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("task join failure: {0}")]
    Join(String),
}

impl From<tokio::task::JoinError> for PipelineError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::Join(err.to_string())
    }
}

/// Result alias for frame processing stages.
pub type FrameResult = Result<FramePayload, PipelineError>;

/// Executes the pipeline end-to-end once configuration and ARC controllers are ready.
#[instrument(skip_all)]
pub async fn run(config: config::AppConfig) -> Result<(), PipelineError> {
    let telemetry = arc::telemetry::TelemetrySink::default();
    let coordinator = arc::coordinator::ArcCoordinator::new(
        telemetry.clone(),
        config.policy.clone(),
        config.arc_policy.clone(),
    );

    pipeline::execute_pipeline(config, coordinator, telemetry).await
}

impl Display for FramePayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FramePayload(clip={}, frame={}, {}x{}, gpu={})",
            self.metadata.clip_id,
            self.metadata.frame_index,
            self.metadata.width,
            self.metadata.height,
            self.buffer.is_gpu()
        )
    }
}
