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
- **Policy engine** — deny-by-default RBAC and allow/deny policy primitives for embedding applications
- **Plugin SDK** — first-class plugin system with lifecycle management
- **Role-aware operations** — first-class role loading and validation from the `roles/` directory
- **Execution-time context assembly** — role, personality, memory, learning, and provider context can be attached to task execution without changing the `Agent` trait
- **Provider-aware configuration** — routing defaults, allow/block controls, and compatibility posture
- **Memory and learning governance** — configurable retention, redaction, approval, and scope defaults
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

# Inspect configured learning governance
cargo run -p swarm-cli --bin swarm -- learning inspect

# Validate role definitions
cargo run -p swarm-cli --bin swarm -- role validate

# Run all tests
cargo test --workspace
```

---

## Installation

The latest release tag is `v0.1.1`.
The examples below show both **latest** and a **pinned version**. Replace
`vX.Y.Z` with a published release tag when installing or updating a specific
version.

### Linux / macOS

Published installer binaries currently cover Linux x86_64 as well as macOS on
Intel and Apple Silicon systems.

#### Linux / macOS quick start

```bash
curl -fsSL https://raw.githubusercontent.com/JosunLP/AiOfficeSwarm/main/scripts/install.sh -o install-swarm.sh
sh install-swarm.sh
rm install-swarm.sh
```

Installs `swarm` to `~/.local/bin` by default.
Use `SWARM_INSTALL_DIR=/my/path` to override the destination directory.

#### Linux / macOS pinned install

```bash
curl -fsSL https://raw.githubusercontent.com/JosunLP/AiOfficeSwarm/vX.Y.Z/scripts/install.sh -o install-swarm.sh
sh install-swarm.sh vX.Y.Z
rm install-swarm.sh
```

### Windows (PowerShell)

Published installer binaries currently cover Windows x86_64.

#### Windows quick start

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/JosunLP/AiOfficeSwarm/main/scripts/install.ps1 -OutFile install-swarm.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\install-swarm.ps1
Remove-Item .\install-swarm.ps1
```

Installs `swarm.exe` to `%LOCALAPPDATA%\AiOfficeSwarm\bin` by default.

#### Windows pinned install

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/JosunLP/AiOfficeSwarm/vX.Y.Z/scripts/install.ps1 -OutFile install-swarm.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\install-swarm.ps1 -Version vX.Y.Z
Remove-Item .\install-swarm.ps1
```

### Verify the installation

```bash
swarm --version
swarm demo
```

### Update

After installation, the CLI can update itself:

```bash
swarm update
swarm update --check
swarm update --version vX.Y.Z
```

### Uninstall

- Linux / macOS:

    ```bash
    curl -fsSL https://raw.githubusercontent.com/JosunLP/AiOfficeSwarm/main/scripts/uninstall.sh -o uninstall-swarm.sh
    sh uninstall-swarm.sh
    rm uninstall-swarm.sh
    ```

- Windows:

    ```powershell
    Invoke-WebRequest https://raw.githubusercontent.com/JosunLP/AiOfficeSwarm/main/scripts/uninstall.ps1 -OutFile uninstall-swarm.ps1
    powershell -NoProfile -ExecutionPolicy Bypass -File .\uninstall-swarm.ps1
    Remove-Item .\uninstall-swarm.ps1
    ```

### Release notes

- Changelog: [CHANGELOG.md](CHANGELOG.md)
- Latest release tag: `v0.1.1`

---

## Workspace Layout

```bash
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
│   ├── swarm-provider/          # Provider registry, routing, compatibility data
│   ├── swarm-personality/       # Personality profiles and boundaries
│   ├── swarm-memory/            # Memory scopes, retention, and redaction
│   ├── swarm-learning/          # Learning outputs, stores, and governance
│   ├── swarm-role/              # Role loader, validator, and policy bindings
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

| Layer         | Crate                                                   | Responsibility                           |
| ------------- | ------------------------------------------------------- | ---------------------------------------- |
| Core domain   | `swarm-core`                                            | Types, traits, error model               |
| Control plane | `swarm-orchestrator`                                    | Registry, scheduling, supervision        |
| Policy        | `swarm-policy`                                          | RBAC, admission, policy evaluation       |
| Runtime       | `swarm-runtime`                                         | Execution, retry, circuit breaker        |
| Plugin        | `swarm-plugin`                                          | Plugin SDK and host                      |
| Config        | `swarm-config`                                          | Configuration, secrets                   |
| Telemetry     | `swarm-telemetry`                                       | Tracing, metrics, audit                  |
| Cognition     | `swarm-memory` / `swarm-learning` / `swarm-personality` | Stateful agent context and governance    |
| Provider      | `swarm-provider`                                        | Provider registry and routing            |
| Role          | `swarm-role`                                            | Role loading, validation, policy binding |
| Interface     | `swarm-cli`                                             | CLI management interface                 |

---

## Operator commands

The CLI now exposes lightweight inspection commands for key enterprise concepts:

- `swarm role list` — enumerate loadable role definitions.
- `swarm role validate` — validate role files and print diagnostics.
- `swarm task submit --name triage --input '{"ticket":42}'` — persist a local task snapshot for operator workflows and integration testing.
- `swarm task list` / `status <id>` / `cancel <id>` / `retry <id>` / `retry-batch --status failed --limit 10` — inspect or update the local persistent task queue snapshot.
- `swarm task process --workers 2 --limit 10` — rehydrate pending persisted tasks into an in-process orchestrator and execute them with built-in local workers.
- `swarm task export --format jsonl --status failed --output failed-tasks.jsonl` — export persisted task snapshots for audit, backup, and migration workflows.
- `swarm learning inspect` — show the effective learning governance baseline.
- `swarm learning list --scope global --category plan_template` — inspect recorded learning outputs, including reusable learned templates.
- `swarm learning pending --scope global --category plan_template` — inspect the persistent learning approval queue with optional category filtering.
- `swarm learning approve <id>` / `reject <id>` / `rollback <id>` — manage individual learning lifecycle decisions.
- `swarm learning approve-batch --scope global --category plan_template` — apply lifecycle updates to filtered groups of learning outputs.
- `swarm config --format json` — inspect the full effective configuration, including provider, memory, learning, plugin, and role settings.

The local task commands persist to `orchestrator.task_store_path` (default
`.swarm/task-store.json`) so multiple CLI invocations can share the same task
snapshot history without requiring a long-running swarm process.

---

## Runtime integration baseline

The framework now includes a concrete execution-time integration seam through
`swarm-runtime::TaskExecutionContext`.

When attached to a `TaskRunner`, the runtime can:

- enforce an execution-time policy check,
- resolve role-derived personality overlays,
- retrieve memory context into task metadata,
- persist episodic execution memories,
- capture learning outputs for approval queues,
- run pluggable learning strategies to derive reusable execution templates,
- annotate tasks with provider-routing selections.

This keeps the public `Agent` trait stable while making the cognition and
provider subsystems materially useful at runtime.

Current limits remain explicit:

- provider routing can now enforce health, compliance, locality, and fallback preferences but still annotates execution context rather than invoking providers directly,
- plugin invocation can now enforce manifest-declared actions and host-granted framework permissions, while richer embedding-level policy enforcement still remains external.

The injected metadata includes keys such as:

- `swarm.role.name`
- `swarm.personality.profile_json`
- `swarm.memory.context_json`
- `swarm.provider.name`

See `examples/basic_swarm/` for an end-to-end reference flow.

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
min_host_version = "0.1.1"
wasm_file        = "my-plugin.wasm"
capabilities     = ["ActionProvider"]

# Framework RBAC permissions
required_permissions = []

# OS-level sandbox permissions
# (can be enforced by PluginHost allowlists when the embedding app configures
# them; otherwise they remain declarative metadata)
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
    let input = core::slice::from_raw_parts(params_ptr, params_len);
    let out   = core::slice::from_raw_parts_mut(result_ptr, result_cap);
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
    async fn invoke(&mut self, action: &str, params: serde_json::Value) -> SwarmResult<serde_json::Value> {
        Ok(params)
    }
    async fn health_check(&self) -> SwarmResult<()> { Ok(()) }
}
```

See `plugins/example-integration/` for a complete worked example.

---

## License

Licensed under [MIT license](LICENSE-MIT).
