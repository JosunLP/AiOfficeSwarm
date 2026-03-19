//! Role resolver.
//!
//! Resolves a [`RoleSpec`] into an [`EffectiveRoleProfile`] by applying
//! tenant-specific overrides and enforcing the most-restrictive-wins policy.

use chrono::Utc;

use crate::model::*;

/// Resolves effective runtime profiles from role specifications.
///
/// The resolver applies tenant overrides on top of the base role spec and
/// enforces the conflict resolution rule: **more restrictive wins**.
pub struct RoleResolver;

impl RoleResolver {
    /// Resolve an effective runtime profile for a role.
    ///
    /// If `tenant_override` is `None`, the base spec is used directly.
    /// When overrides are present, they are merged with the
    /// most-restrictive-wins policy.
    pub fn resolve(
        spec: &RoleSpec,
        tenant_override: Option<&TenantRoleOverride>,
    ) -> EffectiveRoleProfile {
        let mut tool_policy = spec.tool_policy.clone();
        let mut memory_policy = spec.memory_policy.clone();
        let mut learning_policy = spec.learning_policy.clone();
        let mut provider_prefs = spec.provider_preferences.clone();
        let mut trust_level = spec.trust_level;
        let mut tenant_id = None;

        if let Some(ov) = tenant_override {
            tenant_id = Some(ov.tenant_id.clone());

            // Tool policy: merge with most-restrictive-wins.
            if let Some(ref tp) = ov.tool_policy {
                tool_policy = Self::merge_tool_policy(&tool_policy, tp);
            }
            // Add additional denied tools (always additive restriction).
            for denied in &ov.additional_denied_tools {
                if !tool_policy.denied_tools.contains(denied) {
                    tool_policy.denied_tools.push(denied.clone());
                }
            }

            // Memory policy: override if provided, then restrict.
            if let Some(ref mp) = ov.memory_policy {
                memory_policy = Self::merge_memory_policy(&memory_policy, mp);
            }

            // Learning policy: override if provided, then restrict.
            if let Some(ref lp) = ov.learning_policy {
                learning_policy = Self::merge_learning_policy(&learning_policy, lp);
            }
            for denied in &ov.additional_denied_categories {
                if !learning_policy.denied_categories.contains(denied) {
                    learning_policy.denied_categories.push(denied.clone());
                }
            }

            // Provider preferences: override wins.
            if let Some(ref pp) = ov.provider_preferences {
                provider_prefs = pp.clone();
            }

            // Trust level: most restrictive (lowest) wins.
            if let Some(ov_trust) = ov.trust_level {
                if ov_trust < trust_level {
                    trust_level = ov_trust;
                }
            }
        }

        EffectiveRoleProfile {
            role_id: spec.id,
            name: spec.name.clone(),
            department: spec.department.clone(),
            agent_kind: spec.agent_kind,
            trust_level,
            tool_policy,
            memory_policy,
            learning_policy,
            provider_preferences: provider_prefs,
            escalation: spec.escalation.clone(),
            personality: spec.personality.clone(),
            prompt_template: spec.prompt_template.clone(),
            required_capabilities: spec.required_capabilities.clone(),
            supervisor: spec.supervisor.clone(),
            subordinates: spec.subordinates.clone(),
            collaboration_rules: spec.collaboration_rules.clone(),
            tenant_id,
            resolved_at: Utc::now(),
        }
    }

    /// Merge two tool policies with most-restrictive-wins semantics.
    fn merge_tool_policy(base: &RoleToolPolicy, overlay: &RoleToolPolicy) -> RoleToolPolicy {
        // Denied tools: union of both sets.
        let mut denied = base.denied_tools.clone();
        for d in &overlay.denied_tools {
            if !denied.contains(d) {
                denied.push(d.clone());
            }
        }

        // Allowed tools: intersection (if both have lists) or the more restrictive.
        let allowed = if overlay.allowed_tools.is_empty() {
            base.allowed_tools.clone()
        } else if base.allowed_tools.is_empty() {
            overlay.allowed_tools.clone()
        } else {
            // Intersection of both allow lists.
            base.allowed_tools
                .iter()
                .filter(|t| overlay.allowed_tools.contains(t))
                .cloned()
                .collect()
        };

        // Max calls: minimum of both.
        let max_calls = match (
            base.max_tool_calls_per_task,
            overlay.max_tool_calls_per_task,
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        RoleToolPolicy {
            allowed_tools: allowed,
            denied_tools: denied,
            required_plugin_capabilities: {
                let mut caps = base.required_plugin_capabilities.clone();
                for c in &overlay.required_plugin_capabilities {
                    if !caps.contains(c) {
                        caps.push(c.clone());
                    }
                }
                caps
            },
            max_tool_calls_per_task: max_calls,
        }
    }

    /// Merge two memory policies with most-restrictive-wins semantics.
    fn merge_memory_policy(
        base: &RoleMemoryPolicy,
        overlay: &RoleMemoryPolicy,
    ) -> RoleMemoryPolicy {
        // Readable scopes: intersection.
        let readable = if overlay.readable_scopes.is_empty() {
            base.readable_scopes.clone()
        } else if base.readable_scopes.is_empty() {
            overlay.readable_scopes.clone()
        } else {
            base.readable_scopes
                .iter()
                .filter(|s| overlay.readable_scopes.contains(s))
                .cloned()
                .collect()
        };

        // Writable scopes: intersection.
        let writable = if overlay.writable_scopes.is_empty() {
            base.writable_scopes.clone()
        } else if base.writable_scopes.is_empty() {
            overlay.writable_scopes.clone()
        } else {
            base.writable_scopes
                .iter()
                .filter(|s| overlay.writable_scopes.contains(s))
                .cloned()
                .collect()
        };

        // Sensitivity: more restrictive (lower) wins.
        let sensitivity = Self::more_restrictive_sensitivity(
            base.max_sensitivity.as_deref(),
            overlay.max_sensitivity.as_deref(),
        );

        RoleMemoryPolicy {
            readable_scopes: readable,
            writable_scopes: writable,
            max_sensitivity: sensitivity.map(|s| s.to_string()),
            retention_hint: overlay
                .retention_hint
                .clone()
                .or_else(|| base.retention_hint.clone()),
        }
    }

    /// Merge two learning policies with most-restrictive-wins semantics.
    fn merge_learning_policy(
        base: &RoleLearningPolicy,
        overlay: &RoleLearningPolicy,
    ) -> RoleLearningPolicy {
        RoleLearningPolicy {
            // If either disables, result is disabled.
            enabled: base.enabled && overlay.enabled,
            // If either requires approval, result requires approval.
            require_approval: base.require_approval || overlay.require_approval,
            // Allowed categories: intersection.
            allowed_categories: if overlay.allowed_categories.is_empty() {
                base.allowed_categories.clone()
            } else if base.allowed_categories.is_empty() {
                overlay.allowed_categories.clone()
            } else {
                base.allowed_categories
                    .iter()
                    .filter(|c| overlay.allowed_categories.contains(c))
                    .cloned()
                    .collect()
            },
            // Denied categories: union.
            denied_categories: {
                let mut denied = base.denied_categories.clone();
                for d in &overlay.denied_categories {
                    if !denied.contains(d) {
                        denied.push(d.clone());
                    }
                }
                denied
            },
        }
    }

    /// Return the more restrictive sensitivity level string.
    fn more_restrictive_sensitivity<'a>(a: Option<&'a str>, b: Option<&'a str>) -> Option<&'a str> {
        match (a, b) {
            (None, x) | (x, None) => x,
            (Some(a_val), Some(b_val)) => {
                let rank = |s: &str| -> u8 {
                    match s {
                        "public" => 0,
                        "internal" => 1,
                        "confidential" => 2,
                        "restricted" => 3,
                        _ => 1, // Unknown defaults to internal.
                    }
                };
                if rank(a_val) <= rank(b_val) {
                    Some(a_val)
                } else {
                    Some(b_val)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_spec() -> RoleSpec {
        let mut spec = RoleSpec::new("Test Role", DepartmentCategory::Governance);
        spec.tool_policy.allowed_tools = vec!["tool_a".into(), "tool_b".into(), "tool_c".into()];
        spec.tool_policy.denied_tools = vec!["tool_x".into()];
        spec.memory_policy.readable_scopes = vec!["own".into(), "shared".into()];
        spec.memory_policy.writable_scopes = vec!["own".into()];
        spec.memory_policy.max_sensitivity = Some("confidential".into());
        spec.learning_policy.enabled = true;
        spec.learning_policy.require_approval = false;
        spec.learning_policy.allowed_categories = vec!["a".into(), "b".into(), "c".into()];
        spec.trust_level = swarm_core::TrustLevel::High;
        spec
    }

    #[test]
    fn resolve_without_override_returns_base() {
        let spec = base_spec();
        let profile = RoleResolver::resolve(&spec, None);
        assert_eq!(profile.trust_level, swarm_core::TrustLevel::High);
        assert_eq!(profile.tool_policy.allowed_tools.len(), 3);
        assert!(profile.tenant_id.is_none());
    }

    #[test]
    fn override_restricts_trust_level() {
        let spec = base_spec();
        let ov = TenantRoleOverride {
            tenant_id: "tenant-1".into(),
            trust_level: Some(swarm_core::TrustLevel::Low),
            ..Default::default()
        };
        let profile = RoleResolver::resolve(&spec, Some(&ov));
        assert_eq!(profile.trust_level, swarm_core::TrustLevel::Low);
        assert_eq!(profile.tenant_id.as_deref(), Some("tenant-1"));
    }

    #[test]
    fn override_adds_denied_tools() {
        let spec = base_spec();
        let ov = TenantRoleOverride {
            tenant_id: "t".into(),
            additional_denied_tools: vec!["tool_a".into(), "tool_y".into()],
            ..Default::default()
        };
        let profile = RoleResolver::resolve(&spec, Some(&ov));
        assert!(profile
            .tool_policy
            .denied_tools
            .contains(&"tool_x".to_string()));
        assert!(profile
            .tool_policy
            .denied_tools
            .contains(&"tool_a".to_string()));
        assert!(profile
            .tool_policy
            .denied_tools
            .contains(&"tool_y".to_string()));
    }

    #[test]
    fn learning_policy_most_restrictive() {
        let spec = base_spec();
        let ov = TenantRoleOverride {
            tenant_id: "t".into(),
            learning_policy: Some(RoleLearningPolicy {
                enabled: true,
                require_approval: true,
                allowed_categories: vec!["a".into(), "d".into()],
                denied_categories: vec!["z".into()],
            }),
            ..Default::default()
        };
        let profile = RoleResolver::resolve(&spec, Some(&ov));
        // require_approval: base false || overlay true = true.
        assert!(profile.learning_policy.require_approval);
        // Allowed: intersection of {a,b,c} ∩ {a,d} = {a}.
        assert_eq!(profile.learning_policy.allowed_categories, vec!["a"]);
        // Denied: union.
        assert!(profile
            .learning_policy
            .denied_categories
            .contains(&"z".to_string()));
    }

    #[test]
    fn memory_sensitivity_most_restrictive() {
        let spec = base_spec();
        let ov = TenantRoleOverride {
            tenant_id: "t".into(),
            memory_policy: Some(RoleMemoryPolicy {
                readable_scopes: vec!["own".into()],
                writable_scopes: vec!["own".into()],
                max_sensitivity: Some("internal".into()),
                retention_hint: None,
            }),
            ..Default::default()
        };
        let profile = RoleResolver::resolve(&spec, Some(&ov));
        // Intersection of {own, shared} ∩ {own} = {own}.
        assert_eq!(profile.memory_policy.readable_scopes, vec!["own"]);
        // internal < confidential, so internal wins.
        assert_eq!(
            profile.memory_policy.max_sensitivity.as_deref(),
            Some("internal")
        );
    }
}
