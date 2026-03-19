//! Role registry.
//!
//! A concurrent, thread-safe registry of loaded [`RoleSpec`] instances.
//! Uses `DashMap` for lock-free concurrent access, consistent with the
//! registry pattern used throughout the framework.

use dashmap::DashMap;
use std::sync::Arc;

use crate::error::{RoleError, RoleResult};
use crate::model::{DepartmentCategory, RoleId, RoleSpec};

/// A concurrent registry of role specifications.
///
/// The registry is the single source of truth for loaded roles during
/// the lifetime of a swarm instance.
#[derive(Clone)]
pub struct RoleRegistry {
    inner: Arc<DashMap<RoleId, RoleSpec>>,
    /// Index: role name (lowercased) → RoleId for fast name-based lookup.
    name_index: Arc<DashMap<String, RoleId>>,
}

impl RoleRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            name_index: Arc::new(DashMap::new()),
        }
    }

    /// Register a role specification.
    ///
    /// Returns `Err` if a role with the same ID is already registered.
    pub fn register(&self, spec: RoleSpec) -> RoleResult<RoleId> {
        let id = spec.id;
        let name_key = spec.name.to_lowercase();

        if self.inner.contains_key(&id) {
            return Err(RoleError::Duplicate { id: id.to_string() });
        }

        self.name_index.insert(name_key, id);
        self.inner.insert(id, spec);
        tracing::info!(role_id = %id, "Role registered");
        Ok(id)
    }

    /// Replace an existing role specification (for version updates).
    pub fn update(&self, spec: RoleSpec) -> RoleResult<()> {
        let id = spec.id;
        if !self.inner.contains_key(&id) {
            return Err(RoleError::NotFound { id: id.to_string() });
        }
        let name_key = spec.name.to_lowercase();
        self.name_index.insert(name_key, id);
        self.inner.insert(id, spec);
        tracing::info!(role_id = %id, "Role updated");
        Ok(())
    }

    /// Retrieve a role specification by ID.
    pub fn get(&self, id: &RoleId) -> RoleResult<RoleSpec> {
        self.inner
            .get(id)
            .map(|r| r.value().clone())
            .ok_or_else(|| RoleError::NotFound { id: id.to_string() })
    }

    /// Retrieve a role specification by name (case-insensitive).
    pub fn get_by_name(&self, name: &str) -> RoleResult<RoleSpec> {
        let key = name.to_lowercase();
        let id = self
            .name_index
            .get(&key)
            .map(|r| *r.value())
            .ok_or_else(|| RoleError::NotFound {
                id: name.to_string(),
            })?;
        self.get(&id)
    }

    /// Look up a role ID by name (case-insensitive).
    pub fn id_by_name(&self, name: &str) -> Option<RoleId> {
        self.name_index
            .get(&name.to_lowercase())
            .map(|r| *r.value())
    }

    /// Deregister a role by ID.
    pub fn deregister(&self, id: &RoleId) -> RoleResult<RoleSpec> {
        let (_, spec) = self
            .inner
            .remove(id)
            .ok_or_else(|| RoleError::NotFound { id: id.to_string() })?;
        self.name_index.remove(&spec.name.to_lowercase());
        tracing::info!(role_id = %id, "Role deregistered");
        Ok(spec)
    }

    /// Return all registered roles.
    pub fn all(&self) -> Vec<RoleSpec> {
        self.inner.iter().map(|r| r.value().clone()).collect()
    }

    /// Return all roles in a specific department.
    pub fn by_department(&self, dept: &DepartmentCategory) -> Vec<RoleSpec> {
        self.inner
            .iter()
            .filter(|r| r.value().department == *dept)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Return the number of registered roles.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl Default for RoleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_retrieve() {
        let registry = RoleRegistry::new();
        let spec = RoleSpec::new("Test Role", DepartmentCategory::Governance);
        let id = registry.register(spec.clone()).unwrap();
        let retrieved = registry.get(&id).unwrap();
        assert_eq!(retrieved.name, "Test Role");
    }

    #[test]
    fn lookup_by_name_case_insensitive() {
        let registry = RoleRegistry::new();
        let spec = RoleSpec::new("CEO Agent", DepartmentCategory::Governance);
        registry.register(spec).unwrap();
        let result = registry.get_by_name("ceo agent");
        assert!(result.is_ok());
    }

    #[test]
    fn duplicate_registration_fails() {
        let registry = RoleRegistry::new();
        let spec = RoleSpec::new("CEO Agent", DepartmentCategory::Governance);
        registry.register(spec.clone()).unwrap();
        let result = registry.register(spec);
        assert!(result.is_err());
    }

    #[test]
    fn deregister_removes_role() {
        let registry = RoleRegistry::new();
        let spec = RoleSpec::new("Temp Role", DepartmentCategory::BackOffice);
        let id = registry.register(spec).unwrap();
        assert_eq!(registry.len(), 1);
        registry.deregister(&id).unwrap();
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn by_department_filters() {
        let registry = RoleRegistry::new();
        registry
            .register(RoleSpec::new("A", DepartmentCategory::Governance))
            .unwrap();
        registry
            .register(RoleSpec::new("B", DepartmentCategory::Customer))
            .unwrap();
        registry
            .register(RoleSpec::new("C", DepartmentCategory::Governance))
            .unwrap();
        let gov = registry.by_department(&DepartmentCategory::Governance);
        assert_eq!(gov.len(), 2);
    }
}
