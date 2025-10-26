# Videoforge Architecture

Videoforge is an asynchronous Rust application that validates and transforms
video frames through a series of Tokio-powered pipeline stages while enforcing
Adaptive Resource Controller (ARC) policy constraints.

## Pipeline Overview

```
┌────────┐    ┌───────┐    ┌────────┐    ┌──────────┐    ┌────────┐
│ ingest ├──▶│ decode├──▶│  mask  ├──▶│  inpaint  ├──▶│ temporal│
└────────┘    └───────┘    └────────┘    └──────────┘    └────────┘
                                                           │
                                                           ▼
                                                        ┌───────┐
                                                        │encode│
                                                        └───────┘
```

Each stage communicates through bounded `tokio::sync::mpsc` channels carrying
`Result<FramePayload, PipelineError>` messages. Stages forward errors without
mutating them, ensuring that failure propagation is predictable.

* **Decode** – Performs rights validation and migrates CPU payloads into a
  synthetic GPU representation.
* **Mask** – Applies policy validation prior to matting inference.
* **Inpaint** – Fills masked regions and normalises frames back to CPU memory.
* **Temporal** – Enforces monotonic timestamps and collects telemetry.
* **Encode** – Optionally persists frames to disk and hands results to the
  caller for aggregation.

> **Inference runtime**
>
> The `inference` Cargo feature intentionally ships without a backend runtime
> while we evaluate a patched ONNX Runtime release that is free from the
> `time` crate advisory (RUSTSEC-2020-0071).  Integrations depending on ONNX
> should add a workspace-local crate that re-enables the runtime once an
> audited build is available.

## ARC Coordinator

The ARC coordinator couples a telemetry sink, a lightweight resource estimator,
and policy enforcement logic. Before a frame enters the pipeline, the
coordinator validates metadata against `policy.toml` bounds and acquires a
permit from a concurrency semaphore sized by `[arc]` configuration values.

Resource estimates are compared against GPU bandwidth and compute thresholds.
Denials are surfaced as `PipelineError::PolicyViolation` errors.

## Configuration Flow

Configuration is built by merging:

1. `policy.toml` – runtime limits and defaults (resolution, clip length,
   channel capacity, frame rate).
2. `models.toml` – approved model registry with checksum validation.
3. Environment variables – overrides for clip timing, resolution, channel
   capacity, and rights confirmation.
4. CLI flags – highest-precedence overrides parsed via `clap`.

The `AppConfig` structure contains the merged settings plus derived values such
as frame period and bounded frame counts. Failure to confirm rights or exceeding
policy limits aborts startup.

## Testing and Benchmarking

Integration and unit tests under `tests/` cover:

* Pipeline messaging guarantees (error propagation, bounded channels).
* ARC policy decisions using synthetic estimates.
* Configuration parsing and checksum validation.

Criterion benchmarks located in `benches/` perform tile-size exploration for the
inpaint stage by simulating different frame dimensions.

## Continuous Integration

GitHub Actions runs the following checks on every push:

* `cargo fmt -- --check`
* `cargo clippy --all-targets --all-features -D warnings`
* `cargo test --all-features`
* `cargo audit`

These checks preserve code quality, enforce formatting, and monitor dependency
security advisories.
