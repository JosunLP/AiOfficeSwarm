# ADR-006: WASM-Based Plugin Distribution

**Status**: Accepted  
**Date**: 2026-03-16

## Context

Plugins need a safe, language-agnostic distribution mechanism. Native
Rust plugins must be compiled with the exact same toolchain as the host,
creating tight coupling. A sandboxed execution model improves security and
portability.

## Decision

Support **precompiled WebAssembly (`.wasm`) files** as a first-class plugin
format alongside native Rust plugins.

- The wasmtime runtime (`wasmtime` crate, enabled via the `wasm` feature) is
  used to compile and execute WASM modules.
- A TOML manifest file (`plugin.toml`) accompanies each `.wasm` binary and
  declares identity, capabilities, actions, and permissions.
- The WASM ABI is a stable, minimal set of exported functions that the host
  calls to drive the plugin lifecycle.

## Rationale

- **Language agnostic**: plugin authors can use Rust, Go, C, Python (via
  pyodide), or any language that compiles to WASM.
- **Security**: WASM modules run in a sandboxed address space; they cannot
  access host memory outside their linear memory region.
- **Explicit permissions**: `WasmPermission` in the manifest declares all
  OS-level capabilities (network, filesystem, env vars). The host can enforce
  these before instantiation.
- **Forward compatibility**: the ABI is versioned via `min_host_version` in
  the manifest.

## WASM ABI stability

The six exported functions (`swarm_alloc`, `swarm_dealloc`, `swarm_on_load`,
`swarm_on_unload`, `swarm_health_check`, `swarm_invoke`) form a stable,
versioned contract. Breaking changes require a new major version.

## Feature flag

The `wasm` feature is enabled by default in `swarm-plugin`. Users who want a
minimal binary without WASM support can disable it with
`default-features = false`.

## Consequences

- `wasmtime` (~10 MB compiled) is added as a default dependency of
  `swarm-plugin`.
- WASM plugin authors must target `wasm32-unknown-unknown` or `wasm32-wasi`.
- The bump-allocator pattern shown in the example is sufficient for most
  plugins; production plugins should use a proper allocator (e.g., `wee_alloc`
  or the standard global allocator via `wasm32-wasi`).
- Dynamic sandboxing (network filtering, filesystem isolation) is the host
  operator's responsibility in the initial version; enforcement hooks are
  reserved for a future milestone.
