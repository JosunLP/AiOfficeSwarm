# Changelog

All notable changes to this project will be documented in this file.

The format is inspired by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project follows [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-03-18

First public release of `AiOfficeSwarm`.

### Added

- Modular Rust workspace for AI agent orchestration with dedicated crates for
  core types, orchestration, policy, plugin hosting, runtime, config, telemetry
  and CLI usage.
- Hierarchical agent supervision, capability-aware task scheduling and a deny-
  by-default policy model.
- Native plugin SDK plus WASM plugin loading with manifest-driven permissions.
- `swarm` CLI with demo, config, task, plugin, metrics and self-update commands.
- Cross-platform install and uninstall scripts for Linux, macOS and Windows.
- GitHub Actions workflows for CI and release packaging across major platforms.

### Notes

- Release artifacts are published as GitHub Release downloads.
- Installation instructions and update commands are documented in `README.md`.
