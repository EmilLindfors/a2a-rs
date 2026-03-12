use serde::{Deserialize, Serialize};

/// AP2 v0.1 extension URI.
///
/// Agents that support the AP2 extension MUST use this URI in their
/// `AgentExtension` declaration.
pub const AP2_EXTENSION_URI: &str = "https://github.com/google-agentic-commerce/ap2/tree/v0.1";

/// Data-part key for an [`IntentMandate`](super::mandate::IntentMandate).
pub const INTENT_MANDATE_DATA_KEY: &str = "ap2.mandates.IntentMandate";

/// Data-part key for a [`CartMandate`](super::mandate::CartMandate).
pub const CART_MANDATE_DATA_KEY: &str = "ap2.mandates.CartMandate";

/// Data-part key for a [`PaymentMandate`](super::mandate::PaymentMandate).
pub const PAYMENT_MANDATE_DATA_KEY: &str = "ap2.mandates.PaymentMandate";

/// Data-part key for a [`PaymentReceipt`](super::receipt::PaymentReceipt).
pub const PAYMENT_RECEIPT_DATA_KEY: &str = "ap2.PaymentReceipt";

/// Data-part key for implementation-defined risk signals.
pub const RISK_DATA_KEY: &str = "risk_data";

/// Roles that agents can fulfill in the AP2 ecosystem.
///
/// Every agent that supports the AP2 extension MUST perform at least one role.
/// The role is declared in the `AgentExtension.params.roles` array.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ap2Role {
    /// Handles payments and checkout.
    #[serde(rename = "merchant")]
    Merchant,
    /// Makes purchases on the user's behalf.
    #[serde(rename = "shopper")]
    Shopper,
    /// Supplies payment credentials.
    #[serde(rename = "credentials-provider")]
    CredentialsProvider,
    /// Processes payment transactions.
    #[serde(rename = "payment-processor")]
    PaymentProcessor,
}

impl Ap2Role {
    /// Returns the wire-format string for this role.
    pub fn as_str(&self) -> &'static str {
        match self {
            Ap2Role::Merchant => "merchant",
            Ap2Role::Shopper => "shopper",
            Ap2Role::CredentialsProvider => "credentials-provider",
            Ap2Role::PaymentProcessor => "payment-processor",
        }
    }
}

impl std::fmt::Display for Ap2Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serialization() {
        assert_eq!(
            serde_json::to_string(&Ap2Role::Merchant).unwrap(),
            "\"merchant\""
        );
        assert_eq!(
            serde_json::to_string(&Ap2Role::CredentialsProvider).unwrap(),
            "\"credentials-provider\""
        );
        assert_eq!(
            serde_json::to_string(&Ap2Role::PaymentProcessor).unwrap(),
            "\"payment-processor\""
        );
    }

    #[test]
    fn role_deserialization() {
        let role: Ap2Role = serde_json::from_str("\"shopper\"").unwrap();
        assert_eq!(role, Ap2Role::Shopper);
    }

    #[test]
    fn role_display() {
        assert_eq!(
            Ap2Role::CredentialsProvider.to_string(),
            "credentials-provider"
        );
    }
}
