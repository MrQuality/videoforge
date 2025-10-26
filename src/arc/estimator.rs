//! Lightweight resource estimator for ARC policy evaluation.

use crate::FrameMetadata;

/// Estimated resource usage for a single frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceEstimate {
    pub bandwidth_mb: f32,
    pub compute_cost_ms: u32,
}

/// Stateless estimator relying on metadata dimensions.
#[derive(Debug, Default, Clone)]
pub struct ResourceEstimator;

impl ResourceEstimator {
    /// Builds a simple estimate from the frame dimensions and channels.
    pub fn estimate(&self, metadata: &FrameMetadata) -> ResourceEstimate {
        let pixels = metadata.width as f32 * metadata.height as f32;
        let channels = metadata.channels as f32;
        let bandwidth_mb = pixels * channels / (1024.0 * 1024.0);
        let compute_cost_ms = (bandwidth_mb * 2.5) as u32 + 1;
        ResourceEstimate {
            bandwidth_mb,
            compute_cost_ms,
        }
    }
}
