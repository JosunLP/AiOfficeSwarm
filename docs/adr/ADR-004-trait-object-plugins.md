# ADR-004: Trait-Object Plugin System

**Status**: Accepted  
**Date**: 2026-03-16

## Context

The plugin system needs to support arbitrary third-party extensions without
requiring recompilation of the host framework.

## Decision

Plugins are represented as `Box<dyn Plugin>` trait objects. The `Plugin` trait
is the only contract between host and plugin. Plugins are stored as
`Arc<tokio::sync::Mutex<Box<dyn Plugin>>>` for async-safe access.

## Rationale

- Zero coupling: plugin crates only depend on `swarm-plugin` and `swarm-core`.
- Runtime polymorphism allows heterogeneous plugin collections.
- `Arc<tokio::sync::Mutex<...>>` ensures safe concurrent access in async code.

## Consequences

- Plugin dispatch has a small virtual-call overhead (acceptable for I/O-bound workloads).
- Dynamic library loading (`.so`/`.dll`) is deferred to a future milestone
  to avoid ABI instability.
- Plugins must be compiled with the same Rust toolchain as the host.

## Future

When dynamic loading is added, a stable ABI (e.g., via `abi_stable` crate or
WASM) will be introduced rather than raw `dlopen`.
