//! Capability definitions for agents and plugins.
//!
//! A [`Capability`] is a named, versioned ability that an agent can possess and
//! that tasks can require. The orchestrator uses capability matching to route
//! tasks to agents that can fulfil them.
//!
//! ## Design rationale
//! Capabilities act as a lightweight contract between task producers and agent
//! executors without requiring compile-time coupling. A capability is
//! intentionally kept as a simple string-plus-version so that it can be extended
//! dynamically by plugins.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// A named, optionally versioned ability that an agent or plugin can advertise.
///
/// ## Examples
/// ```
/// use swarm_core::capability::Capability;
///
/// let cap = Capability::new("text-generation");
/// let versioned = Capability::with_version("image-analysis", "2.0");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capability {
    /// The canonical name of this capability (e.g., `"text-generation"`).
    pub name: String,
    /// An optional semantic version string (e.g., `"1.0"`, `"2.3.1"`).
    pub version: Option<String>,
}

impl Capability {
    /// Create a capability with only a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
        }
    }

    /// Create a capability with a name and an explicit version.
    pub fn with_version(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: Some(version.into()),
        }
    }

    /// Returns `true` if this capability satisfies `required`.
    ///
    /// A capability satisfies a requirement if the names match. If `required`
    /// specifies a version, this capability's version must match exactly.
    /// (Future versions may implement semver range matching.)
    pub fn satisfies(&self, required: &Capability) -> bool {
        if self.name != required.name {
            return false;
        }
        match &required.version {
            None => true,
            Some(req_ver) => self.version.as_deref() == Some(req_ver.as_str()),
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Some(v) => write!(f, "{}@{}", self.name, v),
            None => write!(f, "{}", self.name),
        }
    }
}

/// A set of [`Capability`] values advertised by an agent or required by a task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet(HashSet<Capability>);

impl CapabilitySet {
    /// Create an empty capability set.
    pub fn new() -> Self {
        Self(HashSet::new())
    }

    /// Add a capability to the set.
    pub fn add(&mut self, cap: Capability) {
        self.0.insert(cap);
    }

    /// Returns `true` if this set satisfies all capabilities in `required`.
    pub fn satisfies_all(&self, required: &CapabilitySet) -> bool {
        required
            .0
            .iter()
            .all(|req| self.0.iter().any(|owned| owned.satisfies(req)))
    }

    /// Returns `true` if the set contains no capabilities.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of capabilities in the set.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Iterate over all capabilities.
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.0.iter()
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_satisfies_unversioned_requirement() {
        let owned = Capability::with_version("text-generation", "1.0");
        let req = Capability::new("text-generation");
        assert!(owned.satisfies(&req));
    }

    #[test]
    fn capability_does_not_satisfy_wrong_version() {
        let owned = Capability::with_version("text-generation", "1.0");
        let req = Capability::with_version("text-generation", "2.0");
        assert!(!owned.satisfies(&req));
    }

    #[test]
    fn capability_set_satisfies_all() {
        let mut set = CapabilitySet::new();
        set.add(Capability::new("text-generation"));
        set.add(Capability::new("image-analysis"));

        let mut required = CapabilitySet::new();
        required.add(Capability::new("text-generation"));

        assert!(set.satisfies_all(&required));
    }

    #[test]
    fn capability_set_fails_missing_capability() {
        let mut set = CapabilitySet::new();
        set.add(Capability::new("text-generation"));

        let mut required = CapabilitySet::new();
        required.add(Capability::new("video-analysis"));

        assert!(!set.satisfies_all(&required));
    }

    #[test]
    fn capability_display() {
        let cap = Capability::with_version("text-generation", "1.0");
        assert_eq!(cap.to_string(), "text-generation@1.0");
    }
}
