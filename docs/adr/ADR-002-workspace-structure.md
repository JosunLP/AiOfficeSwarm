# ADR-002: Workspace-Based Multi-Crate Structure

**Status**: Accepted  
**Date**: 2026-03-16

## Context

The framework has multiple distinct concerns (core domain, orchestration,
policy, plugins, runtime, config, telemetry, CLI). Keeping these in a single
crate would create tight coupling and make it hard to adopt individual
subsystems independently.

## Decision

Organize the project as a **Cargo workspace** with one crate per layer:

- `swarm-core` — zero-dependency domain layer
- `swarm-orchestrator` — control plane
- `swarm-policy` — policy engine
- `swarm-plugin` — plugin SDK
- `swarm-config` — configuration
- `swarm-telemetry` — observability
- `swarm-runtime` — execution runtime
- `swarm-cli` — CLI binary

## Consequences

- Clear dependency ordering (core has no internal deps).
- Incremental compilation: changing one crate does not recompile others.
- Downstream users can depend on only the crates they need.
- Slightly more boilerplate (one Cargo.toml per crate).
