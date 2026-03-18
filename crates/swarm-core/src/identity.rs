//! Unique identifier types for all first-class domain entities.
//!
//! Every entity in the swarm is identified by a strongly-typed UUID wrapper.
//! Using newtype wrappers prevents accidental mixing of identifiers (e.g., passing
//! an `AgentId` where a `TaskId` is expected).

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! define_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(Uuid);

        impl $name {
            /// Create a new, random identifier.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Create an identifier from a known UUID value (useful in tests or
            /// when deserializing from external systems).
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Return the underlying [`Uuid`].
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::parse_str(s)?))
            }
        }
    };
}

define_id!(AgentId, "A unique identifier for an [`Agent`](crate::agent::Agent) instance.");
define_id!(TaskId, "A unique identifier for a [`Task`](crate::task::Task) instance.");
define_id!(PolicyId, "A unique identifier for a [`Policy`](crate::policy::Policy).");
define_id!(PluginId, "A unique identifier for a registered plugin.");
define_id!(TenantId, "A tenant identifier for logical isolation in a multi-tenant deployment.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_id_is_unique() {
        let a = AgentId::new();
        let b = AgentId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn agent_id_roundtrip_display_and_parse() {
        let id = AgentId::new();
        let s = id.to_string();
        let parsed: AgentId = s.parse().expect("should parse back");
        assert_eq!(id, parsed);
    }

    #[test]
    fn task_id_and_agent_id_differ_in_type() {
        // Compile-time check: the following would fail to compile if types were mixed.
        let _task_id: TaskId = TaskId::new();
        let _agent_id: AgentId = AgentId::new();
    }
}
