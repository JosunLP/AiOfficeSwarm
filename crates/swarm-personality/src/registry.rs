//! Runtime registry of personality profiles.

use dashmap::DashMap;
use tracing;

use swarm_core::error::{SwarmError, SwarmResult};

use crate::profile::PersonalityProfile;
use crate::traits::PersonalityId;

/// Thread-safe registry of personality profiles.
pub struct PersonalityRegistry {
    profiles: DashMap<PersonalityId, PersonalityProfile>,
}

impl Default for PersonalityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PersonalityRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            profiles: DashMap::new(),
        }
    }

    /// Register a personality profile.
    pub fn register(&self, profile: PersonalityProfile) -> SwarmResult<()> {
        let id = profile.id;
        let name = profile.name.clone();
        if self.profiles.contains_key(&id) {
            return Err(SwarmError::Internal {
                reason: format!("Personality '{}' ({}) is already registered", name, id),
            });
        }
        self.profiles.insert(id, profile);
        tracing::info!(personality_id = %id, name = %name, "Personality registered");
        Ok(())
    }

    /// Look up a personality by ID.
    pub fn get(&self, id: &PersonalityId) -> Option<PersonalityProfile> {
        self.profiles.get(id).map(|r| r.value().clone())
    }

    /// Remove a personality profile.
    pub fn deregister(&self, id: &PersonalityId) -> SwarmResult<()> {
        self.profiles
            .remove(id)
            .ok_or_else(|| SwarmError::Internal {
                reason: format!("Personality {} not found", id),
            })?;
        Ok(())
    }

    /// List all registered personality profiles (id, name, version).
    pub fn list(&self) -> Vec<(PersonalityId, String, String)> {
        self.profiles
            .iter()
            .map(|r| {
                (
                    r.value().id,
                    r.value().name.clone(),
                    r.value().version.clone(),
                )
            })
            .collect()
    }

    /// Return the number of registered profiles.
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Return `true` if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_get() {
        let registry = PersonalityRegistry::new();
        let profile = PersonalityProfile::new("Test", "1.0.0");
        let id = profile.id;
        registry.register(profile).unwrap();
        let retrieved = registry.get(&id).unwrap();
        assert_eq!(retrieved.name, "Test");
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn duplicate_registration_fails() {
        let registry = PersonalityRegistry::new();
        let profile = PersonalityProfile::new("Test", "1.0.0");
        let dup = profile.clone();
        registry.register(profile).unwrap();
        assert!(registry.register(dup).is_err());
    }
}
