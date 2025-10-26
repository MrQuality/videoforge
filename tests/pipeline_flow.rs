use std::path::PathBuf;

use videoforge::{
    arc::{coordinator::ArcCoordinator, telemetry::TelemetrySink},
    config::{AppConfig, CliArgs},
    pipeline,
};

#[path = "common/mod.rs"]
mod common;

fn test_cli_args(input: PathBuf) -> CliArgs {
    CliArgs {
        input,
        policy: PathBuf::from("policy.toml"),
        models: PathBuf::from("models.toml"),
        output: None,
        clip_id: "test_clip".to_string(),
        clip_seconds: Some(2),
        width: None,
        height: None,
        dry_run_frames: Some(1),
        channel_capacity: Some(4),
        confirm_rights: true,
        video_codec: Some("png".to_string()),
        hw_accel: None,
    }
}

#[tokio::test]
async fn pipeline_completes_synthetic_frames() {
    let (_clip_dir, clip_path) = common::write_sample_png().expect("clip fixture");
    let cli = test_cli_args(clip_path);
    let config = AppConfig::load(cli.clone()).await.expect("config load");
    let telemetry = TelemetrySink::default();
    let coordinator = ArcCoordinator::new(
        telemetry.clone(),
        config.policy.clone(),
        config.arc_policy.clone(),
    );

    pipeline::execute_pipeline(config.clone(), coordinator, telemetry.clone())
        .await
        .expect("pipeline execution");

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.completed_frames, config.runtime.dry_run_frames);
    let mut stage_counts = snapshot.stage_counts;
    stage_counts.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, count) in stage_counts {
        assert!(count >= config.runtime.dry_run_frames);
    }
}
