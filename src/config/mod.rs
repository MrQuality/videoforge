//! Configuration loading and validation utilities.

use std::path::PathBuf;

use clap::Parser;
use serde::Deserialize;
use tokio::fs;
use tracing::instrument;

use crate::PipelineError;

/// Command-line arguments used to bootstrap the runtime.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "Videoforge video processing pipeline")]
pub struct CliArgs {
    /// Location of the policy document.
    #[arg(long, value_name = "PATH", default_value = "policy.toml")]
    pub policy: PathBuf,
    /// Location of the model registry document.
    #[arg(long, value_name = "PATH", default_value = "models.toml")]
    pub models: PathBuf,
    /// Output file for encoded previews.
    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,
    /// Identifier for the clip used in telemetry.
    #[arg(long, value_name = "ID", default_value = "synthetic_clip")]
    pub clip_id: String,
    /// Clip duration override.
    #[arg(long, value_name = "SECONDS", env = "VIDEOFORGE_CLIP_SECONDS")]
    pub clip_seconds: Option<u32>,
    /// Output width override.
    #[arg(long, value_name = "WIDTH", env = "VIDEOFORGE_WIDTH")]
    pub width: Option<u32>,
    /// Output height override.
    #[arg(long, value_name = "HEIGHT", env = "VIDEOFORGE_HEIGHT")]
    pub height: Option<u32>,
    /// Number of synthetic frames to push through the pipeline.
    #[arg(long, value_name = "FRAMES", env = "VIDEOFORGE_DRY_RUN_FRAMES")]
    pub dry_run_frames: Option<u64>,
    /// Bounded channel capacity.
    #[arg(long, value_name = "CAPACITY", env = "VIDEOFORGE_CHANNEL_CAPACITY")]
    pub channel_capacity: Option<u32>,
    /// Confirmation that content rights have been validated.
    #[arg(long, env = "VIDEOFORGE_CONFIRM_RIGHTS")]
    pub confirm_rights: bool,
}

/// Limits enforced at runtime.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PolicyLimits {
    pub max_clip_seconds: u32,
    pub max_frames_per_clip: u64,
    pub max_width: u32,
    pub max_height: u32,
    pub max_frame_rate: u32,
}

/// Default runtime overrides provided by policy authors.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RuntimeDefaults {
    pub clip_seconds: u32,
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub frame_rate: u32,
    pub dry_run_frames: u64,
    pub channel_capacity: u32,
}

/// ARC policy thresholds.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ArcPolicy {
    pub max_concurrent_jobs: u32,
    pub max_gpu_bandwidth_mb: f32,
    pub max_frame_compute_ms: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct PolicyDocument {
    pub limits: PolicyLimits,
    pub defaults: RuntimeDefaults,
    pub arc: ArcPolicy,
}

/// Registered model metadata.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ModelRegistry {
    pub models: Vec<ModelRecord>,
}

/// A single model entry.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ModelRecord {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    #[serde(rename = "checksum_sha256")]
    pub checksum: String,
}

impl ModelRegistry {
    pub fn validate(&self) -> Result<(), PipelineError> {
        for model in &self.models {
            if model.checksum.len() != 64
                || !model
                    .checksum
                    .chars()
                    .all(|c| matches!(c, '0'..='9' | 'a'..='f' | 'A'..='F'))
            {
                return Err(PipelineError::Config(format!(
                    "invalid checksum for model {}:{}",
                    model.name, model.version
                )));
            }
            if model.path.as_os_str().is_empty() {
                return Err(PipelineError::Config(format!(
                    "model {}:{} missing artifact path",
                    model.name, model.version
                )));
            }
        }
        Ok(())
    }
}

/// Derived runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub clip_id: String,
    pub clip_seconds: u32,
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub frame_rate: u32,
    pub dry_run_frames: u64,
    pub channel_capacity: u32,
    pub confirm_rights: bool,
}

impl RuntimeConfig {
    pub fn frame_period_ms(&self) -> u64 {
        if self.frame_rate == 0 {
            0
        } else {
            1000 / self.frame_rate as u64
        }
    }
}

/// Fully merged configuration set.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub cli: CliArgs,
    pub policy: PolicyLimits,
    pub arc_policy: ArcPolicy,
    pub models: ModelRegistry,
    pub runtime: RuntimeConfig,
    pub output_path: Option<PathBuf>,
}

impl AppConfig {
    #[instrument(skip_all)]
    pub async fn load(cli: CliArgs) -> Result<Self, PipelineError> {
        let policy_raw = fs::read_to_string(&cli.policy)
            .await
            .map_err(|err| PipelineError::Config(format!("failed to read policy: {err}")))?;
        let policy_doc: PolicyDocument = toml::from_str(&policy_raw)
            .map_err(|err| PipelineError::Config(format!("invalid policy document: {err}")))?;

        let models_raw = fs::read_to_string(&cli.models)
            .await
            .map_err(|err| PipelineError::Config(format!("failed to read models: {err}")))?;
        let models: ModelRegistry = toml::from_str(&models_raw)
            .map_err(|err| PipelineError::Config(format!("invalid models document: {err}")))?;
        models.validate()?;

        let clip_seconds = cli.clip_seconds.unwrap_or(policy_doc.defaults.clip_seconds);
        if clip_seconds > policy_doc.limits.max_clip_seconds {
            return Err(PipelineError::PolicyViolation(format!(
                "clip duration {}s exceeds policy cap {}s",
                clip_seconds, policy_doc.limits.max_clip_seconds
            )));
        }
        let width = cli.width.unwrap_or(policy_doc.defaults.width);
        if width > policy_doc.limits.max_width {
            return Err(PipelineError::PolicyViolation(format!(
                "width {} exceeds policy cap {}",
                width, policy_doc.limits.max_width
            )));
        }
        let height = cli.height.unwrap_or(policy_doc.defaults.height);
        if height > policy_doc.limits.max_height {
            return Err(PipelineError::PolicyViolation(format!(
                "height {} exceeds policy cap {}",
                height, policy_doc.limits.max_height
            )));
        }
        let dry_run_frames = cli
            .dry_run_frames
            .unwrap_or(policy_doc.defaults.dry_run_frames)
            .min(policy_doc.limits.max_frames_per_clip);
        if dry_run_frames == 0 {
            return Err(PipelineError::Config(
                "dry-run frames must be positive".to_string(),
            ));
        }
        let channel_capacity = cli
            .channel_capacity
            .unwrap_or(policy_doc.defaults.channel_capacity)
            .max(1);

        if !cli.confirm_rights {
            return Err(PipelineError::Config(
                "confirm_rights must be supplied via --confirm-rights or env".to_string(),
            ));
        }

        let runtime = RuntimeConfig {
            clip_id: cli.clip_id.clone(),
            clip_seconds,
            width,
            height,
            channels: policy_doc.defaults.channels,
            frame_rate: policy_doc
                .defaults
                .frame_rate
                .min(policy_doc.limits.max_frame_rate)
                .max(1),
            dry_run_frames,
            channel_capacity,
            confirm_rights: cli.confirm_rights,
        };

        Ok(Self {
            output_path: cli.output.clone(),
            cli,
            policy: policy_doc.limits,
            arc_policy: policy_doc.arc,
            models,
            runtime,
        })
    }
}
