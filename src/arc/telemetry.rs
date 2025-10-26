//! Telemetry collection primitives for ARC decisions.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::FramePayload;

#[derive(Debug, Default)]
struct TelemetryState {
    stage_counts: HashMap<&'static str, u64>,
    completed_frames: u64,
}

/// Snapshot of telemetry suitable for assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetrySnapshot {
    pub stage_counts: Vec<(String, u64)>,
    pub completed_frames: u64,
}

/// Shared sink capturing per-stage events.
#[derive(Clone, Default)]
pub struct TelemetrySink {
    state: Arc<Mutex<TelemetryState>>,
}

impl TelemetrySink {
    /// Records a stage observation.
    pub fn record_stage(&self, stage: &'static str, _frame: &FramePayload) {
        let mut state = self.state.lock().expect("telemetry mutex poisoned");
        *state.stage_counts.entry(stage).or_insert(0) += 1;
    }

    /// Records the completion of a frame at the end of the pipeline.
    pub fn record_completed(&self, _frame: &FramePayload) {
        let mut state = self.state.lock().expect("telemetry mutex poisoned");
        state.completed_frames += 1;
    }

    /// Exposes a snapshot for diagnostics and testing.
    pub fn snapshot(&self) -> TelemetrySnapshot {
        let state = self.state.lock().expect("telemetry mutex poisoned");
        TelemetrySnapshot {
            stage_counts: state
                .stage_counts
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            completed_frames: state.completed_frames,
        }
    }
}
