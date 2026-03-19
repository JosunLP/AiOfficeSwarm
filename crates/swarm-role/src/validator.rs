//! Role validation.
//!
//! Validates [`RoleSpec`] instances against structural and semantic rules.
//! Validation issues are collected (not short-circuited) so that callers
//! receive a complete diagnostic report.

use crate::model::RoleSpec;

/// Severity level of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ValidationSeverity {
    /// Informational note; does not block loading.
    Info,
    /// A potential problem that should be reviewed.
    Warning,
    /// A critical issue that prevents the role from being used.
    Error,
}

use serde::{Deserialize, Serialize};

/// A single validation issue found in a role specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// Which field or aspect is affected.
    pub field: String,
    /// Severity of the issue.
    pub severity: ValidationSeverity,
    /// Human-readable description of the issue.
    pub message: String,
}

/// Validates [`RoleSpec`] instances.
pub struct RoleValidator;

impl RoleValidator {
    /// Validate a role specification and return all issues found.
    ///
    /// An empty result means the specification is valid.
    pub fn validate(spec: &RoleSpec) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // ── Required fields ─────────────────────────────────────────────
        if spec.name.trim().is_empty() {
            issues.push(ValidationIssue {
                field: "name".into(),
                severity: ValidationSeverity::Error,
                message: "role name must not be empty".into(),
            });
        }

        if spec.mission.trim().is_empty() {
            issues.push(ValidationIssue {
                field: "mission".into(),
                severity: ValidationSeverity::Warning,
                message: "role has no mission statement".into(),
            });
        }

        if spec.description.trim().is_empty() {
            issues.push(ValidationIssue {
                field: "description".into(),
                severity: ValidationSeverity::Warning,
                message: "role has no description / profile".into(),
            });
        }

        // ── Responsibilities ────────────────────────────────────────────
        if spec.responsibilities.is_empty() {
            issues.push(ValidationIssue {
                field: "responsibilities".into(),
                severity: ValidationSeverity::Warning,
                message: "role has no responsibilities defined".into(),
            });
        }

        if spec.decision_rights.is_empty() {
            issues.push(ValidationIssue {
                field: "decision_rights".into(),
                severity: ValidationSeverity::Info,
                message: "role has no explicit decision rights".into(),
            });
        }

        // ── KPIs ────────────────────────────────────────────────────────
        if spec.kpis.is_empty() {
            issues.push(ValidationIssue {
                field: "kpis".into(),
                severity: ValidationSeverity::Info,
                message: "role has no KPIs defined".into(),
            });
        }

        // ── Personality ─────────────────────────────────────────────────
        if spec.personality.traits.is_empty() {
            issues.push(ValidationIssue {
                field: "personality.traits".into(),
                severity: ValidationSeverity::Warning,
                message: "role has no personality traits".into(),
            });
        }

        // ── Interfaces ──────────────────────────────────────────────────
        if spec.interfaces.is_empty() {
            issues.push(ValidationIssue {
                field: "interfaces".into(),
                severity: ValidationSeverity::Info,
                message: "role has no collaboration interfaces".into(),
            });
        }

        // ── Version format ──────────────────────────────────────────────
        if !Self::is_valid_semver(&spec.version) {
            issues.push(ValidationIssue {
                field: "version".into(),
                severity: ValidationSeverity::Warning,
                message: format!("version '{}' is not a valid semver string", spec.version),
            });
        }

        // ── Escalation ──────────────────────────────────────────────────
        if spec.escalation.triggers.is_empty() {
            issues.push(ValidationIssue {
                field: "escalation".into(),
                severity: ValidationSeverity::Info,
                message: "role has no escalation triggers defined".into(),
            });
        }

        // ── Prompt template ─────────────────────────────────────────────
        if spec.prompt_template.system_preamble.is_empty() {
            issues.push(ValidationIssue {
                field: "prompt_template".into(),
                severity: ValidationSeverity::Info,
                message: "role has no prompt template".into(),
            });
        }

        // ── Trust level vs. capabilities consistency ────────────────────
        if spec.trust_level == swarm_core::TrustLevel::Full
            && spec.agent_kind != swarm_core::AgentKind::Executive
        {
            issues.push(ValidationIssue {
                field: "trust_level".into(),
                severity: ValidationSeverity::Warning,
                message: "Full trust is typically reserved for Executive-tier agents".into(),
            });
        }

        issues
    }

    /// Check whether the set of issues contains any errors.
    pub fn has_errors(issues: &[ValidationIssue]) -> bool {
        issues
            .iter()
            .any(|i| i.severity == ValidationSeverity::Error)
    }

    /// Simple semver check (major.minor.patch).
    fn is_valid_semver(v: &str) -> bool {
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() != 3 {
            return false;
        }
        parts.iter().all(|p| p.parse::<u64>().is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DepartmentCategory, RoleSpec};

    #[test]
    fn valid_spec_has_no_errors() {
        let mut spec = RoleSpec::new("CEO Agent", DepartmentCategory::Governance);
        spec.mission = "Lead the company".into();
        spec.description = "The top executive".into();
        spec.responsibilities = vec!["strategy".into()];
        spec.personality.traits = vec!["decisive".into()];

        let issues = RoleValidator::validate(&spec);
        assert!(!RoleValidator::has_errors(&issues));
    }

    #[test]
    fn empty_name_is_error() {
        let spec = RoleSpec::new("", DepartmentCategory::Governance);
        let issues = RoleValidator::validate(&spec);
        assert!(RoleValidator::has_errors(&issues));
    }

    #[test]
    fn missing_mission_is_warning() {
        let spec = RoleSpec::new("Test Role", DepartmentCategory::Custom("test".into()));
        let issues = RoleValidator::validate(&spec);
        let mission_issues: Vec<_> = issues.iter().filter(|i| i.field == "mission").collect();
        assert!(!mission_issues.is_empty());
        assert_eq!(mission_issues[0].severity, ValidationSeverity::Warning);
    }
}
