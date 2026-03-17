# Architecture Overview

## Product Vision

AiOfficeSwarm is a **platform-independent, enterprise-ready Rust framework** for
orchestrating AI agents. It treats AI agents as general-purpose coordinated
workers in a managed enterprise platform — not just software development
assistants. The framework can support any domain: business processes, data
analysis, content generation, customer support, finance operations, and more.

---

## Layered Architecture

```
┌──────────────────────────────────────────────────────────┐
│                    Interface Layer                         │
│  swarm-cli (CLI)  │  API surface (future HTTP/gRPC)       │
├──────────────────────────────────────────────────────────┤
│                   Plugin / Integration Layer               │
│  swarm-plugin SDK │ example-integration │ custom plugins  │
├──────────────────────────────────────────────────────────┤
│                    Policy Layer                            │
│  swarm-policy: RBAC engine, policy evaluation, admission  │
├──────────────────────────────────────────────────────────┤
│                  Orchestration / Control Plane             │
│  swarm-orchestrator: registry, scheduler, supervision     │
├──────────────────────────────────────────────────────────┤
│                    Execution / Runtime Layer               │
│  swarm-runtime: task runner, circuit breaker, retry       │
├──────────────────────────────────────────────────────────┤
│              Configuration & Observability                 │
│  swarm-config │ swarm-telemetry                           │
├──────────────────────────────────────────────────────────┤
│                      Core Domain Layer                     │
│  swarm-core: types, traits, errors, events                │
└──────────────────────────────────────────────────────────┘
```

### Layer Responsibilities

| Layer | Crate(s) | Key Concerns |
|-------|----------|--------------|
| Core domain | `swarm-core` | Agent/Task/Policy/RBAC types; error model; event types |
| Control plane | `swarm-orchestrator` | Agent registry; task queue; capability-based scheduling; supervision trees |
| Policy | `swarm-policy` | RBAC engine; policy evaluation; deny-by-default admission |
| Runtime | `swarm-runtime` | Async task execution; timeout; circuit breaker; retry with backoff |
| Plugin | `swarm-plugin` | Plugin SDK (manifest, lifecycle, host, registry) |
| Config | `swarm-config` | TOML config; env overrides; secrets abstraction |
| Telemetry | `swarm-telemetry` | `tracing` setup; atomic metrics; audit logger |
| Interface | `swarm-cli` | `swarm` CLI binary with agent/task/plugin/demo commands |

---

## Agent Hierarchy

Agents are organized in a three-tier hierarchy:

```
Executive (AgentKind::Executive)
  └── Manager (AgentKind::Manager)
        ├── Worker A (AgentKind::Worker)
        └── Worker B (AgentKind::Worker)
```

- **Executive agents** handle cross-domain arbitration and strategic direction.
- **Manager agents** coordinate within a domain and distribute workload.
- **Worker agents** execute concrete tasks.

Failure escalation follows the supervision tree upward. The
`SupervisionManager` tracks parent/child relationships and provides
`ancestry()` and `escalation_target()` queries.

---

## Task Lifecycle

```
Pending ──► Scheduled ──► Running ──► Completed
                                   └──► Failed
                                   └──► TimedOut
                                   └──► Cancelled
```

Tasks are immutable once in a terminal state. The orchestrator emits an
`EventEnvelope` on every state transition.

---

## Scheduling Algorithm

The `Scheduler` uses a **capability-match + least-loaded** strategy:

1. Filter agents by `AgentStatus::Ready`.
2. Filter by `CapabilitySet::satisfies_all(required_capabilities)`.
3. Sort remaining candidates by `tasks_completed` ascending.
4. Assign to the first (least-loaded) candidate.

---

## Policy Engine

The `PolicyEngine` evaluates a priority-ordered list of `Policy` trait objects:

- Returns `PolicyDecision::Allowed` on first `PolicyOutcome::Allow`.
- Returns `PolicyDecision::Denied` on first `PolicyOutcome::Deny`.
- Applies the configured **default decision** if all policies abstain.

Default posture: **deny-by-default** (secure by default).

Built-in policies: `AllowAllPolicy`, `DenyAllPolicy`, `ActionAllowlistPolicy`.

---

## Plugin System

Plugins implement the `Plugin` trait and declare themselves via `PluginManifest`.

**Plugin lifecycle:**
```
Discovered → Loading → Active → Unloading → Unloaded
                    └──────────► Failed
```

The `PluginHost` manages all loaded plugin instances and routes invocations.
Plugins are isolated behind `Arc<tokio::sync::Mutex<Box<dyn Plugin>>>` and cannot directly
access orchestrator state — they must go through the public API.

---

## WASM Plugins (precompiled `.wasm` files)

Plugins can also be distributed as precompiled WebAssembly binaries (`wasm`
feature, enabled by default).

### Distribution format

A WASM plugin is a pair of files:
```
plugins/my-plugin/
├── plugin.toml      # manifest (metadata + permissions)
└── my-plugin.wasm   # precompiled WebAssembly binary
```

### Manifest format (`plugin.toml`)

```toml
[plugin]
name             = "My WASM Plugin"
version          = "1.0.0"
author           = "Acme Corp"
description      = "A useful plugin compiled to WebAssembly"
min_host_version = "0.1.0"
wasm_file        = "my-plugin.wasm"
capabilities     = ["ActionProvider"]
required_permissions = ["read:config"]

[[plugin.wasm_permissions]]
kind  = "Network"
value = "api.example.com:443"

[[plugin.wasm_permissions]]
kind  = "EnvVar"
value = "MY_API_KEY"

[[plugin.actions]]
name        = "do_something"
description = "Performs a useful operation"
```

### WASM sandbox permissions (`WasmPermission`)

| Kind | Example value | Description |
|------|---------------|-------------|
| `Network` | `"api.example.com:443"` | Outbound network access |
| `EnvVar` | `"MY_API_KEY"` | Read an environment variable |
| `FileRead` | `"/etc/ssl/certs"` | Read from a filesystem path |
| `FileWrite` | `"/tmp/plugin-cache"` | Write to a filesystem path |
| `Custom` | `"my-special-permission"` | Arbitrary named capability |

### WASM ABI

The compiled `.wasm` module must export:

| Export | Signature | Semantics |
|--------|-----------|-----------|
| `memory` | memory | Linear memory (default export) |
| `swarm_alloc` | `(i32) → i32` | Allocate bytes; returns pointer |
| `swarm_dealloc` | `(i32, i32)` | Free bytes at pointer |
| `swarm_on_load` | `() → i32` | `0` = success |
| `swarm_on_unload` | `() → i32` | `0` = success |
| `swarm_health_check` | `() → i32` | `0` = healthy |
| `swarm_invoke` | `(i32, i32, i32, i32, i32, i32) → i32` | See below |

`swarm_invoke(action_ptr, action_len, params_ptr, params_len, result_ptr, result_cap)`:
- Returns `n ≥ 0`: success — `n` bytes of JSON at `result_ptr`.
- Returns `n < 0`: error — `(-n)` bytes of error message at `result_ptr`.

### Loading a WASM plugin

```rust
use std::path::Path;
use swarm_plugin::{PluginHost, wasm_loader::WasmPluginLoader};

let plugin = WasmPluginLoader::from_manifest_file(
    Path::new("plugins/my-plugin/plugin.toml"),
)?;
let host = PluginHost::new();
let id = host.load(Box::new(plugin)).await?;
```

See `plugins/wasm-example/` for a complete worked example of a WASM plugin
written in Rust (targeting `wasm32-unknown-unknown`).

---

## Fault Tolerance

### Circuit Breaker
`CircuitBreaker` protects agent calls from cascade failures:
- Opens after `failure_threshold` consecutive failures.
- Transitions to `HalfOpen` after `open_duration_secs`.
- Closes on a successful probe.

### Retry Executor
`RetryExecutor` applies `RetryPolicy` (fixed or exponential backoff with
optional jitter) to transient failures. Non-retryable errors (policy
violations, etc.) bypass retry immediately.

---

## RBAC Model

```
Subject (agent / user / service-account / plugin)
  ├── assigned Role(s)
  │     └── Permission(s) { verb: "create", resource: "task" }
  └── checked by RbacEngine.has_permission()
```

Built-in roles: `admin`, `task-executor`, `task-submitter`, `observer`.

---

## Security Architecture

| Concern | Mechanism |
|---------|-----------|
| Least privilege | Deny-by-default policy engine |
| Identity | Strongly-typed `AgentId`, `PluginId`, etc. |
| Authorization | RBAC engine with role/permission model |
| Secret management | `SecretsProvider` abstraction (env-based default) |
| Audit trail | `AuditLogger` with allowed/denied entries |
| Plugin isolation | Plugins behind `Arc<tokio::sync::Mutex<Box<dyn Plugin>>>` |
| No unsafe code | `#![forbid(unsafe_code)]` in all workspace crates |
| Input validation | Explicit `InvalidTaskSpec` error on malformed input |

---

## Event Bus

The orchestrator maintains a Tokio `broadcast` channel. Significant state
changes emit `EventEnvelope` values:

- Agent: Registered, StatusChanged, Deregistered
- Task: Submitted, Scheduled, Started, Completed, Failed, Cancelled, TimedOut
- Policy: Evaluated
- Plugin: Loaded, Unloaded, Event
- System: OrchestratorStarted, OrchestratorShuttingDown

Subscribe via `OrchestratorHandle::subscribe()`.

---

## Future Roadmap

| Milestone | Description |
|-----------|-------------|
| v0.2 | HTTP API surface (REST/gRPC) for remote orchestrator access |
| v0.2 | Persistent task store (SQLite/PostgreSQL adapter) |
| v0.3 | Multi-tenant isolation via `TenantId` namespace partitioning |
| v0.3 | Webhook/trigger plugins (schedule, HTTP, event-driven) |
| v0.4 | Distributed orchestrator (Raft-based consensus, multi-node) |
| v0.4 | OpenTelemetry metrics export (Prometheus, OTLP) |
| v0.5 | Dynamic shared-library plugin loading; WASM permission enforcement hooks |
| v0.5 | Agent hot-restart and graceful drain |
