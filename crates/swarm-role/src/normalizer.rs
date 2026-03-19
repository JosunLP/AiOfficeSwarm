//! Role normalizer.
//!
//! Converts a [`RawRoleSource`] (unvalidated parse result) into a fully typed
//! [`RoleSpec`]. The normalizer applies heuristic mappings for sections that
//! follow the standard role file structure from the `roles/` directory.

use chrono::Utc;

use crate::error::{RoleError, RoleResult};
use crate::model::*;
use crate::parser::RoleParser;

/// Normalizes [`RawRoleSource`] into [`RoleSpec`].
pub struct RoleNormalizer;

impl RoleNormalizer {
    /// Normalize a raw role source into a typed specification.
    pub fn normalize(raw: &RawRoleSource) -> RoleResult<RoleSpec> {
        let name = raw
            .title
            .clone()
            .ok_or_else(|| RoleError::NormalizationFailed {
                name: raw.source_path.clone(),
                reason: "role file has no title (H1 heading)".to_string(),
            })?;

        let department = raw
            .department_dir
            .as_deref()
            .map(DepartmentCategory::from_dir_prefix)
            .unwrap_or(DepartmentCategory::Custom("unknown".to_string()));

        let description = raw
            .sections
            .get("profile")
            .map(|s| RoleParser::extract_paragraph(s))
            .unwrap_or_default();

        let mission = raw
            .sections
            .get("mission")
            .map(|s| RoleParser::extract_paragraph(s))
            .unwrap_or_default();

        let success_measure = raw
            .sections
            .get("success measure")
            .map(|s| RoleParser::extract_paragraph(s))
            .unwrap_or_default();

        let responsibilities = raw
            .sections
            .get("responsibilities")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let decision_rights = raw
            .sections
            .get("decision rights")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let non_responsibilities = raw
            .sections
            .get("not responsible for")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let kpis = raw
            .sections
            .get("kpis")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let main_inputs = raw
            .sections
            .get("main inputs")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let main_outputs = raw
            .sections
            .get("main outputs")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let interfaces = raw
            .sections
            .get("interfaces")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        // Build collaboration rules from interfaces list.
        let collaboration_rules = interfaces
            .iter()
            .map(|target| RoleCollaborationRule {
                target_role: target.clone(),
                relationship: "collaborates".to_string(),
                required: false,
            })
            .collect();

        // Personality extraction.
        let personality_traits = raw
            .sections
            .get("personality")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let working_principles = raw
            .sections
            .get("working principles")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let thinking_model = raw
            .sections
            .get("thinking model")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        let core_questions = raw
            .sections
            .get("core questions")
            .map(|s| RoleParser::extract_list(s))
            .unwrap_or_default();

        // Derive tone from personality traits.
        let tone = Self::infer_tone(&personality_traits);

        let personality = RolePersonalitySpec {
            traits: personality_traits,
            working_principles,
            thinking_model,
            core_questions,
            tone: Some(tone),
        };

        // Escalation extraction.
        let escalation_text = raw
            .sections
            .get("escalation")
            .map(|s| RoleParser::extract_paragraph(s))
            .unwrap_or_default();

        let escalation = EscalationPolicy {
            triggers: if escalation_text.is_empty() {
                Vec::new()
            } else {
                vec![escalation_text]
            },
            escalation_targets: Vec::new(), // Will be populated by hierarchy resolution.
            max_retries_before_escalation: Some(2),
            include_full_context: true,
        };

        // Prompt template extraction.
        let prompt_template = Self::extract_prompt_template(raw);

        // Infer agent kind from department and role name.
        let agent_kind = Self::infer_agent_kind(&name, &department);

        // Infer trust level from agent kind and department.
        let trust_level = Self::infer_trust_level(&agent_kind, &department);

        // Derive required capabilities from responsibilities and role name.
        let required_capabilities = Self::derive_capabilities(&name, &responsibilities);

        // Default memory policy: role-scoped read/write, team read.
        let memory_policy = Self::default_memory_policy(&department);

        // Default learning policy based on department.
        let learning_policy = Self::default_learning_policy(&department);

        let spec = RoleSpec {
            id: RoleId::from_name(&name),
            name,
            version: "1.0.0".to_string(),
            department,
            description,
            mission,
            success_measure,
            responsibilities,
            decision_rights,
            non_responsibilities,
            required_capabilities,
            kpis,
            main_inputs,
            main_outputs,
            interfaces,
            collaboration_rules,
            escalation,
            supervisor: None,         // Populated by hierarchy resolution.
            subordinates: Vec::new(), // Populated by hierarchy resolution.
            personality,
            prompt_template,
            tool_policy: RoleToolPolicy::default(),
            memory_policy,
            learning_policy,
            provider_preferences: RoleProviderPreferences::default(),
            agent_kind,
            trust_level,
            metadata: RoleMetadata {
                tags: Vec::new(),
                custom: Default::default(),
                source_path: Some(raw.source_path.clone()),
                source_hash: None,
            },
            updated_at: Utc::now(),
        };

        Ok(spec)
    }

    /// Infer a communication tone from personality trait adjectives.
    fn infer_tone(traits: &[String]) -> String {
        // Map known trait clusters to tones.
        let trait_set: Vec<&str> = traits.iter().map(|s| s.as_str()).collect();

        if trait_set
            .iter()
            .any(|t| ["analytical", "sober", "precise", "rational"].contains(t))
        {
            return "analytical".to_string();
        }
        if trait_set
            .iter()
            .any(|t| ["creative", "curious", "experimental"].contains(t))
        {
            return "creative".to_string();
        }
        if trait_set
            .iter()
            .any(|t| ["empathetic", "patient", "friendly"].contains(t))
        {
            return "empathetic".to_string();
        }
        if trait_set
            .iter()
            .any(|t| ["decisive", "clear", "direct"].contains(t))
        {
            return "decisive".to_string();
        }
        if trait_set
            .iter()
            .any(|t| ["pragmatic", "disciplined", "resilient"].contains(t))
        {
            return "pragmatic".to_string();
        }
        if trait_set
            .iter()
            .any(|t| ["vigilant", "controlled", "cautious"].contains(t))
        {
            return "cautious".to_string();
        }
        "professional".to_string()
    }

    /// Infer the agent kind (Executive, Manager, Worker) from the role name and department.
    fn infer_agent_kind(name: &str, department: &DepartmentCategory) -> swarm_core::AgentKind {
        let name_lower = name.to_lowercase();

        // Executive-level indicators.
        if name_lower.contains("ceo")
            || name_lower.contains("cfo")
            || name_lower.contains("coo")
            || name_lower.contains("cto")
            || name_lower.contains("chief")
        {
            return swarm_core::AgentKind::Executive;
        }

        // Manager-level indicators based on department heads.
        if matches!(department, DepartmentCategory::Governance)
            && (name_lower.contains("legal") || name_lower.contains("compliance"))
        {
            return swarm_core::AgentKind::Manager;
        }

        // Department heads are typically managers.
        if name_lower.contains("product agent")
            || name_lower.contains("marketing agent")
            || name_lower.contains("sales agent")
            || name_lower.contains("customer success")
            || name_lower.contains("people & culture")
            || name_lower.contains("people &amp; culture")
        {
            return swarm_core::AgentKind::Manager;
        }

        // Everyone else is a worker.
        swarm_core::AgentKind::Worker
    }

    /// Infer trust level from agent kind and department.
    fn infer_trust_level(
        kind: &swarm_core::AgentKind,
        department: &DepartmentCategory,
    ) -> swarm_core::TrustLevel {
        match kind {
            swarm_core::AgentKind::Executive => swarm_core::TrustLevel::High,
            swarm_core::AgentKind::Manager => {
                if matches!(department, DepartmentCategory::Governance) {
                    swarm_core::TrustLevel::High
                } else {
                    swarm_core::TrustLevel::Standard
                }
            }
            swarm_core::AgentKind::Worker => swarm_core::TrustLevel::Standard,
        }
    }

    /// Derive required capabilities from the role name and responsibilities.
    fn derive_capabilities(name: &str, responsibilities: &[String]) -> Vec<String> {
        let mut caps = Vec::new();
        let name_lower = name.to_lowercase();
        let all_text: String = responsibilities
            .iter()
            .map(|r| r.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");

        // Common capability hints.
        if all_text.contains("strategy") || all_text.contains("prioritization") {
            caps.push("strategic-planning".to_string());
        }
        if all_text.contains("budget")
            || all_text.contains("financial")
            || all_text.contains("cost")
        {
            caps.push("financial-analysis".to_string());
        }
        if all_text.contains("contract")
            || all_text.contains("compliance")
            || all_text.contains("legal")
        {
            caps.push("legal-review".to_string());
        }
        if all_text.contains("architecture")
            || all_text.contains("technical")
            || all_text.contains("code")
        {
            caps.push("technical-analysis".to_string());
        }
        if all_text.contains("design") || all_text.contains("ux") || all_text.contains("user") {
            caps.push("ux-design".to_string());
        }
        if all_text.contains("data")
            || all_text.contains("analytics")
            || all_text.contains("reporting")
        {
            caps.push("data-analysis".to_string());
        }
        if all_text.contains("marketing")
            || all_text.contains("messaging")
            || all_text.contains("campaign")
        {
            caps.push("marketing".to_string());
        }
        if all_text.contains("sales") || all_text.contains("deal") || all_text.contains("revenue") {
            caps.push("sales".to_string());
        }
        if all_text.contains("support")
            || all_text.contains("ticket")
            || all_text.contains("incident")
        {
            caps.push("customer-support".to_string());
        }
        if all_text.contains("hiring")
            || all_text.contains("recruiting")
            || all_text.contains("talent")
        {
            caps.push("talent-acquisition".to_string());
        }
        if all_text.contains("learning")
            || all_text.contains("training")
            || all_text.contains("enablement")
        {
            caps.push("learning-enablement".to_string());
        }
        if all_text.contains("security")
            || all_text.contains("privacy")
            || all_text.contains("threat")
        {
            caps.push("security-assessment".to_string());
        }
        if all_text.contains("process")
            || all_text.contains("operational")
            || all_text.contains("delivery")
        {
            caps.push("operations".to_string());
        }
        if all_text.contains("procurement")
            || all_text.contains("vendor")
            || all_text.contains("negotiation")
        {
            caps.push("procurement".to_string());
        }

        // Every role gets text generation as a baseline.
        caps.push("text-generation".to_string());

        // Add a role-derived capability.
        let role_cap = name_lower
            .replace(" / ", "-")
            .replace("/ ", "-")
            .replace(' ', "-");
        caps.push(format!("role:{}", role_cap));

        caps.sort();
        caps.dedup();
        caps
    }

    /// Extract prompt template from raw sections.
    fn extract_prompt_template(raw: &RawRoleSource) -> PromptTemplate {
        raw.sections
            .get("prompt template")
            .map(|body| {
                let lines: Vec<&str> = body.lines().collect();
                let mut preamble_lines = Vec::new();
                let mut structure_items = Vec::new();
                let mut in_structure = false;

                for line in &lines {
                    let trimmed = line.trim();
                    if trimmed.to_lowercase().starts_with("structure:")
                        || trimmed.to_lowercase().starts_with("deliver:")
                        || trimmed
                            .to_lowercase()
                            .starts_with("respond in this structure:")
                    {
                        in_structure = true;
                        continue;
                    }
                    if in_structure {
                        let items = RoleParser::extract_numbered_list(&format!("{}\n", trimmed));
                        if !items.is_empty() {
                            structure_items.extend(items);
                        }
                    } else if !trimmed.is_empty() {
                        preamble_lines.push(trimmed.to_string());
                    }
                }

                PromptTemplate {
                    system_preamble: preamble_lines.join(" "),
                    response_structure: structure_items,
                }
            })
            .unwrap_or_default()
    }

    /// Default memory policy for a department.
    fn default_memory_policy(department: &DepartmentCategory) -> RoleMemoryPolicy {
        let dept_scope = format!("department:{}", department.label());
        RoleMemoryPolicy {
            readable_scopes: vec!["own".to_string(), dept_scope.clone(), "shared".to_string()],
            writable_scopes: vec!["own".to_string(), dept_scope],
            max_sensitivity: match department {
                DepartmentCategory::Governance => Some("confidential".to_string()),
                DepartmentCategory::ProductTech => Some("internal".to_string()),
                _ => Some("internal".to_string()),
            },
            retention_hint: Some("persistent".to_string()),
        }
    }

    /// Default learning policy for a department.
    fn default_learning_policy(department: &DepartmentCategory) -> RoleLearningPolicy {
        match department {
            DepartmentCategory::Governance => RoleLearningPolicy {
                enabled: true,
                require_approval: true,
                allowed_categories: vec![
                    "preference_adaptation".to_string(),
                    "pattern_extraction".to_string(),
                ],
                denied_categories: vec!["configuration_evolution".to_string()],
            },
            _ => RoleLearningPolicy {
                enabled: true,
                require_approval: false,
                allowed_categories: vec![
                    "preference_adaptation".to_string(),
                    "pattern_extraction".to_string(),
                    "feedback_incorporation".to_string(),
                    "knowledge_accumulation".to_string(),
                ],
                denied_categories: Vec::new(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raw_ceo() -> RawRoleSource {
        let mut sections = std::collections::HashMap::new();
        sections.insert(
            "profile".to_string(),
            "The CEO Agent holds the overall company perspective.".to_string(),
        );
        sections.insert("mission".to_string(), "Set a clear direction.".to_string());
        sections.insert(
            "responsibilities".to_string(),
            "- company strategy\n- capital allocation\n- executive alignment".to_string(),
        );
        sections.insert(
            "personality".to_string(),
            "- clear\n- decisive\n- calm".to_string(),
        );
        sections.insert(
            "kpis".to_string(),
            "- revenue growth\n- profitability".to_string(),
        );
        sections.insert(
            "interfaces".to_string(),
            "- COO\n- CFO\n- Product".to_string(),
        );
        sections.insert(
            "escalation".to_string(),
            "When critical strategic conflicts arise.".to_string(),
        );
        sections.insert(
            "success measure".to_string(),
            "Success exists when the company stays focused.".to_string(),
        );

        RawRoleSource {
            source_path: "00_GOVERNANCE/CEO_Agent.md".to_string(),
            department_dir: Some("00_GOVERNANCE".to_string()),
            title: Some("CEO Agent".to_string()),
            sections,
        }
    }

    #[test]
    fn normalizes_ceo_role() {
        let raw = make_raw_ceo();
        let spec = RoleNormalizer::normalize(&raw).unwrap();

        assert_eq!(spec.name, "CEO Agent");
        assert_eq!(spec.department, DepartmentCategory::Governance);
        assert_eq!(spec.agent_kind, swarm_core::AgentKind::Executive);
        assert_eq!(spec.trust_level, swarm_core::TrustLevel::High);
        assert_eq!(spec.responsibilities.len(), 3);
        assert_eq!(spec.interfaces, vec!["COO", "CFO", "Product"]);
        assert!(!spec.personality.traits.is_empty());
    }

    #[test]
    fn infers_worker_for_support_role() {
        let mut raw = make_raw_ceo();
        raw.title = Some("Support Agent".to_string());
        raw.department_dir = Some("03_CUSTOMER".to_string());
        let spec = RoleNormalizer::normalize(&raw).unwrap();
        assert_eq!(spec.agent_kind, swarm_core::AgentKind::Worker);
    }
}
