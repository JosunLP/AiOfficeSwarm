//! Role-to-policy binding.
//!
//! Maps role specifications and effective profiles into the policy, memory,
//! learning, personality, and provider subsystems of the framework.

use swarm_core::agent::{
    LearningPolicyRef, MemoryAccessProfileRef, OperationalConstraints, ProviderPreferences,
    ToolPermissions,
};
use swarm_core::capability::{Capability, CapabilitySet};

use crate::model::{EffectiveRoleProfile, RolePersonalitySpec, RoleSpec};

/// Provides mappings from role constructs to framework-level policy types.
pub struct RolePolicyBinding;

impl RolePolicyBinding {
    /// Convert an effective role profile into `ToolPermissions` for an agent descriptor.
    pub fn to_tool_permissions(profile: &EffectiveRoleProfile) -> ToolPermissions {
        ToolPermissions {
            allowed_tools: profile.tool_policy.allowed_tools.clone(),
            denied_tools: profile.tool_policy.denied_tools.clone(),
            max_tool_calls_per_task: profile.tool_policy.max_tool_calls_per_task,
        }
    }

    /// Convert an effective role profile into `MemoryAccessProfileRef` for an agent descriptor.
    pub fn to_memory_access_ref(profile: &EffectiveRoleProfile) -> MemoryAccessProfileRef {
        MemoryAccessProfileRef {
            readable_scopes: profile.memory_policy.readable_scopes.clone(),
            writable_scopes: profile.memory_policy.writable_scopes.clone(),
            max_sensitivity: profile.memory_policy.max_sensitivity.clone(),
        }
    }

    /// Convert an effective role profile into `LearningPolicyRef` for an agent descriptor.
    pub fn to_learning_policy_ref(profile: &EffectiveRoleProfile) -> LearningPolicyRef {
        LearningPolicyRef {
            enabled: profile.learning_policy.enabled,
            require_approval: profile.learning_policy.require_approval,
            allowed_categories: profile.learning_policy.allowed_categories.clone(),
        }
    }

    /// Convert an effective role profile into `ProviderPreferences` for an agent descriptor.
    pub fn to_provider_preferences(profile: &EffectiveRoleProfile) -> ProviderPreferences {
        ProviderPreferences {
            preferred_provider: profile.provider_preferences.preferred_provider.clone(),
            preferred_model: profile.provider_preferences.preferred_model.clone(),
            allowlist: Vec::new(),
            blocklist: Vec::new(),
        }
    }

    /// Convert role required capabilities into a `CapabilitySet`.
    pub fn to_capability_set(profile: &EffectiveRoleProfile) -> CapabilitySet {
        let mut set = CapabilitySet::new();
        for cap_name in &profile.required_capabilities {
            set.add(Capability::new(cap_name));
        }
        set
    }

    /// Generate operational constraints from a role specification.
    ///
    /// Executive roles get higher limits; workers get tighter bounds.
    pub fn to_operational_constraints(profile: &EffectiveRoleProfile) -> OperationalConstraints {
        let (max_tasks, max_tokens, max_cost) = match profile.agent_kind {
            swarm_core::AgentKind::Executive => (Some(50), Some(100_000), Some(500_000)),
            swarm_core::AgentKind::Manager => (Some(100), Some(50_000), Some(200_000)),
            swarm_core::AgentKind::Worker => (Some(200), Some(20_000), Some(100_000)),
        };

        OperationalConstraints {
            max_tasks_per_hour: max_tasks,
            max_tokens_per_task: max_tokens,
            max_cost_per_task: max_cost,
            allow_external_communication: matches!(
                profile.agent_kind,
                swarm_core::AgentKind::Executive | swarm_core::AgentKind::Manager
            ),
            custom: Default::default(),
        }
    }

    /// Build a complete system prompt from the role's prompt template and personality.
    pub fn build_system_prompt(spec: &RoleSpec) -> String {
        let mut prompt = String::new();

        // System preamble from the prompt template.
        if !spec.prompt_template.system_preamble.is_empty() {
            prompt.push_str(&spec.prompt_template.system_preamble);
            prompt.push_str("\n\n");
        }

        // Inject working principles.
        if !spec.personality.working_principles.is_empty() {
            prompt.push_str("Working principles:\n");
            for p in &spec.personality.working_principles {
                prompt.push_str("- ");
                prompt.push_str(p);
                prompt.push('\n');
            }
            prompt.push('\n');
        }

        // Inject thinking model.
        if !spec.personality.thinking_model.is_empty() {
            prompt.push_str("When analyzing a topic, consider:\n");
            for q in &spec.personality.thinking_model {
                prompt.push_str("- ");
                prompt.push_str(q);
                prompt.push('\n');
            }
            prompt.push('\n');
        }

        // Inject response structure.
        if !spec.prompt_template.response_structure.is_empty() {
            prompt.push_str("Structure your response as follows:\n");
            for (i, item) in spec.prompt_template.response_structure.iter().enumerate() {
                prompt.push_str(&format!("{}. {}\n", i + 1, item));
            }
            prompt.push('\n');
        }

        // Inject boundaries from non-responsibilities.
        if !spec.non_responsibilities.is_empty() {
            prompt.push_str("You are NOT responsible for:\n");
            for nr in &spec.non_responsibilities {
                prompt.push_str("- ");
                prompt.push_str(nr);
                prompt.push('\n');
            }
            prompt.push('\n');
        }

        prompt.trim_end().to_string()
    }

    /// Derive a personality tone string from role personality traits.
    pub fn derive_tone(personality: &RolePersonalitySpec) -> String {
        personality
            .tone
            .clone()
            .unwrap_or_else(|| "professional".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn sample_profile() -> EffectiveRoleProfile {
        let spec = RoleSpec::new("Test", DepartmentCategory::Governance);
        crate::resolver::RoleResolver::resolve(&spec, None)
    }

    #[test]
    fn to_tool_permissions_maps_correctly() {
        let mut profile = sample_profile();
        profile.tool_policy.allowed_tools = vec!["a".into()];
        profile.tool_policy.denied_tools = vec!["b".into()];
        let perms = RolePolicyBinding::to_tool_permissions(&profile);
        assert_eq!(perms.allowed_tools, vec!["a"]);
        assert_eq!(perms.denied_tools, vec!["b"]);
    }

    #[test]
    fn build_system_prompt_includes_all_sections() {
        let mut spec = RoleSpec::new("CEO Agent", DepartmentCategory::Governance);
        spec.prompt_template.system_preamble = "You are the CEO Agent.".into();
        spec.prompt_template.response_structure = vec!["situation".into(), "recommendation".into()];
        spec.personality.working_principles = vec!["clarity before activity".into()];
        spec.personality.thinking_model = vec!["What matters most?".into()];
        spec.non_responsibilities = vec!["operational details".into()];

        let prompt = RolePolicyBinding::build_system_prompt(&spec);
        assert!(prompt.contains("You are the CEO Agent."));
        assert!(prompt.contains("clarity before activity"));
        assert!(prompt.contains("What matters most?"));
        assert!(prompt.contains("1. situation"));
        assert!(prompt.contains("operational details"));
    }
}
