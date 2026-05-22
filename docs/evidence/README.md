# Auction Module — Evidence Screenshots

## Required

| File | Capture |
|------|---------|
| `llvm-cov-summary.png` | CI `cargo llvm-cov --fail-under-lines 90` summary |
| `sonar-quality-gate.png` | `advprog-2026-A17-project_bidmart-auction-service-rust` |
| `ci-workflow-success.png` | `.github/workflows/ci.yml` green run |

## Module-specific (profiling)

| File | Capture |
|------|---------|
| `profiling-naive-flamegraph.png` | `docs/Profiling-profile_bidding_naive/2. Flame Graph.png` |
| `profiling-optimized-flamegraph.png` | `docs/Profiling-profile_bidding_optimized/2. Flame Graph.png` |
| `profiling-terminal-compare.png` | Terminal timings from [7. Profiling Report.md](../7.%20Profiling%20Report.md) (97.6% improvement) |
| `grafana-apdex.png` | Grafana auction dashboard APDEX panel |
| `load-harness.png` | `tests/load_performance_harness_tests.rs` passing in CI log |

## Profiling reproduction

```bash
cargo build --release --bin profile_bidding --bin profile_bidding_naive
samply record ./target/release/profile_bidding_naive
samply record ./target/release/profile_bidding
```

Sonar: https://sonarcloud.io/project/overview?id=advprog-2026-A17-project_bidmart-auction-service-rust

## Panduan capture manual

Langkah lengkap: [SCREENSHOT_CAPTURE_GUIDE.md](../../../SCREENSHOT_CAPTURE_GUIDE.md) (workspace root).
