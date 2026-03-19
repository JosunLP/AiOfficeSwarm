//! Role hierarchy (organigram) management.
//!
//! Models the supervision tree defined in `ORGANIGRAM.md` and resolves
//! parent/child relationships between roles.

use std::collections::HashMap;

use crate::error::{RoleError, RoleResult};
use crate::model::RoleSpec;

/// An edge in the role hierarchy (supervisor → subordinate).
#[derive(Debug, Clone)]
pub struct HierarchyEdge {
    /// The supervisor role name.
    pub supervisor: String,
    /// The subordinate role name.
    pub subordinate: String,
}

/// Models the organizational hierarchy (organigram) of roles.
///
/// This is a static structure loaded once from the organigram definition.
/// It is used to populate the `supervisor` and `subordinates` fields on
/// [`RoleSpec`] and to resolve escalation targets.
pub struct RoleHierarchy {
    /// Edges in the hierarchy.
    edges: Vec<HierarchyEdge>,
    /// Map from role name (lowercased) → supervisor name.
    supervisor_map: HashMap<String, String>,
    /// Map from role name (lowercased) → list of subordinate names.
    subordinate_map: HashMap<String, Vec<String>>,
}

impl RoleHierarchy {
    /// Build the default hierarchy from the known organigram.
    ///
    /// This encodes the structure from `roles/ORGANIGRAM.md`.
    pub fn from_default_organigram() -> Self {
        let edges = vec![
            // CEO direct reports
            ("CEO Agent", "Chief of Staff Agent"),
            ("CEO Agent", "COO Agent"),
            ("CEO Agent", "CFO Agent"),
            ("CEO Agent", "Product Agent"),
            ("CEO Agent", "Marketing Agent"),
            ("CEO Agent", "Sales Agent"),
            ("CEO Agent", "Customer Success Agent"),
            ("CEO Agent", "People & Culture Agent"),
            // COO subordinates
            ("COO Agent", "Delivery Agent"),
            ("COO Agent", "Support Agent"),
            ("COO Agent", "Procurement & Vendor Agent"),
            ("COO Agent", "Internal IT Agent"),
            // CFO subordinates
            ("CFO Agent", "Legal & Compliance Agent"),
            // Product subordinates
            ("Product Agent", "UX / Design Agent"),
            ("Product Agent", "Data & Analytics Agent"),
            ("Product Agent", "CTO / Engineering Agent"),
            // CTO subordinates
            ("CTO / Engineering Agent", "Security & Privacy Agent"),
            // Marketing subordinates
            ("Marketing Agent", "Growth / Performance Agent"),
            // Sales subordinates
            ("Sales Agent", "Partnerships / BizDev Agent"),
            // People subordinates
            ("People & Culture Agent", "Talent Acquisition Agent"),
            ("People & Culture Agent", "Learning & Enablement Agent"),
        ];

        Self::from_edges(
            edges
                .into_iter()
                .map(|(sup, sub)| HierarchyEdge {
                    supervisor: sup.to_string(),
                    subordinate: sub.to_string(),
                })
                .collect(),
        )
    }

    /// Build a hierarchy from a list of edges.
    pub fn from_edges(edges: Vec<HierarchyEdge>) -> Self {
        let mut supervisor_map = HashMap::new();
        let mut subordinate_map: HashMap<String, Vec<String>> = HashMap::new();

        for edge in &edges {
            supervisor_map.insert(edge.subordinate.to_lowercase(), edge.supervisor.clone());
            subordinate_map
                .entry(edge.supervisor.to_lowercase())
                .or_default()
                .push(edge.subordinate.clone());
        }

        Self {
            edges,
            supervisor_map,
            subordinate_map,
        }
    }

    /// Get the supervisor name for a role.
    pub fn supervisor_of(&self, role_name: &str) -> Option<&str> {
        self.supervisor_map
            .get(&role_name.to_lowercase())
            .map(|s| s.as_str())
    }

    /// Get the subordinate names for a role.
    pub fn subordinates_of(&self, role_name: &str) -> Vec<&str> {
        self.subordinate_map
            .get(&role_name.to_lowercase())
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Check if a role is the root of the hierarchy (no supervisor).
    pub fn is_root(&self, role_name: &str) -> bool {
        !self.supervisor_map.contains_key(&role_name.to_lowercase())
    }

    /// Get all edges.
    pub fn edges(&self) -> &[HierarchyEdge] {
        &self.edges
    }

    /// Apply the hierarchy to a set of role specifications, populating
    /// their `supervisor` and `subordinates` fields.
    pub fn apply_to_specs(&self, specs: &mut [RoleSpec]) -> RoleResult<()> {
        for spec in specs.iter_mut() {
            let name_lower = spec.name.to_lowercase();

            if let Some(sup) = self.supervisor_map.get(&name_lower) {
                spec.supervisor = Some(sup.clone());
                // Add supervisor as escalation target if not already present.
                if !spec.escalation.escalation_targets.contains(sup) {
                    spec.escalation.escalation_targets.push(sup.clone());
                }
            }

            if let Some(subs) = self.subordinate_map.get(&name_lower) {
                spec.subordinates = subs.clone();
            }
        }

        Ok(())
    }

    /// Get the escalation chain for a role (walk up the hierarchy).
    pub fn escalation_chain(&self, role_name: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut current = role_name.to_lowercase();

        while let Some(sup) = self.supervisor_map.get(&current) {
            chain.push(sup.clone());
            current = sup.to_lowercase();
        }

        chain
    }

    /// Validate that all referenced roles in the hierarchy exist in the
    /// provided role name list.
    pub fn validate_against_roles(&self, known_roles: &[&str]) -> Vec<RoleError> {
        let known: std::collections::HashSet<String> =
            known_roles.iter().map(|n| n.to_lowercase()).collect();
        let mut errors = Vec::new();

        for edge in &self.edges {
            if !known.contains(&edge.supervisor.to_lowercase()) {
                errors.push(RoleError::UnknownRelationship {
                    name: edge.supervisor.clone(),
                });
            }
            if !known.contains(&edge.subordinate.to_lowercase()) {
                errors.push(RoleError::UnknownRelationship {
                    name: edge.subordinate.clone(),
                });
            }
        }

        errors
    }
}

impl Default for RoleHierarchy {
    fn default() -> Self {
        Self::from_default_organigram()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hierarchy_has_edges() {
        let h = RoleHierarchy::from_default_organigram();
        assert!(!h.edges().is_empty());
    }

    #[test]
    fn ceo_is_root() {
        let h = RoleHierarchy::from_default_organigram();
        assert!(h.is_root("CEO Agent"));
    }

    #[test]
    fn coo_supervisor_is_ceo() {
        let h = RoleHierarchy::from_default_organigram();
        assert_eq!(h.supervisor_of("COO Agent"), Some("CEO Agent"));
    }

    #[test]
    fn ceo_has_subordinates() {
        let h = RoleHierarchy::from_default_organigram();
        let subs = h.subordinates_of("CEO Agent");
        assert!(subs.contains(&"COO Agent"));
        assert!(subs.contains(&"CFO Agent"));
    }

    #[test]
    fn escalation_chain_walks_up() {
        let h = RoleHierarchy::from_default_organigram();
        let chain = h.escalation_chain("Security & Privacy Agent");
        // Security → CTO → Product → CEO
        assert!(chain.len() >= 3);
        assert_eq!(chain[0], "CTO / Engineering Agent");
    }
}
