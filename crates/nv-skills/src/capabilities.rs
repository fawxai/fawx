//! Capability enforcement for skills.

use crate::manifest::Capability;
use nv_core::error::SkillError;
use std::collections::HashSet;

/// Capability checker for runtime enforcement.
#[derive(Debug, Clone)]
pub struct CapabilityChecker {
    allowed: HashSet<Capability>,
}

impl CapabilityChecker {
    /// Create a new capability checker with the allowed capabilities.
    pub fn new(allowed: Vec<Capability>) -> Self {
        Self {
            allowed: allowed.into_iter().collect(),
        }
    }

    /// Check if a single capability is allowed.
    pub fn check(&self, required: &Capability) -> Result<(), SkillError> {
        if self.allowed.contains(required) {
            Ok(())
        } else {
            Err(SkillError::Execution(format!(
                "Capability denied: {}",
                required
            )))
        }
    }

    /// Check if all required capabilities are allowed.
    ///
    /// Returns an error on the first denied capability.
    pub fn check_all(&self, required: &[Capability]) -> Result<(), SkillError> {
        for cap in required {
            self.check(cap)?;
        }
        Ok(())
    }

    /// Get the set of allowed capabilities.
    pub fn allowed(&self) -> &HashSet<Capability> {
        &self.allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_allowed_capability() {
        let checker = CapabilityChecker::new(vec![Capability::Network, Capability::Storage]);

        assert!(checker.check(&Capability::Network).is_ok());
        assert!(checker.check(&Capability::Storage).is_ok());
    }

    #[test]
    fn test_check_denied_capability() {
        let checker = CapabilityChecker::new(vec![Capability::Network]);

        let result = checker.check(&Capability::PhoneActions);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Execution(_))));
    }

    #[test]
    fn test_check_all_allowed() {
        let checker = CapabilityChecker::new(vec![
            Capability::Network,
            Capability::Storage,
            Capability::Notifications,
        ]);

        let required = vec![Capability::Network, Capability::Storage];
        assert!(checker.check_all(&required).is_ok());
    }

    #[test]
    fn test_check_all_with_denied() {
        let checker = CapabilityChecker::new(vec![Capability::Network]);

        let required = vec![Capability::Network, Capability::PhoneActions];
        let result = checker.check_all(&required);

        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Execution(_))));
    }

    #[test]
    fn test_check_empty_capabilities() {
        let checker = CapabilityChecker::new(vec![]);
        assert!(checker.check(&Capability::Network).is_err());
    }

    #[test]
    fn test_check_all_empty_required() {
        let checker = CapabilityChecker::new(vec![Capability::Network]);
        assert!(checker.check_all(&[]).is_ok());
    }
}
