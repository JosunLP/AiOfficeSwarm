//! Bridge between role personality specs and the personality subsystem.
//!
//! This module converts [`RolePersonalitySpec`] into a
//! [`PersonalityOverlay`] that can be composed on top of an agent's
//! base personality profile.

use swarm_personality::profile::{
    CollaborationPattern, CommunicationStyle, DecisionTendencies, EscalationBehavior,
    PersonalityOverlay, RiskTolerance,
};

use crate::model::RoleSpec;

/// Convert a [`RoleSpec`]'s personality section into a
/// [`PersonalityOverlay`] suitable for composing on top of a base profile.
pub fn overlay_from_role(spec: &RoleSpec) -> PersonalityOverlay {
    let ps = &spec.personality;

    // Map tone → CommunicationStyle override
    let communication_style = ps.tone.as_ref().map(|tone| CommunicationStyle {
        tone: tone.clone(),
        verbosity: infer_verbosity(spec),
        language: "en-US".into(),
        use_jargon: infer_jargon(spec),
    });

    // Map role kind → collaboration pattern
    let collaboration_pattern = Some(match spec.agent_kind {
        swarm_core::agent::AgentKind::Executive => CollaborationPattern::Directive,
        swarm_core::agent::AgentKind::Manager => CollaborationPattern::Collaborative,
        swarm_core::agent::AgentKind::Worker => CollaborationPattern::Cooperative,
    });

    // Risk tolerance from trust level
    let risk_tolerance = Some(match spec.trust_level {
        swarm_core::TrustLevel::Full => RiskTolerance::VeryHigh,
        swarm_core::TrustLevel::High => RiskTolerance::High,
        swarm_core::TrustLevel::Standard => RiskTolerance::Medium,
        swarm_core::TrustLevel::Low => RiskTolerance::Low,
        swarm_core::TrustLevel::Untrusted => RiskTolerance::VeryLow,
    });

    // Decision tendencies from personality traits
    let decision_tendencies = Some(infer_decision_tendencies(spec));

    // Escalation behavior from role's escalation policy
    let escalation_behavior = Some(escalation_to_behavior(&spec.escalation));

    // Domain hints from department
    let domain_hints = Some(vec![spec.department.label().to_string()]);

    PersonalityOverlay {
        communication_style,
        decision_tendencies,
        risk_tolerance,
        collaboration_pattern,
        escalation_behavior,
        domain_hints,
        response_formatting: None,
        custom_traits: Default::default(),
    }
}

fn infer_verbosity(spec: &RoleSpec) -> u8 {
    // Executives are concise, Workers are detailed, Managers in between.
    match spec.agent_kind {
        swarm_core::agent::AgentKind::Executive => 2,
        swarm_core::agent::AgentKind::Manager => 3,
        swarm_core::agent::AgentKind::Worker => 4,
    }
}

fn infer_jargon(spec: &RoleSpec) -> bool {
    matches!(
        spec.department,
        crate::model::DepartmentCategory::ProductTech
            | crate::model::DepartmentCategory::BackOffice
    )
}

fn infer_decision_tendencies(spec: &RoleSpec) -> DecisionTendencies {
    let traits_lower: Vec<String> = spec
        .personality
        .traits
        .iter()
        .map(|t| t.to_lowercase())
        .collect();

    let thoroughness = if traits_lower
        .iter()
        .any(|t| t.contains("analytical") || t.contains("meticulous"))
    {
        0.8
    } else if traits_lower
        .iter()
        .any(|t| t.contains("pragmatic") || t.contains("decisive"))
    {
        0.4
    } else {
        0.5
    };

    let consensus_seeking = matches!(spec.agent_kind, swarm_core::agent::AgentKind::Manager)
        || traits_lower
            .iter()
            .any(|t| t.contains("collaborative") || t.contains("empathetic"));

    let innovation_bias = if traits_lower
        .iter()
        .any(|t| t.contains("creative") || t.contains("innovative"))
    {
        0.7
    } else if traits_lower
        .iter()
        .any(|t| t.contains("conservative") || t.contains("cautious"))
    {
        0.2
    } else {
        0.4
    };

    DecisionTendencies {
        thoroughness,
        consensus_seeking,
        innovation_bias,
    }
}

fn escalation_to_behavior(esc: &crate::model::EscalationPolicy) -> EscalationBehavior {
    let trigger = if esc.max_retries_before_escalation.unwrap_or(0) > 0 {
        "after_retry".into()
    } else {
        "immediate".into()
    };

    EscalationBehavior {
        trigger,
        max_retries_before_escalation: esc.max_retries_before_escalation.unwrap_or(2),
        include_full_context: esc.include_full_context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DepartmentCategory, RolePersonalitySpec, RoleSpec};

    fn make_ceo_spec() -> RoleSpec {
        let mut spec = RoleSpec::new("CEO Agent", DepartmentCategory::Governance);
        spec.agent_kind = swarm_core::AgentKind::Executive;
        spec.trust_level = swarm_core::TrustLevel::Full;
        spec.personality = RolePersonalitySpec {
            traits: vec!["decisive".into(), "pragmatic".into(), "calm".into()],
            working_principles: vec!["Think before acting".into()],
            thinking_model: vec!["First principles".into()],
            core_questions: vec!["What drives value?".into()],
            tone: Some("formal".into()),
        };
        spec
    }

    #[test]
    fn overlay_from_ceo_role() {
        let spec = make_ceo_spec();
        let overlay = overlay_from_role(&spec);

        assert_eq!(overlay.risk_tolerance, Some(RiskTolerance::VeryHigh));
        assert_eq!(
            overlay.collaboration_pattern,
            Some(CollaborationPattern::Directive)
        );
        let style = overlay.communication_style.unwrap();
        assert_eq!(style.tone, "formal");
        assert_eq!(style.verbosity, 2); // Executive is concise
    }
}
