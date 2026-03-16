# AiOfficeSwarm

> **Enterprise-ready Rust framework for orchestrating AI agents**

AiOfficeSwarm is a modular, secure, and highly extensible Rust framework that
coordinates multiple AI agents through hierarchical management, workload
balancing, policy enforcement, and fault handling — all inspired by
cloud-native orchestration principles (Kubernetes, control planes, plugin
platforms).

---

## Features

- **Hierarchical agent management** — Executive → Manager → Worker supervision trees
- **Priority-based task scheduling** — capability-matching, least-loaded dispatch
- **Policy engine** — deny-by-default RBAC, allow/deny policies, admission control
- **Plugin SDK** — first-class plugin system with lifecycle management
- **Fault tolerance** — circuit breakers, configurable retry with exponential backoff
- **Observability** — structured tracing, metrics counters, audit logger
- **Configuration** — TOML files + environment variable overrides
- **CLI** — `swarm` binary for management and demos

---

## Quick Start

```bash
# Build the workspace
cargo build --workspace

# Run the built-in demo
cargo run -p swarm-cli --bin swarm -- demo

# Run the basic_swarm example
cargo run -p basic_swarm --bin basic_swarm

# Show effective configuration
cargo run -p swarm-cli --bin swarm -- config

# Run all tests
cargo test --workspace
```

---

## Workspace Layout

```
AiOfficeSwarm/
├── Cargo.toml                   # Workspace root
├── crates/
│   ├── swarm-core/              # Domain types, traits, error model, events
│   ├── swarm-orchestrator/      # Agent registry, task queue, scheduler, supervision
│   ├── swarm-policy/            # Policy engine, RBAC enforcement
│   ├── swarm-plugin/            # Plugin SDK, lifecycle, host
│   ├── swarm-config/            # TOML config, env overrides, secrets abstraction
│   ├── swarm-telemetry/         # Tracing setup, metrics, audit logger
│   ├── swarm-runtime/           # Task runner, circuit breaker, retry executor
│   └── swarm-cli/               # `swarm` CLI binary
├── plugins/
│   └── example-integration/     # Example plugin: NotificationPlugin
├── examples/
│   └── basic_swarm/             # Usage example: agents, tasks, plugins
└── docs/
    ├── architecture.md           # Architecture overview
    └── adr/                      # Architecture Decision Records
```

---

## Architecture Overview

See [docs/architecture.md](docs/architecture.md) for a full description.

The framework is layered:

| Layer | Crate | Responsibility |
|-------|-------|----------------|
| Core domain | `swarm-core` | Types, traits, error model |
| Control plane | `swarm-orchestrator` | Registry, scheduling, supervision |
| Policy | `swarm-policy` | RBAC, admission, policy evaluation |
| Runtime | `swarm-runtime` | Execution, retry, circuit breaker |
| Plugin | `swarm-plugin` | Plugin SDK and host |
| Config | `swarm-config` | Configuration, secrets |
| Telemetry | `swarm-telemetry` | Tracing, metrics, audit |
| Interface | `swarm-cli` | CLI management interface |

---

## Writing a Custom Agent

```rust
use async_trait::async_trait;
use swarm_core::{
    agent::{Agent, AgentDescriptor, AgentKind},
    capability::{Capability, CapabilitySet},
    error::SwarmResult,
    task::Task,
};

struct MyAgent {
    descriptor: AgentDescriptor,
}

impl MyAgent {
    fn new() -> Self {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::new("my-capability"));
        Self {
            descriptor: AgentDescriptor::new("MyAgent", AgentKind::Worker, caps),
        }
    }
}

#[async_trait]
impl Agent for MyAgent {
    fn descriptor(&self) -> &AgentDescriptor { &self.descriptor }

    async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
        // Your logic here
        Ok(task.spec.input.clone())
    }

    async fn health_check(&self) -> SwarmResult<()> { Ok(()) }
}
```

---

## Writing a WASM Plugin

WASM plugins are precompiled `.wasm` binaries paired with a TOML manifest.
They can be written in **any language that compiles to WebAssembly**.

### 1. Create the manifest (`plugin.toml`)

```toml
[plugin]
name             = "My WASM Plugin"
version          = "1.0.0"
author           = "Acme Corp"
description      = "A plugin compiled to WebAssembly"
min_host_version = "0.1.0"
wasm_file        = "my-plugin.wasm"
capabilities     = ["ActionProvider"]

# Framework RBAC permissions
required_permissions = []

# OS-level sandbox permissions
[[plugin.wasm_permissions]]
kind  = "EnvVar"
value = "MY_API_KEY"

[[plugin.wasm_permissions]]
kind  = "Network"
value = "api.example.com:443"

[[plugin.actions]]
name        = "my_action"
description = "Does something useful"
```

### 2. Implement the WASM ABI (Rust example)

```rust
// Compile with: cargo build --target wasm32-unknown-unknown --release

#[no_mangle]
pub extern "C" fn swarm_on_load()      -> i32 { 0 }
#[no_mangle]
pub extern "C" fn swarm_on_unload()    -> i32 { 0 }
#[no_mangle]
pub extern "C" fn swarm_health_check() -> i32 { 0 }

#[no_mangle]
pub unsafe extern "C" fn swarm_alloc(size: usize) -> *mut u8 {
    /* allocate with your preferred allocator */
}
#[no_mangle]
pub unsafe extern "C" fn swarm_dealloc(ptr: *mut u8, len: usize) { /* free */ }

#[no_mangle]
pub unsafe extern "C" fn swarm_invoke(
    _action_ptr: *const u8, _action_len: usize,
    params_ptr: *const u8, params_len: usize,
    result_ptr: *mut u8, result_cap: usize,
) -> i32 {
    // return n >= 0 on success (n bytes of JSON at result_ptr)
    // return n < 0 on error  ((-n) bytes of error message at result_ptr)
    let input = std::slice::from_raw_parts(params_ptr, params_len);
    let out   = std::slice::from_raw_parts_mut(result_ptr, result_cap);
    let n = input.len().min(result_cap);
    out[..n].copy_from_slice(&input[..n]);
    n as i32
}
```

### 3. Load it from the host

```rust
use std::path::Path;
use swarm_plugin::{PluginHost, wasm_loader::WasmPluginLoader};

let plugin = WasmPluginLoader::from_manifest_file(
    Path::new("plugins/my-plugin/plugin.toml"),
)?;
let host = PluginHost::new();
let id = host.load(Box::new(plugin)).await?;
let result = host.invoke(&id, "my_action", serde_json::json!({})).await?;
```

See `plugins/wasm-example/` for the complete example.

```rust
use async_trait::async_trait;
use swarm_core::error::SwarmResult;
use swarm_plugin::{manifest::PluginManifest, Plugin};

struct MyPlugin { manifest: PluginManifest }

#[async_trait]
impl Plugin for MyPlugin {
    fn manifest(&self) -> &PluginManifest { &self.manifest }
    async fn on_load(&mut self) -> SwarmResult<()> { Ok(()) }
    async fn on_unload(&mut self) -> SwarmResult<()> { Ok(()) }
    async fn invoke(&self, action: &str, params: serde_json::Value) -> SwarmResult<serde_json::Value> {
        Ok(params)
    }
    async fn health_check(&self) -> SwarmResult<()> { Ok(()) }
}
```

See `plugins/example-integration/` for a complete worked example.

---

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
