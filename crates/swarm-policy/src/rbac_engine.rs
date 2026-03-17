//! RBAC enforcement engine.
//!
//! The [`RbacEngine`] manages the assignment of [`Role`]s to [`Subject`]s and
//! provides permission checking. It is used by the policy engine (via a
//! dedicated RBAC policy) and can also be queried directly.

use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;

use swarm_core::{
    rbac::{Permission, Role, Subject},
};

/// Manages role assignments and evaluates permission queries.
#[derive(Clone, Default)]
pub struct RbacEngine {
    /// Map from role name to Role definition.
    roles: Arc<DashMap<String, Role>>,
    /// Map from Subject to the set of role names assigned to it.
    assignments: Arc<DashMap<Subject, HashSet<String>>>,
}

impl RbacEngine {
    /// Create a new RBAC engine with no roles or assignments.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a role definition.
    pub fn define_role(&self, role: Role) {
        tracing::debug!(role = %role.name, "Role defined");
        self.roles.insert(role.name.clone(), role);
    }

    /// Assign a named role to a subject.
    ///
    /// If the role does not exist yet, the assignment is stored but will not
    /// grant any permissions until the role is defined.
    pub fn assign_role(&self, subject: &Subject, role_name: impl Into<String>) {
        let role = role_name.into();
        self.assignments
            .entry(subject.clone())
            .or_insert_with(HashSet::new)
            .insert(role.clone());
        tracing::debug!(subject = %subject, role = %role, "Role assigned to subject");
    }

    /// Revoke a role from a subject.
    pub fn revoke_role(&self, subject: &Subject, role_name: &str) {
        if let Some(mut roles) = self.assignments.get_mut(subject) {
            roles.remove(role_name);
        }
    }

    /// Returns `true` if the subject has been granted the required permission
    /// through any of their assigned roles.
    pub fn has_permission(&self, subject: &Subject, required: &Permission) -> bool {
        let Some(role_names) = self.assignments.get(subject) else {
            return false;
        };
        role_names.iter().any(|role_name| {
            self.roles
                .get(role_name)
                .map(|role| role.has_permission(required))
                .unwrap_or(false)
        })
    }

    /// Return all permissions granted to a subject across all their roles.
    pub fn effective_permissions(&self, subject: &Subject) -> Vec<Permission> {
        let Some(role_names) = self.assignments.get(subject) else {
            return Vec::new();
        };
        let mut perms: HashSet<Permission> = HashSet::new();
        for role_name in role_names.iter() {
            if let Some(role) = self.roles.get(role_name.as_str()) {
                for p in &role.permissions {
                    perms.insert(p.clone());
                }
            }
        }
        perms.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_core::rbac::{builtin_roles, Permission, Subject};

    #[test]
    fn assign_role_and_check_permission() {
        let engine = RbacEngine::new();
        engine.define_role(builtin_roles::task_executor());

        let subject = Subject::Agent("worker-1".into());
        engine.assign_role(&subject, "task-executor");

        assert!(engine.has_permission(&subject, &Permission::new("read", "task")));
        assert!(!engine.has_permission(&subject, &Permission::new("delete", "agent")));
    }

    #[test]
    fn no_permission_without_role() {
        let engine = RbacEngine::new();
        let subject = Subject::User("alice".into());
        assert!(!engine.has_permission(&subject, &Permission::new("create", "task")));
    }

    #[test]
    fn admin_role_grants_everything() {
        let engine = RbacEngine::new();
        engine.define_role(builtin_roles::admin());

        let subject = Subject::User("admin-user".into());
        engine.assign_role(&subject, "admin");

        assert!(engine.has_permission(&subject, &Permission::new("delete", "agent")));
        assert!(engine.has_permission(&subject, &Permission::new("invoke", "plugin")));
    }

    #[test]
    fn revoke_role_removes_permissions() {
        let engine = RbacEngine::new();
        engine.define_role(builtin_roles::task_executor());
        let subject = Subject::Agent("worker".into());
        engine.assign_role(&subject, "task-executor");
        engine.revoke_role(&subject, "task-executor");
        assert!(!engine.has_permission(&subject, &Permission::new("read", "task")));
    }
}
