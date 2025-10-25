use std::path::PathBuf;

use videoforge::config::{AppConfig, CliArgs, ModelRecord, ModelRegistry};

fn base_cli(confirm_rights: bool) -> CliArgs {
    CliArgs {
        policy: PathBuf::from("policy.toml"),
        models: PathBuf::from("models.toml"),
        output: None,
        clip_id: "config_test".into(),
        clip_seconds: None,
        width: None,
        height: None,
        dry_run_frames: None,
        channel_capacity: None,
        confirm_rights,
    }
}

#[tokio::test]
async fn config_loads_defaults_successfully() {
    let cli = base_cli(true);
    let config = AppConfig::load(cli).await.expect("load defaults");
    assert_eq!(config.runtime.width, 1280);
    assert_eq!(config.runtime.height, 720);
    assert_eq!(config.runtime.frame_rate, 24);
}

#[tokio::test]
async fn config_rejects_missing_rights_confirmation() {
    let cli = base_cli(false);
    let err = AppConfig::load(cli)
        .await
        .expect_err("missing rights should fail");
    assert!(format!("{err}").contains("confirm_rights"));
}

#[test]
fn model_registry_checksum_validation() {
    let registry = ModelRegistry {
        models: vec![ModelRecord {
            name: "broken".into(),
            version: "0.0.1".into(),
            path: PathBuf::from("/tmp/model.onnx"),
            checksum: "abc".into(),
        }],
    };
    let err = registry.validate().expect_err("checksum should fail");
    assert!(format!("{err}").contains("invalid checksum"));
}
