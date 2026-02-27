//! Capability enforcement for skills.

use crate::manifest::Capability;
use fx_core::error::SkillError;
use std::collections::HashSet;

#[cfg(feature = "audit")]
use {
    fx_security::{AuditEvent, AuditEventType, AuditLog},
    std::collections::BTreeMap,
};

/// Capability checker for runtime enforcement.
#[derive(Debug, Clone)]
pub struct CapabilityChecker {
    allowed: HashSet<Capability>,
    skill_name: String,
}

impl CapabilityChecker {
    /// Create a new capability checker with the allowed capabilities.
    ///
    /// # Arguments
    /// * `allowed` - List of capabilities the skill is allowed to use
    /// * `skill_name` - Name of the skill (for audit logging)
    pub fn new(allowed: Vec<Capability>, skill_name: impl Into<String>) -> Self {
        Self {
            allowed: allowed.into_iter().collect(),
            skill_name: skill_name.into(),
        }
    }

    /// Check if a single capability is allowed (synchronous, no audit).
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

    /// Check if a single capability is allowed with optional audit logging.
    ///
    /// This async version logs capability checks to the audit system if provided.
    ///
    /// # Audit Semantics
    /// - Audit events are logged **after** the capability check is performed
    /// - If audit logging fails, a warning is logged but the capability check
    ///   result is still returned (audit failures don't block operations)
    /// - If `audit_log` is `None`, no logging occurs (valid for testing/development)
    /// - Both allowed and denied capability checks are logged
    ///
    /// # Arguments
    /// * `required` - The capability to check
    /// * `audit_log` - Optional audit log for recording the check
    ///
    /// # Returns
    /// * `Ok(())` - If capability is allowed
    /// * `Err(SkillError)` - If capability is denied
    ///
    /// # Note
    /// In production, `audit_log` should always be `Some` for security monitoring.
    /// Passing `None` is intended for testing and development scenarios only.
    #[cfg(feature = "audit")]
    pub async fn check_with_audit(
        &self,
        required: &Capability,
        audit_log: Option<&mut AuditLog>,
    ) -> Result<(), SkillError> {
        let allowed = self.allowed.contains(required);

        // Log the capability check if audit log is provided
        if let Some(log) = audit_log {
            let mut metadata = BTreeMap::new();
            metadata.insert("capability".to_string(), required.to_string());
            metadata.insert(
                "result".to_string(),
                if allowed { "allowed" } else { "denied" }.to_string(),
            );

            let description = if allowed {
                format!("Skill '{}' used capability: {}", self.skill_name, required)
            } else {
                format!(
                    "Skill '{}' denied capability: {}",
                    self.skill_name, required
                )
            };

            // Attempt to create and log the audit event
            // If audit logging fails, log a warning but don't fail the capability check
            match AuditEvent::with_metadata(
                AuditEventType::SkillInvoked,
                format!("skill:{}", self.skill_name),
                description,
                metadata,
            ) {
                Ok(event) => {
                    if let Err(e) = log.append(event).await {
                        tracing::warn!(
                            skill = %self.skill_name,
                            capability = %required,
                            error = %e,
                            "Failed to write capability check to audit log"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        skill = %self.skill_name,
                        capability = %required,
                        error = %e,
                        "Failed to create audit event for capability check"
                    );
                }
            }
        }

        // Return the capability check result (independent of audit logging)
        if allowed {
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

    /// Check if all required capabilities are allowed with optional audit logging.
    ///
    /// Logs each capability check to the audit system if provided.
    ///
    /// # Audit Ordering
    /// Capability checks are performed sequentially, and each is logged
    /// immediately after its check completes. This ensures audit log entries
    /// appear in check order, which is important for security analysis.
    ///
    /// If a capability is denied, the method returns immediately with an error,
    /// but any previous capability checks will still have been logged.
    ///
    /// # Performance Note
    /// Each capability check awaits individually, which means audit log writes
    /// happen sequentially. For skills with many capabilities (rare), this could
    /// add latency. This is intentional to maintain audit log ordering and
    /// simplify error handling. Consider this a tradeoff for correctness over
    /// raw performance.
    ///
    /// # Arguments
    /// * `required` - List of capabilities to check
    /// * `audit_log` - Optional audit log for recording checks
    ///
    /// # Returns
    /// * `Ok(())` - If all capabilities are allowed
    /// * `Err(SkillError)` - On first denied capability
    ///
    /// # Example
    /// ```no_run
    /// # use fx_skills::CapabilityChecker;
    /// # use fx_skills::manifest::Capability;
    /// # #[cfg(feature = "audit")]
    /// # use fx_security::AuditLog;
    /// # async fn example() {
    /// # #[cfg(feature = "audit")]
    /// # let mut audit_log = AuditLog::in_memory();
    /// let checker = CapabilityChecker::new(
    ///     vec![Capability::Network, Capability::Storage],
    ///     "my_skill"
    /// );
    ///
    /// # #[cfg(feature = "audit")]
    /// let result = checker.check_all_with_audit(
    ///     &[Capability::Network, Capability::Storage],
    ///     Some(&mut audit_log)
    /// ).await;
    /// // Both checks logged, result is Ok(())
    /// # }
    /// ```
    #[cfg(feature = "audit")]
    pub async fn check_all_with_audit(
        &self,
        required: &[Capability],
        audit_log: Option<&mut AuditLog>,
    ) -> Result<(), SkillError> {
        for cap in required {
            self.check_with_audit(cap, audit_log).await?;
        }
        Ok(())
    }

    /// Get the set of allowed capabilities.
    pub fn allowed(&self) -> &HashSet<Capability> {
        &self.allowed
    }

    /// Get the skill name.
    pub fn skill_name(&self) -> &str {
        &self.skill_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_allowed_capability() {
        let checker =
            CapabilityChecker::new(vec![Capability::Network, Capability::Storage], "test_skill");

        assert!(checker.check(&Capability::Network).is_ok());
        assert!(checker.check(&Capability::Storage).is_ok());
    }

    #[test]
    fn test_check_denied_capability() {
        let checker = CapabilityChecker::new(vec![Capability::Network], "test_skill");

        let result = checker.check(&Capability::PhoneActions);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Execution(_))));
    }

    #[test]
    fn test_check_all_allowed() {
        let checker = CapabilityChecker::new(
            vec![
                Capability::Network,
                Capability::Storage,
                Capability::Notifications,
            ],
            "test_skill",
        );

        let required = vec![Capability::Network, Capability::Storage];
        assert!(checker.check_all(&required).is_ok());
    }

    #[test]
    fn test_check_all_with_denied() {
        let checker = CapabilityChecker::new(vec![Capability::Network], "test_skill");

        let required = vec![Capability::Network, Capability::PhoneActions];
        let result = checker.check_all(&required);

        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Execution(_))));
    }

    #[test]
    fn test_check_empty_capabilities() {
        let checker = CapabilityChecker::new(vec![], "test_skill");
        assert!(checker.check(&Capability::Network).is_err());
    }

    #[test]
    fn test_check_all_empty_required() {
        let checker = CapabilityChecker::new(vec![Capability::Network], "test_skill");
        assert!(checker.check_all(&[]).is_ok());
    }

    #[test]
    fn test_skill_name() {
        let checker = CapabilityChecker::new(vec![Capability::Network], "my_skill");
        assert_eq!(checker.skill_name(), "my_skill");
    }

    #[cfg(feature = "audit")]
    #[tokio::test]
    async fn test_check_with_audit_allowed() {
        use fx_security::AuditLog;

        let checker = CapabilityChecker::new(vec![Capability::Network], "audit_test_skill");
        let mut log = AuditLog::in_memory();

        let result = checker
            .check_with_audit(&Capability::Network, Some(&mut log))
            .await;

        assert!(result.is_ok());
        assert_eq!(log.count(), 1);

        let events = log.query(&Default::default()).unwrap();
        assert_eq!(events[0].actor, "skill:audit_test_skill");
        assert!(events[0].description.contains("used capability"));
        assert_eq!(
            events[0].metadata.get("capability"),
            Some(&"network".to_string())
        );
        assert_eq!(
            events[0].metadata.get("result"),
            Some(&"allowed".to_string())
        );
    }

    #[cfg(feature = "audit")]
    #[tokio::test]
    async fn test_check_with_audit_denied() {
        use fx_security::AuditLog;

        let checker = CapabilityChecker::new(vec![Capability::Network], "audit_test_skill");
        let mut log = AuditLog::in_memory();

        let result = checker
            .check_with_audit(&Capability::PhoneActions, Some(&mut log))
            .await;

        assert!(result.is_err());
        assert_eq!(log.count(), 1);

        let events = log.query(&Default::default()).unwrap();
        assert_eq!(events[0].actor, "skill:audit_test_skill");
        assert!(events[0].description.contains("denied capability"));
        assert_eq!(
            events[0].metadata.get("capability"),
            Some(&"phone_actions".to_string())
        );
        assert_eq!(
            events[0].metadata.get("result"),
            Some(&"denied".to_string())
        );
    }

    #[cfg(feature = "audit")]
    #[tokio::test]
    async fn test_check_with_audit_none() {
        let checker = CapabilityChecker::new(vec![Capability::Network], "audit_test_skill");

        // Should work without audit log
        let result = checker.check_with_audit(&Capability::Network, None).await;
        assert!(result.is_ok());
    }

    #[cfg(feature = "audit")]
    #[tokio::test]
    async fn test_check_all_with_audit() {
        use fx_security::AuditLog;

        let checker = CapabilityChecker::new(
            vec![Capability::Network, Capability::Storage],
            "multi_check_skill",
        );
        let mut log = AuditLog::in_memory();

        let required = vec![Capability::Network, Capability::Storage];
        let result = checker
            .check_all_with_audit(&required, Some(&mut log))
            .await;

        assert!(result.is_ok());
        assert_eq!(log.count(), 2);

        let events = log.query(&Default::default()).unwrap();
        assert!(events[0].description.contains("network"));
        assert!(events[1].description.contains("storage"));
    }

    #[cfg(feature = "audit")]
    #[tokio::test]
    async fn test_check_all_with_audit_partial_deny() {
        use fx_security::AuditLog;

        let checker = CapabilityChecker::new(vec![Capability::Network], "partial_deny_skill");
        let mut log = AuditLog::in_memory();

        let required = vec![Capability::Network, Capability::PhoneActions];
        let result = checker
            .check_all_with_audit(&required, Some(&mut log))
            .await;

        assert!(result.is_err());
        // Should have logged Network (allowed) and PhoneActions (denied)
        assert_eq!(log.count(), 2);

        let events = log.query(&Default::default()).unwrap();
        assert_eq!(
            events[0].metadata.get("result"),
            Some(&"allowed".to_string())
        );
        assert_eq!(
            events[1].metadata.get("result"),
            Some(&"denied".to_string())
        );
    }
}
