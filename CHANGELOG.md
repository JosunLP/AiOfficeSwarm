# Changelog

<!-- markdownlint-disable MD024 -->

All notable changes to this project will be documented in this file.

The format is inspired by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project follows [Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-03-19

Patch release focused on validation hardening, documentation cleanup, and
release workflow compatibility.

### Added

- Central `TaskSpec::validate()` timeout and structural validation in the core
  task model, with orchestrator enforcement before task submission.
- Strict role loading options that allow CLI and examples to treat warnings as
  blocking issues when configured.
- Host-side plugin manifest validation for semantic versions, duplicate actions,
  permission shapes, and action-provider consistency.

### Changed

- Tightened the `basic_swarm` example so strict role validation can fail fast on
  blocking role issues.
- Refreshed architecture and role-system documentation to reflect the current
  runtime integration and release version.
- Updated GitHub Actions workflows to supported macOS runner labels and newer
  action versions compatible with the current JavaScript runtime rollout.

### Fixed

- Resolved a Clippy `manual_filter` lint in the runtime provider-selection path.
- Aligned plugin host test fixtures with the stricter manifest validation rules.

## [0.1.0] - 2026-03-18

First public release of `AiOfficeSwarm`.

### Added

- Modular Rust workspace for AI agent orchestration with dedicated crates for
  core types, orchestration, policy, plugin hosting, runtime, config, telemetry
  and CLI usage.
- Hierarchical agent supervision, capability-aware task scheduling and a deny-
  by-default policy model.
- Native plugin SDK plus WASM plugin loading with manifest-declared permission metadata.
- `swarm` CLI with demo, config, task, plugin, metrics and self-update commands.
- Cross-platform install and uninstall scripts for Linux, macOS and Windows.
- GitHub Actions workflows for CI and release packaging across major platforms.

### Notes

- Release artifacts are published as GitHub Release downloads.
- Installation instructions and update commands are documented in `README.md`.
