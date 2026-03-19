# Architecture v2 — Extended System Design

## Table of Contents

1. [Revised Requirements Catalog](#1-revised-requirements-catalog)
2. [Updated Architecture Diagram](#2-updated-architecture-diagram)
3. [Extended Domain Model](#3-extended-domain-model)
4. [Provider Abstraction Model](#4-provider-abstraction-model)
5. [Memory Architecture](#5-memory-architecture)
6. [Learning Architecture](#6-learning-architecture)
7. [Personality System Design](#7-personality-system-design)
8. [Compatibility Strategy — OpenClaw Providers](#8-compatibility-strategy--openclaw-providers)
9. [Plugin SDK Extensions](#9-plugin-sdk-extensions)
10. [Security and Compliance Updates](#10-security-and-compliance-updates)
11. [Updated Workspace / Crate Layout](#11-updated-workspace--crate-layout)
12. [MVP and Phased Rollout Plan](#12-mvp-and-phased-rollout-plan)

---

## 1. Revised Requirements Catalog

### Original requirements (preserved)

| ID    | Requirement                                               | Status    |
| ----- | --------------------------------------------------------- | --------- |
| R-001 | Platform-independent Rust framework                       | Unchanged |
| R-002 | Enterprise-ready: multi-tenant, RBAC, audit               | Unchanged |
| R-003 | Three-tier agent hierarchy (Executive / Manager / Worker) | Unchanged |
| R-004 | Capability-based task scheduling                          | Unchanged |
| R-005 | Deny-by-default policy engine                             | Unchanged |
| R-006 | Plugin SDK (native + WASM)                                | Unchanged |
| R-007 | Observability via tracing + metrics                       | Unchanged |
| R-008 | Circuit-breaker and retry for resilient execution         | Unchanged |
| R-009 | Configuration via TOML + env + secrets                    | Unchanged |

### New requirements (v2)

| ID    | Requirement                                                                                   | Priority |
| ----- | --------------------------------------------------------------------------------------------- | -------- |
| R-100 | Provider-agnostic AI model integration layer                                                  | P0       |
| R-101 | Support chat-completion, reasoning, tool-calling, embedding, speech, multimodal models        | P0       |
| R-102 | Support local, self-hosted, cloud, proxy, and SDK-based providers                             | P0       |
| R-103 | Request/response normalization layer with streaming support                                   | P0       |
| R-104 | OpenClaw-compatible provider coverage as reference target                                     | P1       |
| R-105 | Compatibility matrix, feature negotiation, graceful degradation                               | P1       |
| R-110 | First-class personality system for agents                                                     | P0       |
| R-111 | Configurable, versioned, composable, policy-constrained personalities                         | P0       |
| R-112 | Default, org-defined, task-overlay, and compliance-restricted personalities                   | P1       |
| R-120 | First-class memory subsystem with multiple scopes                                             | P0       |
| R-121 | Structured, semantic, episodic, procedural memory types                                       | P0       |
| R-122 | Retention policies, redaction, expiration, auditability                                       | P1       |
| R-123 | Privacy-aware, policy-aware, access-controlled memory                                         | P0       |
| R-130 | First-class learning subsystem                                                                | P1       |
| R-131 | Preference adaptation, pattern extraction, feedback incorporation                             | P1       |
| R-132 | Reviewable, auditable, rollback-capable, disableable learning                                 | P0       |
| R-140 | Extended agent cognition model (identity + personality + memory + learning + provider prefs)  | P0       |
| R-150 | Provider selection strategies (cost, latency, compliance, locality)                           | P1       |
| R-151 | Model routing and failover between providers                                                  | P1       |
| R-160 | Plugin categories: provider adapters, memory backends, learning strategies, personality packs | P0       |
| R-170 | Security reassessment for multi-provider, sensitive memory, enterprise learning               | P0       |

---

## 2. Updated Architecture Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                        Interface Layer                            │
│  swarm-cli │ API surface (future HTTP/gRPC) │ operator dashboard  │
├──────────────────────────────────────────────────────────────────┤
│                   Plugin / Integration Layer                      │
│  swarm-plugin SDK │ provider adapters │ memory backends │         │
│  learning strategies │ personality packs │ tool plugins │         │
│  workflow plugins │ enterprise connectors                         │
├──────────────────────────────────────────────────────────────────┤
│                      Cognition Layer  [NEW]                       │
│  swarm-personality │ swarm-memory │ swarm-learning                │
│  personality profiles │ memory scopes │ learning policies          │
│  context assembly │ preference adaptation                         │
├──────────────────────────────────────────────────────────────────┤
│                    Provider Layer  [NEW]                           │
│  swarm-provider: provider registry │ capability discovery │       │
│  request translation │ response normalization │ streaming │       │
│  tool mediation │ token accounting │ rate limiting │ routing      │
├──────────────────────────────────────────────────────────────────┤
│                      Policy Layer                                 │
│  swarm-policy: RBAC engine │ policy evaluation │ admission │      │
│  provider routing policies │ memory access policies │             │
│  learning governance │ personality boundary policies              │
├──────────────────────────────────────────────────────────────────┤
│                 Orchestration / Control Plane                     │
│  swarm-orchestrator: registry │ scheduler │ supervision │         │
│  provider-aware scheduling │ cognition-aware dispatch             │
├──────────────────────────────────────────────────────────────────┤
│                   Execution / Runtime Layer                       │
│  swarm-runtime: task runner │ circuit breaker │ retry │           │
│  provider call execution │ memory retrieval integration           │
├──────────────────────────────────────────────────────────────────┤
│                Configuration & Observability                      │
│  swarm-config │ swarm-telemetry │ provider health monitoring      │
├──────────────────────────────────────────────────────────────────┤
│                      Core Domain Layer                            │
│  swarm-core: types │ traits │ errors │ events │                   │
│  extended agent model │ provider trait │ memory trait │            │
│  personality trait │ learning trait                                │
└──────────────────────────────────────────────────────────────────┘
```

### Layer additions

| Layer     | New crate(s)        | Key concerns                                                                                                                                                                                                     |
| --------- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Provider  | `swarm-provider`    | Provider registry; model capability discovery; request/response normalization; streaming; tool invocation mediation; token/cost accounting; rate limiting; retry/fallback routing; provider selection strategies |
| Cognition | `swarm-personality` | Personality profiles; communication style; decision tendencies; risk tolerance; composable overlays; policy-constrained boundaries                                                                               |
| Cognition | `swarm-memory`      | Memory scopes (session, task, agent, team, tenant, long-term); memory types (structured, semantic, episodic, procedural); retrieval; indexing; summarization; retention; redaction; access control               |
| Cognition | `swarm-learning`    | Preference adaptation; pattern extraction; feedback incorporation; plan templates; scoring; human-approved updates; rollback; audit; tenant isolation                                                            |

---

## 3. Extended Domain Model

### Agent Cognition Model

The `AgentDescriptor` is extended with cognition-related profiles:

```rust
struct AgentDescriptor {
    // --- existing fields ---
    id: AgentId,
    name: String,
    kind: AgentKind,
    capabilities: CapabilitySet,
    resource_limits: ResourceLimits,
    metadata: Metadata,
    registered_at: Timestamp,

    // --- new cognition fields ---
    personality_id: Option<PersonalityId>,
    memory_profile: MemoryAccessProfile,
    learning_policy: LearningPolicyRef,
    trust_level: TrustLevel,
    provider_preferences: ProviderPreferences,
    tool_permissions: ToolPermissions,
    operational_constraints: OperationalConstraints,
}
```

### New identity types

```rust
define_id!(PersonalityId, "Unique identifier for a personality profile.");
define_id!(MemoryId, "Unique identifier for a memory record.");
define_id!(ProviderId, "Unique identifier for an AI provider registration.");
define_id!(LearningRuleId, "Unique identifier for a learning rule.");
```

### Agent state model boundaries

```
┌─────────────────────────────────────────────────────────┐
│                    Agent Instance                         │
│                                                           │
│  ┌─────────────┐  ┌─────────────┐  ┌────────────────┐   │
│  │  Identity    │  │ Personality │  │  Memory Access  │   │
│  │  + Role      │  │  Profile    │  │  Profile        │   │
│  │  + Trust     │  │  (R/O ref)  │  │  (scope rules)  │   │
│  └─────────────┘  └─────────────┘  └────────────────┘   │
│                                                           │
│  ┌─────────────┐  ┌─────────────┐  ┌────────────────┐   │
│  │  Policy      │  │  Planning   │  │  Learning       │   │
│  │  Bindings    │  │  (runtime)  │  │  Policy Ref     │   │
│  └─────────────┘  └─────────────┘  └────────────────┘   │
│                                                           │
│  ┌─────────────┐  ┌─────────────┐  ┌────────────────┐   │
│  │  Provider    │  │  Execution  │  │  Health         │   │
│  │  Preferences │  │  State      │  │  State          │   │
│  └─────────────┘  └─────────────┘  └────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

Boundaries:

- **Prompt/Personality** — read-only configuration; never mutated at runtime.
- **Memory/Context** — accessed via retrieval interfaces; governed by scope and policy.
- **Policy** — evaluated before every sensitive operation; cannot be bypassed.
- **Planning** — runtime-only; derives from task spec + personality + memory context.
- **Execution** — isolated per task invocation; feeds back into memory/learning.
- **Learning updates** — post-execution; require policy approval; auditable.

---

## 4. Provider Abstraction Model

### Core traits

```rust
/// A registered AI model provider.
#[async_trait]
trait ModelProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn name(&self) -> &str;
    fn discover_capabilities(&self) -> ProviderCapabilities;
    async fn chat_completion(&self, req: ChatRequest) -> SwarmResult<ChatResponse>;
    async fn chat_completion_stream(&self, req: ChatRequest)
        -> SwarmResult<Pin<Box<dyn Stream<Item = StreamEvent>>>>;
    async fn embedding(&self, req: EmbeddingRequest) -> SwarmResult<EmbeddingResponse>;
    async fn health_check(&self) -> SwarmResult<ProviderHealth>;
}
```

### Capability discovery

```rust
struct ProviderCapabilities {
    chat_completion: bool,
    streaming: bool,
    tool_calling: bool,
    reasoning: bool,
    embeddings: bool,
    speech: bool,
    multimodal: bool,
    vision: bool,
    supported_models: Vec<ModelDescriptor>,
    max_context_window: Option<u64>,
    max_output_tokens: Option<u64>,
    supports_json_mode: bool,
    supports_function_calling: bool,
    custom_capabilities: HashMap<String, serde_json::Value>,
}
```

### Normalization layer

```
Client code                Provider Adapter
    │                           │
    ├── ChatRequest ──────────► ├── vendor-specific request
    │                           │   (HTTP, gRPC, SDK call)
    │                           │
    ◄── ChatResponse ──────────◄── vendor-specific response
    │   (normalized)            │
    ◄── StreamEvent ───────────◄── vendor-specific SSE/WS
    │   (normalized)            │
    ◄── TokenUsage ────────────◄── vendor-specific metering
    ◄── ProviderError ─────────◄── vendor-specific error
```

### Request/response types

- `ChatRequest` — normalized request with messages, tools, parameters, constraints.
- `ChatResponse` — normalized response with content, tool calls, usage, finish reason.
- `StreamEvent` — normalized streaming delta (content chunk, tool call delta, done signal).
- `EmbeddingRequest` / `EmbeddingResponse` — normalized embedding I/O.
- `TokenUsage` — prompt tokens, completion tokens, total, cost estimate.
- `ProviderError` — normalized error with category, retryability, provider-specific details.

### Provider selection and routing

```rust
trait ProviderRouter: Send + Sync {
    async fn select_provider(&self, req: &RoutingRequest) -> SwarmResult<ProviderId>;
}

struct RoutingRequest {
    required_capabilities: ProviderCapabilities,
    cost_preference: CostPreference,
    latency_preference: LatencyPreference,
    compliance_requirements: Vec<ComplianceRequirement>,
    data_locality: Option<DataLocality>,
    tenant_preferences: Option<TenantProviderPrefs>,
    fallback_allowed: bool,
}

enum CostPreference { Cheapest, Balanced, BestQuality }
enum LatencyPreference { Fastest, Balanced, NoPreference }
```

### Failover

The router maintains an ordered list of providers per capability class. On failure, it tries the next provider in the list, respecting:

- Rate limits and quotas
- Compliance boundaries (data must not leave a region)
- Tenant-specific allowlists/blocklists
- Circuit-breaker state per provider

---

## 5. Memory Architecture

### Memory scopes

| Scope    | Lifetime                        | Isolation       | Use case                        |
| -------- | ------------------------------- | --------------- | ------------------------------- |
| Session  | Single conversation/interaction | Per session     | Conversation context            |
| Task     | Single task execution           | Per task        | Working memory during execution |
| Agent    | Lifetime of an agent            | Per agent       | Learned preferences, patterns   |
| Team     | Shared across a team of agents  | Per team/domain | Shared knowledge                |
| Tenant   | Organization-wide               | Per tenant      | Org policies, knowledge base    |
| LongTerm | Persistent across restarts      | Per scope owner | Accumulated knowledge           |

### Memory types

| Type         | Structure                    | Example                              |
| ------------ | ---------------------------- | ------------------------------------ |
| Structured   | Key-value or tabular         | Configuration snapshots, fact tables |
| Semantic     | Vector-embedded content      | Searchable knowledge fragments       |
| Episodic     | Timestamped event records    | Past task executions, interactions   |
| Procedural   | Step sequences / plans       | Reusable workflow templates          |
| KnowledgeRef | URI/pointer to external data | Links to documents, databases        |
| Summary      | Compressed representation    | Distilled conversation history       |

### Core traits

```rust
#[async_trait]
trait MemoryBackend: Send + Sync {
    async fn store(&self, entry: MemoryEntry) -> SwarmResult<MemoryId>;
    async fn retrieve(&self, query: MemoryQuery) -> SwarmResult<Vec<MemoryEntry>>;
    async fn delete(&self, id: &MemoryId) -> SwarmResult<()>;
    async fn expire(&self, policy: &RetentionPolicy) -> SwarmResult<u64>;
    async fn redact(&self, id: &MemoryId, fields: &[String]) -> SwarmResult<()>;
}
```

### Access control

Every memory operation is subject to:

1. **Scope check** — agent may only access scopes in its `MemoryAccessProfile`.
2. **Policy evaluation** — the policy engine evaluates `read:memory`, `write:memory` etc.
3. **Tenant isolation** — memory from one tenant is never visible to another.
4. **Redaction rules** — PII or sensitive fields may be masked per policy.

### Retention and governance

```rust
struct RetentionPolicy {
    scope: MemoryScope,
    max_age: Option<Duration>,
    max_entries: Option<u64>,
    auto_summarize: bool,
    summarize_after: Option<Duration>,
    require_audit_on_delete: bool,
}
```

---

## 6. Learning Architecture

### Learning mechanisms

| Mechanism               | Description                                   | Safety level           |
| ----------------------- | --------------------------------------------- | ---------------------- |
| Preference adaptation   | Adjust weights/rankings based on feedback     | Safe — no model change |
| Pattern extraction      | Identify successful task sequences            | Safe — stored as plans |
| Feedback incorporation  | Human or automated quality signals            | Safe — auditable       |
| Plan template creation  | Reusable workflow patterns                    | Safe — versioned       |
| Scoring improvements    | Update heuristic scores for routing           | Safe — bounded         |
| Knowledge accumulation  | Org-specific facts from task outcomes         | Safe — policy-gated    |
| Configuration evolution | Suggest config changes from performance data  | Requires approval      |
| Fine-tuning hooks       | Export training data for external fine-tuning | Requires approval      |

### Distinction of learning modes

```
Runtime Adaptation          → Ephemeral; within session; no persistence
Memory Formation            → Persisted as memory entries; governed by retention
Configuration Evolution     → Proposed changes to agent configs; requires review
Model Fine-tuning Hooks     → Data export; external process; never automatic
Human-approved Updates      → Learning deltas queued for human review
```

### Core traits

```rust
#[async_trait]
trait LearningStrategy: Send + Sync {
    fn id(&self) -> LearningRuleId;
    fn name(&self) -> &str;
    async fn observe(&self, event: &LearningEvent) -> SwarmResult<Vec<LearningOutput>>;
    async fn apply(&self, output: &LearningOutput, ctx: &LearningContext)
        -> SwarmResult<LearningResult>;
    fn requires_approval(&self) -> bool;
}

#[async_trait]
trait LearningStore: Send + Sync {
    async fn record(&self, output: LearningOutput) -> SwarmResult<()>;
    async fn list_pending_approvals(&self, scope: &LearningScope) -> SwarmResult<Vec<LearningOutput>>;
    async fn approve(&self, id: &LearningRuleId) -> SwarmResult<()>;
    async fn reject(&self, id: &LearningRuleId) -> SwarmResult<()>;
    async fn rollback(&self, id: &LearningRuleId) -> SwarmResult<()>;
}
```

### Controls

- **Reviewability**: all learning deltas stored with full context.
- **Auditability**: every approved/rejected/rolled-back decision is logged.
- **Rollback**: applied learning outputs can be reverted to previous state.
- **Permissions**: learning operations require `learn:*` RBAC permissions.
- **Tenant isolation**: learning data is scoped to the tenant.
- **Disableable**: learning can be disabled per tenant, team, agent, or workflow via policy.

---

## 7. Personality System Design

### Personality profile

```rust
struct PersonalityProfile {
    id: PersonalityId,
    name: String,
    version: String,
    communication_style: CommunicationStyle,
    decision_tendencies: DecisionTendencies,
    risk_tolerance: RiskTolerance,
    collaboration_pattern: CollaborationPattern,
    escalation_behavior: EscalationBehavior,
    domain_hints: Vec<String>,
    response_formatting: ResponseFormatting,
    custom_traits: HashMap<String, serde_json::Value>,
}
```

### Composability

Personalities are composable via layered overlays:

```
Base personality (framework default)
  └── Organization personality (tenant-defined)
        └── Role personality (per AgentKind)
              └── Task overlay (per task/workflow)
```

Each layer can override, extend, or restrict traits from the layer above. The final "effective personality" is computed at task dispatch time.

### Policy constraints

- Personalities cannot override security policies.
- Compliance-restricted boundaries define which traits may be set.
- The policy engine evaluates personality changes like any other action.
- A `PersonalityBoundary` defines min/max for risk tolerance, restricted styles, etc.

---

## 8. Compatibility Strategy — OpenClaw Providers

### Approach

OpenClaw serves as the **reference compatibility target** for provider coverage. AiOfficeSwarm does not embed OpenClaw or depend on it at compile time. Instead:

1. A **compatibility matrix** is maintained mapping OpenClaw provider names to AiOfficeSwarm adapter plugin status.
2. Each provider integration is delivered as a **plugin** implementing `ModelProvider`.
3. Feature gaps are documented with negotiation rules and fallback behavior.

### Compatibility matrix (initial)

| OpenClaw Provider | Model types                         | Adapter status    | Fallback       |
| ----------------- | ----------------------------------- | ----------------- | -------------- |
| OpenAI            | Chat, Embedding, Vision, TTS, Tools | Planned (MVP)     | —              |
| Azure OpenAI      | Chat, Embedding, Vision, Tools      | Planned (MVP)     | OpenAI adapter |
| Anthropic         | Chat, Vision, Tools                 | Planned (Phase 2) | —              |
| Google Gemini     | Chat, Vision, Embedding, Tools      | Planned (Phase 2) | —              |
| Ollama            | Chat, Embedding (local)             | Planned (Phase 2) | —              |
| LM Studio         | Chat (local)                        | Planned (Phase 3) | Ollama adapter |
| Mistral           | Chat, Embedding, Tools              | Planned (Phase 3) | —              |
| Groq              | Chat, Tools                         | Planned (Phase 3) | —              |
| Cohere            | Chat, Embedding                     | Planned (Phase 3) | —              |
| HuggingFace       | Chat, Embedding                     | Planned (Phase 3) | —              |
| AWS Bedrock       | Chat, Embedding                     | Planned (Phase 3) | —              |
| OpenRouter        | Chat (proxy)                        | Planned (Phase 3) | —              |

### Feature negotiation

When a requested capability is not available from the selected provider:

1. **Negotiate**: check `ProviderCapabilities` for alternative.
2. **Degrade**: if tool-calling is unavailable, fall back to prompt-based tool simulation.
3. **Fail clearly**: return `SwarmError::ProviderCapabilityUnavailable` with details.
4. **Route elsewhere**: the `ProviderRouter` may select an alternative provider.

### Future alignment

Adding a new provider requires:

1. Implement `ModelProvider` trait in a plugin crate.
2. Register capabilities in the provider registry.
3. Update the compatibility matrix.
4. No core refactoring needed.

---

## 9. Plugin SDK Extensions

### New plugin categories

```rust
enum PluginCapabilityKind {
    // --- existing ---
    AgentProvider,
    ActionProvider,
    StorageBackend,
    CommunicationChannel,
    PolicyProvider,
    TriggerProvider,
    // --- new ---
    ProviderAdapter,
    MemoryBackend,
    LearningStrategy,
    PersonalityPack,
    WorkflowProvider,
    EnterpriseConnector,
}
```

### Provider adapter plugins

Provider adapter plugins implement `ModelProvider` and are loaded via the plugin host. They must declare:

- Supported model types / capabilities
- Required secrets (API keys, endpoints)
- Network permissions (for WASM adapters)
- Rate limit configuration

### Memory backend plugins

Memory backend plugins implement `MemoryBackend` and provide alternative storage engines (e.g., PostgreSQL, Redis, Qdrant, Pinecone, local file system).

### Learning strategy plugins

Learning strategy plugins implement `LearningStrategy` and provide custom learning algorithms (preference ranking, pattern extraction, etc.).

### Personality pack plugins

Personality pack plugins bundle one or more `PersonalityProfile` definitions that can be loaded and applied to agents.

### Plugin lifecycle compatibility

All new plugin types share the same lifecycle model:
`Discovered → Loading → Active → Unloading → Unloaded / Failed`

Version compatibility is checked via `min_host_version` in the manifest.

---

## 10. Security and Compliance Updates

### Multi-provider communication

- Each provider adapter plugin declares allowed network endpoints.
- Secrets are managed via `swarm-config` secrets abstraction; never embedded.
- TLS is mandatory for all external provider communication.
- Provider-specific data handling differences are documented per adapter.

### Sensitive memory

- Memory entries may be tagged with sensitivity levels.
- PII detection hooks allow policies to require redaction before storage.
- Encryption-at-rest is delegated to the memory backend plugin.
- Memory access is logged for audit when sensitivity exceeds a threshold.

### Learning from enterprise data

- Learning strategies must declare what data they observe.
- Tenant isolation is enforced: learning data never crosses tenant boundaries.
- Human-approved learning updates require explicit `approve:learning` permission.
- All learning deltas are immutable audit records.

### Policy hooks (new)

| Hook                            | Controls                                            |
| ------------------------------- | --------------------------------------------------- |
| `restrict:provider`             | Which providers may be used by which agents/tenants |
| `restrict:data-boundary`        | What data may leave a compliance boundary           |
| `restrict:memory-access`        | Which memory scopes/types are retrievable           |
| `restrict:learning`             | Whether learning is enabled per scope               |
| `restrict:personality-override` | Whether personalities may be overridden             |
| `restrict:model-routing`        | Which models/regions are allowed                    |
| `restrict:tool-invocation`      | Which tools an agent may call via a provider        |

### Cross-border routing

- Data locality constraints are expressed in `RoutingRequest`.
- The provider router evaluates them before selecting a provider.
- Violations result in `SwarmError::ComplianceBoundaryViolation`.

---

## 11. Updated Workspace / Crate Layout

```
crates/
    swarm-core/           # Core domain types, traits, errors, events  (EXTENDED)
    swarm-provider/       # NEW — Provider abstraction layer
    swarm-personality/    # NEW — Personality system
    swarm-memory/         # NEW — Memory subsystem
    swarm-learning/       # NEW — Learning subsystem
    swarm-orchestrator/   # Orchestration control plane
    swarm-policy/         # Policy engine
    swarm-plugin/         # Plugin SDK  (EXTENDED with new categories)
    swarm-runtime/        # Async task execution
    swarm-config/         # Configuration and secrets
    swarm-telemetry/      # Observability
    swarm-cli/            # CLI binary
```

### Dependency graph (new crates)

```
swarm-core
    ↑
    ├── swarm-provider     (depends on swarm-core)
    ├── swarm-personality  (depends on swarm-core)
    ├── swarm-memory       (depends on swarm-core)
    ├── swarm-learning     (depends on swarm-core, swarm-memory)
    ├── swarm-plugin       (depends on swarm-core)  — extended
    ├── swarm-policy       (depends on swarm-core)
    ├── swarm-orchestrator (depends on swarm-core, swarm-provider, swarm-personality,
    │                       swarm-memory, swarm-learning)
    └── swarm-runtime      (depends on swarm-core, swarm-provider)
```

---

## 12. MVP and Phased Rollout Plan

### Phase 0 — Foundation (current + this PR)

**Goal**: Establish all abstraction points and module boundaries.

- [x] Core domain types, agent model, capabilities, policy, RBAC, events
- [x] Orchestrator, scheduler, supervision, task queue
- [x] Plugin SDK with native + WASM support
- [x] Policy engine with deny-by-default
- [x] Runtime with circuit breaker and retry
- [x] Configuration and telemetry
- [ ] **NEW**: `swarm-provider` crate with `ModelProvider` trait, capability discovery, request/response types, provider registry, router trait, token accounting
- [ ] **NEW**: `swarm-personality` crate with `PersonalityProfile`, composability, `PersonalityRegistry`, policy boundary types
- [ ] **NEW**: `swarm-memory` crate with `MemoryBackend` trait, `MemoryEntry`, scopes, types, `MemoryQuery`, retention policy, `InMemoryBackend`
- [ ] **NEW**: `swarm-learning` crate with `LearningStrategy` + `LearningStore` traits, event/output types, scope controls
- [ ] **NEW**: Extended `AgentDescriptor` with cognition fields
- [ ] **NEW**: Extended plugin SDK with new capability categories
- [ ] **NEW**: Extended error model for provider/memory/learning/personality errors
- [ ] **NEW**: Extended event model for cognition events

### Phase 1 — Provider MVP

- [ ] OpenAI provider adapter plugin (chat completion, streaming, tools, embeddings)
- [ ] Azure OpenAI adapter plugin
- [ ] Provider router with basic capability matching
- [ ] Cost and token accounting integration
- [ ] Rate limiting and retry per provider

### Phase 2 — Cognition MVP

- [ ] In-memory memory backend (for development/testing)
- [ ] Session and task memory integration in runtime
- [ ] Basic personality application in agent execution
- [ ] Preference adaptation learning strategy
- [ ] Memory-aware context assembly for provider calls

### Phase 3 — Extended Provider Coverage

- [ ] Anthropic, Google Gemini, Ollama adapters
- [ ] Multi-provider failover routing
- [ ] Compliance-aware routing (data locality, model restrictions)
- [ ] Provider health monitoring dashboard

### Phase 4 — Enterprise Cognition

- [ ] Persistent memory backends (PostgreSQL, Redis, vector DB)
- [ ] Semantic memory with embedding-based retrieval
- [ ] Episodic memory with summarization pipelines
- [ ] Advanced learning strategies (pattern extraction, plan templates)
- [ ] Human-in-the-loop learning approval workflows
- [ ] Organization-level memory and personality management

### Phase 5 — Production Hardening

- [ ] Full OpenClaw provider parity
- [ ] Cross-border compliance routing
- [ ] PII detection and auto-redaction
- [ ] Learning rollback and audit tooling
- [ ] Performance optimization (memory indexing, provider connection pooling)
- [ ] Distributed deployment support
