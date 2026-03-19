# Role Subsystem Architecture

> **Crate:** `swarm-role`
> **Status:** Active
> **Version:** 0.1.1
> **Last updated:** 2025-07-15

## Overview

The role subsystem transforms the organizational role definitions in `roles/` from informal Markdown prompt files into typed, governable, versionable, and executable runtime constructs. Roles are not cosmetic labels — they influence orchestration, tool access, memory boundaries, learning policies, personality overlays, supervision hierarchies, and compliance controls.

## Architecture Layers

| Layer               | Type                   | Purpose                                            |
| ------------------- | ---------------------- | -------------------------------------------------- |
| **Source**          | `RawRoleSource`        | Unvalidated parse result from a role Markdown file |
| **Specification**   | `RoleSpec`             | Normalized, validated, typed role definition       |
| **Runtime profile** | `EffectiveRoleProfile` | Policy-resolved, tenant-overridden runtime state   |

```text
roles/00_GOVERNANCE/CEO_Agent.md
        │
        ▼
  ┌─────────────┐
  │  RoleParser  │ ── Markdown → RawRoleSource
  └──────┬──────┘
         ▼
  ┌──────────────────┐
  │  RoleNormalizer   │ ── RawRoleSource → RoleSpec
  └──────┬───────────┘
         ▼
  ┌──────────────────┐
  │  RoleValidator    │ ── structural & semantic validation
  └──────┬───────────┘
         ▼
  ┌──────────────────┐
  │  RoleHierarchy    │ ── apply supervisor/subordinate links from organigram
  └──────┬───────────┘
         ▼
  ┌──────────────────┐
  │  RoleRegistry     │ ── DashMap-backed concurrent registry
  └──────┬───────────┘
         ▼
  ┌──────────────────┐
  │  RoleResolver     │ ── RoleSpec + TenantRoleOverride → EffectiveRoleProfile
  └──────────────────┘
```

## Module Reference

### Core modules

| Module           | File                | Purpose                                                                                         |
| ---------------- | ------------------- | ----------------------------------------------------------------------------------------------- |
| `model`          | `model.rs`          | All domain types: `RoleId`, `RoleSpec`, `RawRoleSource`, `EffectiveRoleProfile`, policies, etc. |
| `error`          | `error.rs`          | `RoleError` enum and `RoleResult` type alias                                                    |
| `parser`         | `parser.rs`         | Markdown → `RawRoleSource` extraction                                                           |
| `normalizer`     | `normalizer.rs`     | `RawRoleSource` → `RoleSpec` with heuristic mapping                                             |
| `validator`      | `validator.rs`      | Structural and semantic validation of `RoleSpec`                                                |
| `registry`       | `registry.rs`       | `DashMap`-based concurrent role registry                                                        |
| `hierarchy`      | `hierarchy.rs`      | Organigram relationship graph                                                                   |
| `loader`         | `loader.rs`         | Full loading pipeline: discover → parse → normalize → validate → hierarchy → register           |
| `resolver`       | `resolver.rs`       | Tenant-aware policy resolution with most-restrictive-wins semantics                             |
| `policy_binding` | `policy_binding.rs` | Map role types to framework types (tool perms, capability sets, system prompts)                 |

### Feature-gated bridge modules

| Module               | Feature flag  | Purpose                                                 |
| -------------------- | ------------- | ------------------------------------------------------- |
| `personality_bridge` | `personality` | `RoleSpec` → `PersonalityOverlay` conversion            |
| `memory_bridge`      | `memory`      | `RoleMemoryPolicy` → `MemoryAccessProfile` conversion   |
| `learning_bridge`    | `learning`    | `RoleLearningPolicy` → `LearningScopeConfig` conversion |

All three features are enabled by default.

## Key Domain Types

### `RoleId`

Deterministic UUID v5 derived from the role name (using DNS namespace). This ensures the same role always gets the same ID across loads.

### `RoleSpec`

The canonical role specification (~30 fields), organized into sections:

- **Identity:** `id`, `name`, `version`, `department`
- **Purpose:** `description`, `mission`, `success_measure`
- **Responsibilities:** `responsibilities`, `decision_rights`, `non_responsibilities`
- **Capabilities:** `required_capabilities`, `kpis`
- **I/O:** `main_inputs`, `main_outputs`
- **Relationships:** `interfaces`, `collaboration_rules`, `escalation`, `supervisor`, `subordinates`
- **Personality:** `personality` (traits, working principles, thinking model, core questions, tone)
- **Prompt:** `prompt_template` (preamble, response structure)
- **Policy bindings:** `tool_policy`, `memory_policy`, `learning_policy`, `provider_preferences`
- **Governance:** `agent_kind`, `trust_level`, `metadata`

### `EffectiveRoleProfile`

The runtime-resolved version of a `RoleSpec`, after applying tenant overrides. Used by the orchestrator and bridges.

### `DepartmentCategory`

Enum matching the `roles/` directory structure:

- `Governance` (00_GOVERNANCE)
- `ProductTech` (01_PRODUCT_TECH)
- `GrowthRevenue` (02_GROWTH_REVENUE)
- `Customer` (03_CUSTOMER)
- `People` (04_PEOPLE)
- `BackOffice` (05_BACKOFFICE)
- `Custom(String)` for extensions

## Conflict Resolution

When role definitions conflict with tenant overrides or platform policies, the **most restrictive rule wins**:

| Dimension         | Resolution            |
| ----------------- | --------------------- |
| Trust level       | `min(base, override)` |
| Tool allowed list | Intersection          |
| Tool denied list  | Union                 |
| Learning enabled  | `base AND override`   |
| Require approval  | `base OR override`    |
| Memory scopes     | Intersection          |
| Max sensitivity   | Lower level wins      |

## Integration Points

### swarm-core

- `AgentDescriptor.role_id: Option<String>` — links an agent to its role
- `EventKind::RoleLoaded`, `RoleAssigned`, `RoleUnassigned` — role lifecycle events

### swarm-config

- `SwarmConfig.roles: RolesConfig` — role loading configuration (directory, auto-load, strict validation)

### swarm-orchestrator

- `Scheduler` considers `role_hint` in task metadata for role-affinity scheduling
- When a task's metadata contains `role_hint`, agents with matching `role_id` are preferred

### swarm-personality (via `personality_bridge`)

- `overlay_from_role(&RoleSpec)` → `PersonalityOverlay`
- Maps: agent kind → collaboration pattern, trust level → risk tolerance, personality traits → decision tendencies

### swarm-memory (via `memory_bridge`)

- `access_profile_from_role(&RoleMemoryPolicy, agent_id)` → `MemoryAccessProfile`
- Always grants agent self-scope; maps readable/writable scope labels to `MemoryScope` variants

### swarm-learning (via `learning_bridge`)

- `scope_config_from_role(&RoleLearningPolicy, agent_id)` → `LearningScopeConfig`

## Role Loading Pipeline

```rust
use swarm_role::{RoleLoader, RoleRegistry, RoleHierarchy};

let registry = RoleRegistry::new();
let hierarchy = RoleHierarchy::from_default_organigram();
let loader = RoleLoader::new(registry.clone(), hierarchy);

let result = loader.load_directory("roles/")?;
// result.loaded: Vec<RoleSpec> — successfully loaded roles
// result.errors: Vec<(String, RoleError)> — files that failed
// result.warnings: Vec<(String, Vec<ValidationIssue>)> — validation warnings
```

## Role → Agent Wiring

```rust
use swarm_role::personality_bridge::overlay_from_role;
use swarm_role::memory_bridge::access_profile_from_role;
use swarm_role::learning_bridge::scope_config_from_role;
use swarm_role::RolePolicyBinding;

let spec: &RoleSpec = registry.get(&role_id)?;

// 1. Generate personality overlay
let overlay = overlay_from_role(spec);

// 2. Generate memory profile
let memory = access_profile_from_role(&spec.memory_policy, &agent_id.to_string());

// 3. Generate learning config
let learning = scope_config_from_role(&spec.learning_policy, &agent_id.to_string());

// 4. Generate system prompt
let binding = RolePolicyBinding::from_spec(spec);
let system_prompt = binding.build_system_prompt();

// 5. Get tool permissions
let tools = binding.to_tool_permissions();
```

## Organizational Hierarchy

The `RoleHierarchy` encodes all 21 supervision edges from `ORGANIGRAM.md`:

```text
CEO
├── COO
│   ├── CTO/Engineering
│   │   ├── Security & Privacy
│   │   ├── UX/Design
│   │   └── Data & Analytics
│   ├── Product
│   ├── Delivery
│   ├── Internal IT
│   └── Procurement & Vendor
├── CFO
├── Chief of Staff
│   ├── Legal & Compliance
│   └── People & Culture
│       ├── Talent Acquisition
│       └── Learning & Enablement
├── Growth/Performance
│   ├── Marketing
│   ├── Sales
│   └── Partnerships/BizDev
└── Customer Success
    └── Support
```

## Testing

The crate has 39 unit tests covering:

- Model types and deterministic ID generation
- Markdown parsing (happy path and edge cases)
- Normalization heuristics
- Validation rules
- Registry CRUD operations
- Hierarchy graph traversal
- Resolver conflict resolution
- Policy binding mappings
- Personality bridge
- Memory bridge
- Learning bridge

Run tests:

```bash
cargo test -p swarm-role
```
