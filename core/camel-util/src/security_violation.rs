//! A security policy violation. Port of `org.apache.camel.util.SecurityViolation`.

use std::fmt;

/// Represents a security policy violation detected during configuration analysis.
///
/// *Since Camel 4.19*
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecurityViolation {
    /// The security category (e.g., `secret`, `insecure:ssl`, `insecure:serialization`,
    /// `insecure:dev`).
    pub category: String,
    /// The full property key (e.g., `camel.ssl.trustAllCertificates`).
    pub property_key: String,
    /// Human-readable description of the violation.
    pub message: String,
    /// The effective policy that was resolved for this violation (`warn` or `fail`).
    pub policy: String,
}

impl SecurityViolation {
    /// Creates a new violation.
    pub fn new(category: &str, property_key: &str, message: &str, policy: &str) -> Self {
        SecurityViolation {
            category: category.into(),
            property_key: property_key.into(),
            message: message.into(),
            policy: policy.into(),
        }
    }
}

impl fmt::Display for SecurityViolation {
    /// Formats as `[category] message [propertyKey]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} [{}]",
            self.category, self.message, self.property_key
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        let v = SecurityViolation::new(
            "insecure:ssl",
            "camel.ssl.trustAllCertificates",
            "Trusting all",
            "warn",
        );
        assert_eq!(
            "[insecure:ssl] Trusting all [camel.ssl.trustAllCertificates]",
            v.to_string()
        );
    }
}
