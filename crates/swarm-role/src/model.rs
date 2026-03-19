//! Role domain model.
//!
//! This module defines the complete type hierarchy for roles:
//!
//! - [`RawRoleSource`] — unvalidated parse result from a role file.
//! - [`RoleSpec`] — normalized, validated, versioned role specification.
//! - [`EffectiveRoleProfile`] — resolved runtime profile after policy and tenant overrides.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ────────────────────────────────────────────────────────────────────────────────
// Role identity
// ────────────────────────────────────────────────────────────────────────────────

/// Unique identifier for a role specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoleId(pub Uuid);

impl RoleId {
    /// Generate a new random role ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a deterministic ID from a role name (UUID v5 using the DNS namespace).
    pub fn from_name(name: &str) -> Self {
        Self(Uuid::new_v5(&Uuid::NAMESPACE_DNS, name.as_bytes()))
    }
}

impl Default for RoleId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RoleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// Department categories
// ────────────────────────────────────────────────────────────────────────────────

/// Organizational department categories derived from the directory structure.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DepartmentCategory {
    /// Governance roles (CEO, CFO, COO, Chief of Staff, Legal).
    Governance,
    /// Product and technology roles.
    ProductTech,
    /// Growth and revenue roles.
    GrowthRevenue,
    /// Customer-facing roles.
    Customer,
    /// People and culture roles.
    People,
    /// Back-office and infrastructure roles.
    BackOffice,
    /// Custom department not in the standard taxonomy.
    Custom(String),
}

impl DepartmentCategory {
    /// Parse a department from a directory prefix like `"00_GOVERNANCE"`.
    pub fn from_dir_prefix(prefix: &str) -> Self {
        match prefix {
            "00_GOVERNANCE" => Self::Governance,
            "01_PRODUCT_TECH" => Self::ProductTech,
            "02_GROWTH_REVENUE" => Self::GrowthRevenue,
            "03_CUSTOMER" => Self::Customer,
            "04_PEOPLE" => Self::People,
            "05_BACKOFFICE" => Self::BackOffice,
            other => Self::Custom(other.to_string()),
        }
    }

    /// Return a stable label string for serialization and logging.
    pub fn label(&self) -> &str {
        match self {
            Self::Governance => "governance",
            Self::ProductTech => "product_tech",
            Self::GrowthRevenue => "growth_revenue",
            Self::Customer => "customer",
            Self::People => "people",
            Self::BackOffice => "back_office",
            Self::Custom(s) => s.as_str(),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// Raw role source — unvalidated parse result
// ────────────────────────────────────────────────────────────────────────────────

/// An unvalidated, intermediate representation parsed from a role Markdown file.
///
/// This preserves whatever structure was found in the source file. It is
/// subsequently normalized and validated into a [`RoleSpec`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawRoleSource {
    /// File path relative to the roles directory.
    pub source_path: String,
    /// The department directory this file was found in.
    pub department_dir: Option<String>,
    /// Extracted H1 heading (role name).
    pub title: Option<String>,
    /// Sections extracted from the Markdown structure.
    /// Keys are lowercase section headings, values are the text body.
    pub sections: HashMap<String, String>,
}

// ────────────────────────────────────────────────────────────────────────────────
// Escalation policy
// ────────────────────────────────────────────────────────────────────────────────

/// Defines when and how a role should escalate issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationPolicy {
    /// Conditions that trigger escalation (free-form descriptions).
    pub triggers: Vec<String>,
    /// Target roles to escalate to (by name or ID).
    pub escalation_targets: Vec<String>,
    /// Maximum retries before escalation is mandatory.
    pub max_retries_before_escalation: Option<u32>,
    /// Whether escalation includes full context.
    pub include_full_context: bool,
}

impl Default for EscalationPolicy {
    fn default() -> Self {
        Self {
            triggers: Vec::new(),
            escalation_targets: Vec::new(),
            max_retries_before_escalation: Some(2),
            include_full_context: true,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// Tool access policy (role-level)
// ────────────────────────────────────────────────────────────────────────────────

/// Defines which tools a role is permitted or forbidden to use.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleToolPolicy {
    /// Tool names this role is explicitly allowed to use (empty = defer to agent/policy).
    pub allowed_tools: Vec<String>,
    /// Tool names this role must never use.
    pub denied_tools: Vec<String>,
    /// Plugin capabilities this role requires.
    pub required_plugin_capabilities: Vec<String>,
    /// Maximum tool calls per task execution.
    pub max_tool_calls_per_task: Option<u32>,
}

// ────────────────────────────────────────────────────────────────────────────────
// Memory access policy (role-level)
// ────────────────────────────────────────────────────────────────────────────────

/// Defines memory access boundaries for a role.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleMemoryPolicy {
    /// Memory scope labels this role may read.
    pub readable_scopes: Vec<String>,
    /// Memory scope labels this role may write.
    pub writable_scopes: Vec<String>,
    /// Maximum sensitivity level this role may access (e.g., `"confidential"`).
    pub max_sensitivity: Option<String>,
    /// Retention policy hint (e.g., `"session"`, `"persistent"`, `"ephemeral"`).
    pub retention_hint: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────────────
// Learning policy (role-level)
// ────────────────────────────────────────────────────────────────────────────────

/// Defines learning boundaries for a role.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleLearningPolicy {
    /// Whether learning is enabled for agents with this role.
    pub enabled: bool,
    /// Whether learned outputs require human approval.
    pub require_approval: bool,
    /// Categories of learning this role is allowed to perform.
    pub allowed_categories: Vec<String>,
    /// Categories explicitly forbidden.
    pub denied_categories: Vec<String>,
}

// ────────────────────────────────────────────────────────────────────────────────
// Collaboration rule
// ────────────────────────────────────────────────────────────────────────────────

/// Describes a collaboration relationship with another role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleCollaborationRule {
    /// The name of the collaborating role.
    pub target_role: String,
    /// Nature of the collaboration (e.g., `"consult"`, `"delegate"`, `"report"`).
    pub relationship: String,
    /// Whether this collaboration is mandatory or optional.
    pub required: bool,
}

// ────────────────────────────────────────────────────────────────────────────────
// Role metadata
// ────────────────────────────────────────────────────────────────────────────────

/// Arbitrary metadata attached to a role specification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleMetadata {
    /// Free-form tags for searching and filtering.
    pub tags: Vec<String>,
    /// Custom key-value pairs.
    pub custom: HashMap<String, serde_json::Value>,
    /// Source file path (relative to roles directory).
    pub source_path: Option<String>,
    /// SHA-256 hash of the source file for change detection.
    pub source_hash: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────────────
// Prompt template
// ────────────────────────────────────────────────────────────────────────────────

/// A structured prompt template extracted from a role definition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptTemplate {
    /// The system prompt preamble.
    pub system_preamble: String,
    /// Response structure instructions.
    pub response_structure: Vec<String>,
}

// ────────────────────────────────────────────────────────────────────────────────
// Personality spec (role-level)
// ────────────────────────────────────────────────────────────────────────────────

/// Personality traits extracted from a role definition, to be mapped into
/// a [`PersonalityOverlay`] by the integration layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RolePersonalitySpec {
    /// Adjectives describing the personality (e.g., `"analytical"`, `"calm"`).
    pub traits: Vec<String>,
    /// Working principles that guide behavior.
    pub working_principles: Vec<String>,
    /// Mental model / thinking patterns.
    pub thinking_model: Vec<String>,
    /// Core questions this role asks.
    pub core_questions: Vec<String>,
    /// Communication tone hint.
    pub tone: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────────────
// Provider preferences (role-level)
// ────────────────────────────────────────────────────────────────────────────────

/// Provider/model preferences specified at the role level.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleProviderPreferences {
    /// Preferred provider name (e.g., `"openai"`, `"anthropic"`).
    pub preferred_provider: Option<String>,
    /// Preferred model identifier.
    pub preferred_model: Option<String>,
    /// Minimum capability tier required (e.g., `"reasoning"`, `"basic"`).
    pub minimum_capability_tier: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────────────
// RoleSpec — the canonical normalized specification
// ────────────────────────────────────────────────────────────────────────────────

/// A fully normalized, validated, and typed role specification.
///
/// This is the primary role abstraction in the system. It captures everything
/// the framework needs to instantiate, govern, and execute an agent in a
/// specific organizational role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSpec {
    // ── Identity ────────────────────────────────────────────────────────
    /// Unique, deterministic identifier derived from the role name.
    pub id: RoleId,
    /// Human-readable role name (e.g., `"CEO Agent"`).
    pub name: String,
    /// Semantic version of this role specification.
    pub version: String,
    /// Organizational department.
    pub department: DepartmentCategory,

    // ── Purpose ─────────────────────────────────────────────────────────
    /// Brief description of the role profile.
    pub description: String,
    /// The role's mission statement.
    pub mission: String,
    /// How success is measured for this role.
    pub success_measure: String,

    // ── Responsibilities ────────────────────────────────────────────────
    /// Areas of responsibility.
    pub responsibilities: Vec<String>,
    /// Decisions this role is authorized to make.
    pub decision_rights: Vec<String>,
    /// Explicit non-responsibilities (boundaries).
    pub non_responsibilities: Vec<String>,

    // ── Capabilities ────────────────────────────────────────────────────
    /// Required capabilities (mapped to `swarm-core` `Capability`).
    pub required_capabilities: Vec<String>,
    /// KPIs this role is measured against.
    pub kpis: Vec<String>,

    // ── I/O ─────────────────────────────────────────────────────────────
    /// Primary inputs this role consumes.
    pub main_inputs: Vec<String>,
    /// Primary outputs this role produces.
    pub main_outputs: Vec<String>,

    // ── Relationships ───────────────────────────────────────────────────
    /// Names of roles this role collaborates with.
    pub interfaces: Vec<String>,
    /// Collaboration rules with specific roles.
    pub collaboration_rules: Vec<RoleCollaborationRule>,
    /// Escalation policy.
    pub escalation: EscalationPolicy,
    /// Direct supervisor role name (from organigram).
    pub supervisor: Option<String>,
    /// Direct subordinate role names (from organigram).
    pub subordinates: Vec<String>,

    // ── Personality ─────────────────────────────────────────────────────
    /// Personality specification for overlay generation.
    pub personality: RolePersonalitySpec,

    // ── Prompt ──────────────────────────────────────────────────────────
    /// Structured prompt template.
    pub prompt_template: PromptTemplate,

    // ── Policy bindings ─────────────────────────────────────────────────
    /// Tool access policy.
    pub tool_policy: RoleToolPolicy,
    /// Memory access policy.
    pub memory_policy: RoleMemoryPolicy,
    /// Learning policy.
    pub learning_policy: RoleLearningPolicy,
    /// Provider preferences.
    pub provider_preferences: RoleProviderPreferences,

    // ── Governance ──────────────────────────────────────────────────────
    /// The agent hierarchy tier implied by this role.
    pub agent_kind: swarm_core::AgentKind,
    /// Trust level hint for this role.
    pub trust_level: swarm_core::TrustLevel,
    /// Metadata (tags, source info, custom data).
    pub metadata: RoleMetadata,
    /// When this specification was created or last updated.
    pub updated_at: DateTime<Utc>,
}

impl RoleSpec {
    /// Create a minimal role spec for testing or programmatic construction.
    pub fn new(name: impl Into<String>, department: DepartmentCategory) -> Self {
        let name = name.into();
        Self {
            id: RoleId::from_name(&name),
            name,
            version: "1.0.0".to_string(),
            department,
            description: String::new(),
            mission: String::new(),
            success_measure: String::new(),
            responsibilities: Vec::new(),
            decision_rights: Vec::new(),
            non_responsibilities: Vec::new(),
            required_capabilities: Vec::new(),
            kpis: Vec::new(),
            main_inputs: Vec::new(),
            main_outputs: Vec::new(),
            interfaces: Vec::new(),
            collaboration_rules: Vec::new(),
            escalation: EscalationPolicy::default(),
            supervisor: None,
            subordinates: Vec::new(),
            personality: RolePersonalitySpec::default(),
            prompt_template: PromptTemplate::default(),
            tool_policy: RoleToolPolicy::default(),
            memory_policy: RoleMemoryPolicy::default(),
            learning_policy: RoleLearningPolicy::default(),
            provider_preferences: RoleProviderPreferences::default(),
            agent_kind: swarm_core::AgentKind::Worker,
            trust_level: swarm_core::TrustLevel::Standard,
            metadata: RoleMetadata::default(),
            updated_at: Utc::now(),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// Tenant role override
// ────────────────────────────────────────────────────────────────────────────────

/// A tenant-specific override layer applied on top of a base [`RoleSpec`].
///
/// Only set fields override the base. This allows multi-tenant deployments
/// to customize roles without corrupting the default role model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantRoleOverride {
    /// Tenant this override applies to.
    pub tenant_id: String,
    /// Override tool policy.
    pub tool_policy: Option<RoleToolPolicy>,
    /// Override memory policy.
    pub memory_policy: Option<RoleMemoryPolicy>,
    /// Override learning policy.
    pub learning_policy: Option<RoleLearningPolicy>,
    /// Override provider preferences.
    pub provider_preferences: Option<RoleProviderPreferences>,
    /// Override trust level.
    pub trust_level: Option<swarm_core::TrustLevel>,
    /// Additional denied tools (additive restriction).
    pub additional_denied_tools: Vec<String>,
    /// Additional denied learning categories (additive restriction).
    pub additional_denied_categories: Vec<String>,
    /// Custom metadata overrides.
    pub custom_metadata: HashMap<String, serde_json::Value>,
}

// ────────────────────────────────────────────────────────────────────────────────
// Effective role profile — resolved runtime state
// ────────────────────────────────────────────────────────────────────────────────

/// The fully resolved runtime profile for a role, after applying tenant
/// overrides and policy restrictions.
///
/// This is what the orchestrator, scheduler, and runtime actually use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveRoleProfile {
    /// The base role spec (for reference).
    pub role_id: RoleId,
    /// Resolved role name.
    pub name: String,
    /// Resolved department.
    pub department: DepartmentCategory,
    /// Agent kind tier.
    pub agent_kind: swarm_core::AgentKind,
    /// Resolved trust level.
    pub trust_level: swarm_core::TrustLevel,
    /// Resolved tool policy.
    pub tool_policy: RoleToolPolicy,
    /// Resolved memory policy.
    pub memory_policy: RoleMemoryPolicy,
    /// Resolved learning policy.
    pub learning_policy: RoleLearningPolicy,
    /// Resolved provider preferences.
    pub provider_preferences: RoleProviderPreferences,
    /// Resolved escalation policy.
    pub escalation: EscalationPolicy,
    /// Resolved personality spec.
    pub personality: RolePersonalitySpec,
    /// Resolved prompt template.
    pub prompt_template: PromptTemplate,
    /// Required capabilities.
    pub required_capabilities: Vec<String>,
    /// Supervisor role name.
    pub supervisor: Option<String>,
    /// Subordinate role names.
    pub subordinates: Vec<String>,
    /// Collaboration rules.
    pub collaboration_rules: Vec<RoleCollaborationRule>,
    /// Tenant that this profile was resolved for (if any).
    pub tenant_id: Option<String>,
    /// Timestamp of resolution.
    pub resolved_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_id_from_name_is_deterministic() {
        let a = RoleId::from_name("CEO Agent");
        let b = RoleId::from_name("CEO Agent");
        assert_eq!(a, b);
    }

    #[test]
    fn role_id_from_different_names_differ() {
        let a = RoleId::from_name("CEO Agent");
        let b = RoleId::from_name("CFO Agent");
        assert_ne!(a, b);
    }

    #[test]
    fn department_from_dir_prefix() {
        assert_eq!(
            DepartmentCategory::from_dir_prefix("00_GOVERNANCE"),
            DepartmentCategory::Governance
        );
        assert_eq!(
            DepartmentCategory::from_dir_prefix("03_CUSTOMER"),
            DepartmentCategory::Customer
        );
        assert_eq!(
            DepartmentCategory::from_dir_prefix("99_UNKNOWN"),
            DepartmentCategory::Custom("99_UNKNOWN".to_string())
        );
    }

    #[test]
    fn role_spec_new_uses_deterministic_id() {
        let spec = RoleSpec::new("Test Role", DepartmentCategory::Governance);
        assert_eq!(spec.id, RoleId::from_name("Test Role"));
        assert_eq!(spec.name, "Test Role");
    }
}
