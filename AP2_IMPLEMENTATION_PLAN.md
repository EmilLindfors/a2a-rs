# AP2 (Agent Payments Protocol) Implementation Plan

**Issue**: [#7](https://github.com/EmilLindfors/a2a-rs/issues/7)
**Status**: Planning
**Feature Flag**: `ap2`
**Estimated Complexity**: Medium

## Overview

Implement support for AP2 (Agent Payments Protocol), an A2A extension that enables payment/commerce capabilities between autonomous agents. This implementation will be feature-gated and designed for extensibility.

## Background

- **Requester**: Rig crate maintainer (joshua-mo-143)
- **Use Case**: Enable Rig agents to be converted into A2A agents with payment capabilities
- **Extension URI**: `https://github.com/google-agentic-commerce/ap2/tree/v0.1`
- **Spec Status**: Extremely early (v0.1)

## Implementation Phases

### Phase 1: Extension Infrastructure (Required for all extensions)

**Goal**: Add general extension support following A2A v0.3.0+ specification.

**IMPORTANT**: Use the official `AgentExtension` structure from specification_new.json. This provides a standardized way to declare protocol extensions with URIs, requirements, descriptions, and extension-specific parameters.

#### 1.1 Add AgentExtension Type

**File**: `a2a-rs/src/domain/core/agent.rs`

Add new struct before `AgentCard`:

```rust
/// A protocol extension supported by an agent (A2A v0.3.0+).
///
/// Extensions allow agents to declare support for additional protocols beyond
/// the core A2A specification. Clients can use this information to determine
/// compatibility and enable extended features.
///
/// # Example
/// ```rust
/// use a2a_rs::AgentExtension;
/// use serde_json::json;
///
/// let ap2_extension = AgentExtension {
///     uri: "https://github.com/google-agentic-commerce/ap2/tree/v0.1".to_string(),
///     required: Some(true),
///     description: Some("AP2 payment protocol support".to_string()),
///     params: Some(json!({
///         "roles": ["merchant", "shopper"]
///     }).as_object().unwrap().clone()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExtension {
    /// The unique URI identifying the extension.
    ///
    /// This should be a stable, versioned URI that points to the extension's
    /// specification or documentation.
    pub uri: String,

    /// If true, clients must understand this extension to interact with the agent.
    ///
    /// When true, clients that don't support this extension should not attempt
    /// to use the agent. When false or absent, the extension is optional.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Human-readable description of how this agent uses the extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Extension-specific configuration parameters.
    ///
    /// The structure and content of this field is defined by each extension.
    /// For example, AP2 uses this to declare agent roles like ["merchant", "shopper"].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Map<String, Value>>,
}
```

#### 1.2 Update AgentCapabilities

**File**: `a2a-rs/src/domain/core/agent.rs`

Update `AgentCapabilities` struct:

```rust
/// Capabilities supported by an agent, including streaming and push notifications.
///
/// This structure defines what features an agent supports:
/// - `streaming`: Whether the agent supports real-time streaming updates
/// - `push_notifications`: Whether the agent can send push notifications
/// - `state_transition_history`: Whether the agent maintains task state history
/// - `extensions`: Protocol extensions supported by the agent (v0.3.0+)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentCapabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default, rename = "pushNotifications")]
    pub push_notifications: bool,
    #[serde(default, rename = "stateTransitionHistory")]
    pub state_transition_history: bool,
    /// Protocol extensions supported by this agent (v0.3.0+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<AgentExtension>>,
}
```

#### 1.3 Add Helper Methods

Add to `AgentCapabilities` implementation:

```rust
impl AgentCapabilities {
    /// Add an extension to the capabilities
    pub fn with_extension(mut self, extension: AgentExtension) -> Self {
        match &mut self.extensions {
            Some(exts) => exts.push(extension),
            None => self.extensions = Some(vec![extension]),
        }
        self
    }

    /// Add multiple extensions
    pub fn with_extensions(mut self, extensions: Vec<AgentExtension>) -> Self {
        self.extensions = Some(extensions);
        self
    }

    /// Check if an extension is supported by URI
    pub fn supports_extension(&self, uri: &str) -> bool {
        self.extensions
            .as_ref()
            .map(|exts| exts.iter().any(|ext| ext.uri == uri))
            .unwrap_or(false)
    }

    /// Check if an extension is required
    pub fn is_extension_required(&self, uri: &str) -> bool {
        self.extensions
            .as_ref()
            .and_then(|exts| exts.iter().find(|ext| ext.uri == uri))
            .and_then(|ext| ext.required)
            .unwrap_or(false)
    }
}
```

#### 1.4 Update Message and Artifact Types

**File**: `a2a-rs/src/domain/core/message.rs` and `a2a-rs/src/domain/core/task.rs`

Add extension URIs to Message:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct Message {
    // ... existing fields ...

    /// URIs of extensions relevant to this message (v0.3.0+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
}
```

Add to Artifact:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    // ... existing fields ...

    /// URIs of extensions relevant to this artifact (v0.3.0+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
}
```

#### 1.5 Update Tests

Add test cases for:
- AgentExtension serialization/deserialization
- AgentCapabilities with extensions
- Extension helper methods (supports_extension, is_extension_required)
- Message and Artifact with extension URIs
- Backward compatibility (all extension fields are optional)

**Deliverable**: Complete A2A v0.3.0+ extension infrastructure that's protocol-compliant and ready for AP2 and future extensions

---

### Phase 2: AP2 Type System (Feature-gated)

**Goal**: Create type-safe Rust representations of AP2 mandate schemas.

#### 2.1 Create AP2 Module Structure

**New file**: `a2a-rs/src/domain/extensions/ap2/mod.rs`

```rust
//! AP2 (Agent Payments Protocol) extension support
//!
//! This module provides types and utilities for the AP2 extension,
//! which enables payment and commerce capabilities between agents.
//!
//! Extension URI: https://github.com/google-agentic-commerce/ap2/tree/v0.1

pub mod mandates;
pub mod roles;
pub mod helpers;

// Re-exports
pub use mandates::{IntentMandate, CartMandate, PaymentMandate};
pub use roles::Ap2Role;
pub use helpers::{create_intent_part, create_cart_part, create_payment_part};

/// AP2 extension URI constant
pub const AP2_EXTENSION_URI: &str = "https://github.com/google-agentic-commerce/ap2/tree/v0.1";

/// AP2 data part keys
pub mod keys {
    pub const INTENT_MANDATE: &str = "ap2.mandates.IntentMandate";
    pub const CART_MANDATE: &str = "ap2.mandates.CartMandate";
    pub const PAYMENT_MANDATE: &str = "ap2.mandates.PaymentMandate";
    pub const RISK_DATA: &str = "risk_data";
}
```

#### 2.2 Implement Agent Roles

**New file**: `a2a-rs/src/domain/extensions/ap2/roles.rs`

```rust
use serde::{Deserialize, Serialize};

/// Roles defined in the AP2 specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Ap2Role {
    /// Handles payments and checkout
    Merchant,
    /// Makes purchases on user's behalf
    Shopper,
    /// Supplies payment credentials
    #[serde(rename = "credentials-provider")]
    CredentialsProvider,
    /// Processes transactions
    #[serde(rename = "payment-processor")]
    PaymentProcessor,
}

impl Ap2Role {
    /// Convert role to string as expected by A2A protocol
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
        write!(f, "{}", self.as_str())
    }
}
```

Add helper function to create AgentExtension for AP2:

```rust
use serde_json::json;

/// Create an AgentExtension for AP2 with specified roles
///
/// # Arguments
/// * `roles` - Roles this agent fulfills (must have at least one)
/// * `required` - Whether clients must support AP2 to use this agent
///
/// # Example
/// ```rust
/// let ap2_ext = create_ap2_extension(
///     vec![Ap2Role::Merchant, Ap2Role::Shopper],
///     true
/// );
/// ```
pub fn create_ap2_extension(roles: Vec<Ap2Role>, required: bool) -> AgentExtension {
    use crate::AgentExtension;

    let roles_json: Vec<String> = roles.iter().map(|r| r.as_str().to_string()).collect();

    AgentExtension {
        uri: AP2_EXTENSION_URI.to_string(),
        required: Some(required),
        description: Some("Agent Payments Protocol (AP2) support for commerce capabilities".to_string()),
        params: Some({
            let mut params = serde_json::Map::new();
            params.insert("roles".to_string(), json!(roles_json));
            params
        }),
    }
}
```

#### 2.3 Implement IntentMandate

**New file**: `a2a-rs/src/domain/extensions/ap2/mandates.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// User purchase intent mandate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentMandate {
    /// Whether user confirmation is required for cart
    #[serde(rename = "user_cart_confirmation_required")]
    pub user_cart_confirmation_required: bool,

    /// Natural language description of the purchase intent
    pub natural_language_description: String,

    /// Optional list of acceptable merchants
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merchants: Option<Vec<String>>,

    /// Optional SKUs being requested
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skus: Option<Vec<String>>,

    /// Whether refundability is required
    pub required_refundability: bool,

    /// When this intent expires
    pub intent_expiry: DateTime<Utc>,
}

impl IntentMandate {
    /// Create a new IntentMandate with required fields
    pub fn new(
        user_cart_confirmation_required: bool,
        natural_language_description: String,
        required_refundability: bool,
        intent_expiry: DateTime<Utc>,
    ) -> Self {
        Self {
            user_cart_confirmation_required,
            natural_language_description,
            merchants: None,
            skus: None,
            required_refundability,
            intent_expiry,
        }
    }

    /// Add merchant constraints
    pub fn with_merchants(mut self, merchants: Vec<String>) -> Self {
        self.merchants = Some(merchants);
        self
    }

    /// Add SKU constraints
    pub fn with_skus(mut self, skus: Vec<String>) -> Self {
        self.skus = Some(skus);
        self
    }
}

/// Amount representation for payment details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Amount {
    /// Currency code (e.g., "USD", "EUR")
    pub currency: String,

    /// Amount value as string to avoid floating point issues
    pub value: String,

    /// Optional refund period in ISO 8601 duration format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refund_period: Option<String>,
}

/// Payment request details within a cart
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest {
    /// Payment method identifier
    pub method: String,

    /// Total amount
    pub total: Amount,

    /// Additional payment details
    #[serde(flatten)]
    pub additional_details: Map<String, Value>,
}

/// Cart contents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartContents {
    /// Unique cart identifier
    pub id: String,

    /// Whether user signature is required
    pub user_signature_required: bool,

    /// Payment request details
    pub payment_request: PaymentRequest,

    /// Additional cart details
    #[serde(flatten)]
    pub additional_fields: Map<String, Value>,
}

/// Merchant cart and payment request mandate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartMandate {
    /// Cart and payment details
    pub contents: CartContents,

    /// Cryptographic signature from merchant
    pub merchant_signature: String,

    /// Mandate creation timestamp
    pub timestamp: DateTime<Utc>,
}

impl CartMandate {
    /// Create a new CartMandate
    pub fn new(
        contents: CartContents,
        merchant_signature: String,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            contents,
            merchant_signature,
            timestamp,
        }
    }
}

/// Payment authorization details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentMandateContents {
    /// Unique payment mandate identifier
    pub payment_mandate_id: String,

    /// Payment details identifier
    pub payment_details_id: String,

    /// Total amount with refund information
    pub payment_details_total: Amount,

    /// Payment response details
    pub payment_response: Map<String, Value>,

    /// Merchant agent identifier
    pub merchant_agent: String,

    /// Timestamp of mandate creation
    pub timestamp: DateTime<Utc>,

    /// Additional fields
    #[serde(flatten)]
    pub additional_fields: Map<String, Value>,
}

/// Payment authorization mandate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentMandate {
    /// Payment authorization details
    pub payment_mandate_contents: PaymentMandateContents,

    /// Signed user authorization
    pub user_authorization: String,
}

impl PaymentMandate {
    /// Create a new PaymentMandate
    pub fn new(
        payment_mandate_contents: PaymentMandateContents,
        user_authorization: String,
    ) -> Self {
        Self {
            payment_mandate_contents,
            user_authorization,
        }
    }
}
```

#### 2.4 Add Helper Functions

**New file**: `a2a-rs/src/domain/extensions/ap2/helpers.rs`

```rust
use crate::domain::core::Part;
use crate::domain::extensions::ap2::mandates::{IntentMandate, CartMandate, PaymentMandate};
use crate::domain::extensions::ap2::keys;
use serde_json::{Map, Value};

/// Create a Data Part containing an IntentMandate
pub fn create_intent_part(mandate: &IntentMandate) -> Result<Part, serde_json::Error> {
    let mut data = Map::new();
    data.insert(
        keys::INTENT_MANDATE.to_string(),
        serde_json::to_value(mandate)?
    );
    Ok(Part::data(data))
}

/// Create a Data Part containing a CartMandate
pub fn create_cart_part(mandate: &CartMandate) -> Result<Part, serde_json::Error> {
    let mut data = Map::new();
    data.insert(
        keys::CART_MANDATE.to_string(),
        serde_json::to_value(mandate)?
    );
    Ok(Part::data(data))
}

/// Create a Data Part containing a PaymentMandate
pub fn create_payment_part(mandate: &PaymentMandate) -> Result<Part, serde_json::Error> {
    let mut data = Map::new();
    data.insert(
        keys::PAYMENT_MANDATE.to_string(),
        serde_json::to_value(mandate)?
    );
    Ok(Part::data(data))
}

/// Create a Data Part with risk data
pub fn create_risk_data_part(risk_signals: Map<String, Value>) -> Part {
    let mut data = Map::new();
    data.insert(keys::RISK_DATA.to_string(), Value::Object(risk_signals));
    Part::data(data)
}

/// Extract IntentMandate from a Data Part
pub fn extract_intent_mandate(part: &Part) -> Option<IntentMandate> {
    if let Part::Data { data, .. } = part {
        data.get(keys::INTENT_MANDATE)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    } else {
        None
    }
}

/// Extract CartMandate from a Data Part
pub fn extract_cart_mandate(part: &Part) -> Option<CartMandate> {
    if let Part::Data { data, .. } = part {
        data.get(keys::CART_MANDATE)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    } else {
        None
    }
}

/// Extract PaymentMandate from a Data Part
pub fn extract_payment_mandate(part: &Part) -> Option<PaymentMandate> {
    if let Part::Data { data, .. } = part {
        data.get(keys::PAYMENT_MANDATE)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    } else {
        None
    }
}
```

**Deliverable**: Complete type-safe AP2 mandate system with helpers

---

### Phase 3: Feature Gate Configuration

#### 3.1 Update Cargo.toml

**File**: `a2a-rs/Cargo.toml`

Add feature flag:

```toml
[features]
default = ["server", "tracing"]
# ... existing features ...
ap2 = []  # AP2 extension support
```

#### 3.2 Conditional Compilation

**File**: `a2a-rs/src/domain/mod.rs`

```rust
pub mod core;
pub mod error;
pub mod events;
pub mod protocols;
pub mod validation;

#[cfg(feature = "ap2")]
pub mod extensions;
```

**File**: `a2a-rs/src/domain/extensions/mod.rs`

```rust
#[cfg(feature = "ap2")]
pub mod ap2;
```

**File**: `a2a-rs/src/lib.rs`

Add to re-exports:

```rust
#[cfg(feature = "ap2")]
pub use domain::extensions::ap2::{
    IntentMandate, CartMandate, PaymentMandate, Ap2Role,
    create_intent_part, create_cart_part, create_payment_part,
    AP2_EXTENSION_URI,
};
```

**Deliverable**: AP2 functionality only compiles when feature is enabled

---

### Phase 4: Validation & Error Handling

#### 4.1 Add Validation Functions

**File**: `a2a-rs/src/domain/extensions/ap2/mandates.rs`

```rust
use crate::domain::error::A2AError;

impl IntentMandate {
    /// Validate the IntentMandate
    pub fn validate(&self) -> Result<(), A2AError> {
        // Check expiry is in the future
        if self.intent_expiry <= Utc::now() {
            return Err(A2AError::InvalidParams(
                "intent_expiry must be in the future".to_string()
            ));
        }

        // Validate description is not empty
        if self.natural_language_description.trim().is_empty() {
            return Err(A2AError::InvalidParams(
                "natural_language_description cannot be empty".to_string()
            ));
        }

        Ok(())
    }
}

impl CartMandate {
    /// Validate the CartMandate
    pub fn validate(&self) -> Result<(), A2AError> {
        // Validate cart ID
        if self.contents.id.trim().is_empty() {
            return Err(A2AError::InvalidParams(
                "cart id cannot be empty".to_string()
            ));
        }

        // Validate signature
        if self.merchant_signature.trim().is_empty() {
            return Err(A2AError::InvalidParams(
                "merchant_signature cannot be empty".to_string()
            ));
        }

        Ok(())
    }
}

impl PaymentMandate {
    /// Validate the PaymentMandate
    pub fn validate(&self) -> Result<(), A2AError> {
        // Validate mandate ID
        if self.payment_mandate_contents.payment_mandate_id.trim().is_empty() {
            return Err(A2AError::InvalidParams(
                "payment_mandate_id cannot be empty".to_string()
            ));
        }

        // Validate user authorization
        if self.user_authorization.trim().is_empty() {
            return Err(A2AError::InvalidParams(
                "user_authorization cannot be empty".to_string()
            ));
        }

        Ok(())
    }
}
```

**Deliverable**: Runtime validation for all AP2 types

---

### Phase 5: Testing

#### 5.1 Unit Tests

**File**: `a2a-rs/src/domain/extensions/ap2/mandates.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_intent_mandate_creation() {
        let expiry = Utc::now() + Duration::hours(24);
        let mandate = IntentMandate::new(
            true,
            "Purchase groceries".to_string(),
            true,
            expiry,
        );

        assert_eq!(mandate.natural_language_description, "Purchase groceries");
        assert!(mandate.validate().is_ok());
    }

    #[test]
    fn test_intent_mandate_expired() {
        let expiry = Utc::now() - Duration::hours(1);
        let mandate = IntentMandate::new(
            true,
            "Purchase groceries".to_string(),
            true,
            expiry,
        );

        assert!(mandate.validate().is_err());
    }

    #[test]
    fn test_intent_mandate_serialization() {
        let expiry = Utc::now() + Duration::hours(24);
        let mandate = IntentMandate::new(
            true,
            "Purchase groceries".to_string(),
            true,
            expiry,
        )
        .with_merchants(vec!["merchant-1".to_string()])
        .with_skus(vec!["SKU-123".to_string()]);

        let json = serde_json::to_value(&mandate).unwrap();
        assert_eq!(json["natural_language_description"], "Purchase groceries");
        assert_eq!(json["merchants"][0], "merchant-1");
    }

    // Add similar tests for CartMandate and PaymentMandate
}
```

#### 5.2 Integration Tests

**New file**: `a2a-rs/src/domain/extensions/ap2/integration_tests.rs`

Test:
- Creating messages with AP2 data parts
- Serialization/deserialization round-trip
- AgentCard with AP2 extensions
- Helper function usage

#### 5.3 Example Tests

Create example programs:
- Shopper agent sending IntentMandate
- Merchant agent responding with CartMandate
- Payment processor handling PaymentMandate

**Deliverable**: Comprehensive test coverage for AP2 functionality

---

### Phase 6: Documentation

#### 6.1 Module Documentation

Add comprehensive rustdoc to:
- All public types and functions
- Usage examples
- Security considerations
- AP2 spec references

#### 6.2 Example Implementation

**New file**: `a2a-rs/examples/ap2_payment_flow.rs`

```rust
//! Example demonstrating AP2 payment flow between agents
//!
//! This example shows:
//! 1. Creating an agent with AP2 support
//! 2. Shopper agent creating an IntentMandate
//! 3. Merchant agent responding with CartMandate
//! 4. Payment authorization with PaymentMandate

#[cfg(feature = "ap2")]
use a2a_rs::{
    IntentMandate, CartMandate, PaymentMandate,
    create_intent_part, create_cart_part, create_payment_part,
    create_ap2_extension, Message, AgentCard, AgentCapabilities,
    Ap2Role, AP2_EXTENSION_URI,
};
use chrono::{Duration, Utc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create a merchant agent with AP2 support
    let merchant_capabilities = AgentCapabilities::default()
        .with_extension(create_ap2_extension(
            vec![Ap2Role::Merchant],
            true  // Required - clients must support AP2
        ));

    let merchant_card = AgentCard::builder()
        .name("Payment Merchant Agent".to_string())
        .description("Handles payments and checkout".to_string())
        .url("https://merchant.example.com".to_string())
        .version("1.0.0".to_string())
        .capabilities(merchant_capabilities)
        .build();

    println!("Merchant card with AP2 extension: {:#?}", merchant_card);

    // 2. Create a shopper agent with AP2 support
    let shopper_capabilities = AgentCapabilities::default()
        .with_extension(create_ap2_extension(
            vec![Ap2Role::Shopper],
            false  // Optional - fallback to standard A2A if client doesn't support AP2
        ));

    // 3. Create an IntentMandate
    let intent = IntentMandate::new(
        true,  // user_cart_confirmation_required
        "Purchase organic groceries under $100".to_string(),
        true,  // required_refundability
        Utc::now() + Duration::hours(24),
    )
    .with_merchants(vec!["whole-foods".to_string(), "sprouts".to_string()])
    .with_skus(vec!["organic-apples".to_string(), "organic-milk".to_string()]);

    // 4. Create a message with the IntentMandate
    let intent_part = create_intent_part(&intent)?;
    let message = Message::builder()
        .role(Role::User)
        .parts(vec![intent_part])
        .message_id(uuid::Uuid::new_v4().to_string())
        .extensions(vec![AP2_EXTENSION_URI.to_string()])  // Mark message as using AP2
        .build();

    println!("Intent message sent: {:#?}", message);

    Ok(())
}
```

#### 6.3 Update Main README

**File**: `README.md`

Add section on extensions:

```markdown
### 🔌 Extensions

a2a-rs supports A2A protocol extensions:

#### AP2 (Agent Payments Protocol)

Enable payment capabilities between agents:

\`\`\`toml
[dependencies]
a2a-rs = { version = "0.1.0", features = ["ap2"] }
\`\`\`

See the [AP2 guide](./docs/AP2_GUIDE.md) for details.
```

#### 6.4 Create AP2 Guide

**New file**: `docs/AP2_GUIDE.md`

Comprehensive guide covering:
- What is AP2
- Use cases
- Agent roles
- Mandate types
- Security considerations
- Complete examples
- Integration with Rig (if applicable)

**Deliverable**: Complete documentation for AP2 usage

---

### Phase 7: Release Preparation

#### 7.1 CHANGELOG Update

Add entry:

```markdown
## [Unreleased]

### Added
- **A2A v0.3.0+ Extension Infrastructure**
  - Added `AgentExtension` type for declaring protocol extensions
  - Added `extensions` field to `AgentCapabilities` for extension declarations
  - Added `extensions` field to `Message` and `Artifact` for extension-specific content
  - Helper methods: `supports_extension()`, `is_extension_required()`

- **AP2 (Agent Payments Protocol) Extension Support** (#7)
  - Implemented IntentMandate, CartMandate, and PaymentMandate types
  - Added Ap2Role enum (merchant, shopper, credentials-provider, payment-processor)
  - Helper function `create_ap2_extension()` for easy AgentCard setup
  - Data part helpers: `create_intent_part()`, `create_cart_part()`, `create_payment_part()`
  - Complete W3C Payment Request API type support
  - Validation for all AP2 mandate types
  - Feature-gated with `ap2` flag
  - Comprehensive documentation and examples

### Changed
- Updated to A2A Protocol v0.3.0+ specification
- AgentCapabilities now supports protocol extensions
```

#### 7.2 GitHub Issue Response

Comment on #7 with:
- Implementation details
- How to enable the feature
- Link to documentation
- Request for feedback from Rig maintainer
- Invitation to collaborate on improvements

#### 7.3 Testing Checklist

- [ ] All tests pass with `ap2` feature enabled
- [ ] All tests pass with `ap2` feature disabled
- [ ] Examples compile and run
- [ ] Documentation builds without warnings
- [ ] Backward compatibility maintained
- [ ] No breaking changes to existing API

**Deliverable**: Ready for release

---

## Technical Decisions

### Why Feature-Gated?

1. **Opt-in adoption** - Not all users need payment functionality
2. **Dependency management** - Keeps core library lean
3. **Spec stability** - Easy to deprecate if AP2 changes significantly
4. **Clear boundaries** - Separates extension code from core

### Why Not a Separate Crate?

1. **User experience** - Single dependency with features is simpler
2. **Shared types** - Reuses Part, Message, etc.
3. **Maintenance** - Easier to keep in sync with core
4. **Discoverability** - Users find it through main crate

### Extension Pattern

This implementation establishes a pattern for future extensions:
1. Add to `domain/extensions/<extension-name>/`
2. Feature gate with extension name
3. Provide helper functions
4. Document thoroughly

## Open Questions

1. **Signature verification** - Should we implement cryptographic verification of merchant_signature and user_authorization?
2. **Amount precision** - Should we use a decimal library like `rust_decimal` for amounts?
3. **Risk data structure** - Should we define a standard RiskData type or keep it flexible?
4. **Rig integration** - Should we coordinate with Rig maintainer on API design?

## Success Criteria

- [ ] AP2 extension can be enabled with feature flag
- [ ] All mandate types can be created, validated, and serialized
- [ ] AgentCard can declare AP2 support and roles
- [ ] Helper functions make AP2 usage ergonomic
- [ ] Documentation is comprehensive and accurate
- [ ] Tests provide good coverage
- [ ] Rig maintainer confirms it meets their needs
- [ ] No breaking changes to existing a2a-rs API

## Timeline Estimate

- Phase 1 (Extension Infrastructure): 2-3 hours
- Phase 2 (AP2 Types): 4-6 hours
- Phase 3 (Feature Gates): 1 hour
- Phase 4 (Validation): 2-3 hours
- Phase 5 (Testing): 4-5 hours
- Phase 6 (Documentation): 3-4 hours
- Phase 7 (Release Prep): 1-2 hours

**Total**: 17-24 hours of development time

## References

- [GitHub Issue #7](https://github.com/EmilLindfors/a2a-rs/issues/7)
- [AP2 Specification](https://github.com/google-agentic-commerce/AP2/blob/main/docs/a2a-extension.md)
- [A2A Protocol v0.3.0 Spec (new)](./spec/specification_new.json) - **Contains official AgentExtension structure**
- [A2A Protocol Spec (legacy)](./spec/specification.json)
- [A2A Protocol Spec (split)](https://github.com/EmilLindfors/a2a-rs/tree/master/spec)
- [Rig Framework](https://github.com/0xPlaygrounds/rig)

---

## Update: Official Extension Structure

**Date**: 2025-10-21

This implementation plan has been updated to use the official `AgentExtension` structure from the A2A v0.3.0+ specification (specification_new.json). Key changes:

1. **AgentExtension Type**: Instead of simple string arrays for extensions and roles, we use the structured `AgentExtension` type with:
   - `uri`: Extension identifier
   - `required`: Whether clients must support this extension
   - `description`: Human-readable description
   - `params`: Extension-specific parameters (AP2 uses this for roles)

2. **Location**: Extensions are declared in `AgentCapabilities.extensions`, not directly on AgentCard

3. **AP2 Roles**: Stored in `AgentExtension.params.roles` as an array, following the official pattern

This approach is:
- ✅ **Spec-compliant**: Matches A2A v0.3.0+ specification exactly
- ✅ **Extensible**: Works for any future extensions, not just AP2
- ✅ **Type-safe**: Full Rust type safety with serde support
- ✅ **Flexible**: `params` field allows each extension to define its own configuration

The implementation maintains backward compatibility with agents that don't declare extensions.
