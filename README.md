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
cargo run --bin swarm -- demo

# Run the basic_swarm example
cargo run --bin basic_swarm

# Show effective configuration
cargo run --bin swarm -- config

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

## Writing a Custom Plugin

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
