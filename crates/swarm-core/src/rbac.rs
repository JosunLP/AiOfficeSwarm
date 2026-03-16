//! Role-Based Access Control (RBAC) primitives.
//!
//! This module defines the core types for the RBAC authorization model:
//! [`Role`], [`Permission`], and [`Subject`]. These are used by the policy
//! engine and the orchestrator to enforce least-privilege access.
//!
//! ## Model
//! - A **Subject** is the entity requesting an action (agent, user, service account).
//! - A **Role** bundles a set of **Permissions** that can be granted to a subject.
//! - The policy engine checks whether a subject has the required permission
//!   for a given action on a given resource.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// An entity that can be granted roles and permissions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Subject {
    /// A registered agent identified by its ID string.
    Agent(String),
    /// A human operator or API user.
    User(String),
    /// A service account used by automated systems.
    ServiceAccount(String),
    /// A plugin identified by its name.
    Plugin(String),
}

impl Subject {
    /// Returns a stable string representation for use in audit logs.
    pub fn as_str(&self) -> String {
        match self {
            Subject::Agent(id) => format!("agent:{}", id),
            Subject::User(name) => format!("user:{}", name),
            Subject::ServiceAccount(name) => format!("serviceaccount:{}", name),
            Subject::Plugin(name) => format!("plugin:{}", name),
        }
    }
}

/// A permission represents an allowed action on a resource type.
///
/// Permissions follow the `verb:resource` pattern (e.g., `"create:task"`,
/// `"read:agent"`, `"invoke:plugin"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Permission {
    /// The action verb (e.g., `"create"`, `"read"`, `"update"`, `"delete"`,
    /// `"invoke"`).
    pub verb: String,
    /// The resource type the permission applies to (e.g., `"task"`, `"agent"`,
    /// `"plugin"`).
    pub resource: String,
}

impl Permission {
    /// Create a new permission.
    pub fn new(verb: impl Into<String>, resource: impl Into<String>) -> Self {
        Self {
            verb: verb.into(),
            resource: resource.into(),
        }
    }

    /// Wildcard permission that grants all verbs on all resources.
    /// Use with extreme caution — only for administrative subjects.
    pub fn wildcard() -> Self {
        Self::new("*", "*")
    }

    /// Returns `true` if this permission grants the `required` permission.
    ///
    /// A wildcard (`*`) on either field matches any value in the same field
    /// of `required`.
    pub fn grants(&self, required: &Permission) -> bool {
        let verb_match = self.verb == "*" || self.verb == required.verb;
        let resource_match = self.resource == "*" || self.resource == required.resource;
        verb_match && resource_match
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.verb, self.resource)
    }
}

/// A named set of permissions that can be granted to subjects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Unique name for this role (e.g., `"admin"`, `"task-executor"`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Permissions included in this role.
    pub permissions: HashSet<Permission>,
}

impl Role {
    /// Create a new role with no permissions.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            permissions: HashSet::new(),
        }
    }

    /// Add a permission to this role.
    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions.insert(permission);
    }

    /// Returns `true` if this role grants the specified permission.
    pub fn has_permission(&self, required: &Permission) -> bool {
        self.permissions.iter().any(|p| p.grants(required))
    }
}

/// Built-in roles provided by the framework as a starting point.
pub mod builtin_roles {
    use super::{Permission, Role};

    /// Returns the `admin` role with wildcard permissions.
    /// Intended only for framework administrators.
    pub fn admin() -> Role {
        let mut role = Role::new("admin", "Full access to all framework resources");
        role.add_permission(Permission::wildcard());
        role
    }

    /// Returns the `task-executor` role for worker agents.
    pub fn task_executor() -> Role {
        let mut role = Role::new("task-executor", "Can execute tasks and report results");
        role.add_permission(Permission::new("read", "task"));
        role.add_permission(Permission::new("update", "task"));
        role.add_permission(Permission::new("read", "agent"));
        role
    }

    /// Returns the `task-submitter` role for clients submitting tasks.
    pub fn task_submitter() -> Role {
        let mut role = Role::new("task-submitter", "Can submit new tasks");
        role.add_permission(Permission::new("create", "task"));
        role.add_permission(Permission::new("read", "task"));
        role
    }

    /// Returns the `observer` role for read-only monitoring access.
    pub fn observer() -> Role {
        let mut role = Role::new("observer", "Read-only access to framework state");
        role.add_permission(Permission::new("read", "*"));
        role
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_exact_match() {
        let p = Permission::new("create", "task");
        assert!(p.grants(&Permission::new("create", "task")));
        assert!(!p.grants(&Permission::new("delete", "task")));
    }

    #[test]
    fn permission_wildcard_verb() {
        let p = Permission::new("*", "task");
        assert!(p.grants(&Permission::new("create", "task")));
        assert!(p.grants(&Permission::new("delete", "task")));
        assert!(!p.grants(&Permission::new("create", "agent")));
    }

    #[test]
    fn permission_full_wildcard() {
        let p = Permission::wildcard();
        assert!(p.grants(&Permission::new("create", "task")));
        assert!(p.grants(&Permission::new("delete", "agent")));
    }

    #[test]
    fn role_has_permission() {
        let mut role = Role::new("executor", "Can execute tasks");
        role.add_permission(Permission::new("update", "task"));

        assert!(role.has_permission(&Permission::new("update", "task")));
        assert!(!role.has_permission(&Permission::new("delete", "task")));
    }

    #[test]
    fn admin_role_grants_everything() {
        let admin = builtin_roles::admin();
        assert!(admin.has_permission(&Permission::new("delete", "agent")));
        assert!(admin.has_permission(&Permission::new("invoke", "plugin")));
    }

    #[test]
    fn subject_display() {
        let s = Subject::Agent("abc-123".into());
        assert_eq!(s.as_str(), "agent:abc-123");
    }
}
