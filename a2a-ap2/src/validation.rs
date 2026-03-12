//! Validation for AP2 types.

use crate::error::{Ap2Error, Result};
use crate::types::{
    CartContents, CartMandate, IntentMandate, PaymentCurrencyAmount, PaymentDetailsInit,
    PaymentItem, PaymentMandate, PaymentMandateContents, PaymentMethodData, PaymentReceipt,
    PaymentRequest, PaymentResponse, PaymentShippingOption,
};

/// Trait for validating AP2 types.
pub trait Validate {
    /// Check this value for structural correctness.
    fn validate(&self) -> Result<()>;
}

impl Validate for PaymentCurrencyAmount {
    fn validate(&self) -> Result<()> {
        if self.currency.len() != 3 || !self.currency.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(Ap2Error::ValidationError {
                field: "currency".into(),
                message: format!("must be 3-letter ISO 4217 code, got {:?}", self.currency),
            });
        }
        Ok(())
    }
}

impl Validate for PaymentItem {
    fn validate(&self) -> Result<()> {
        if self.label.trim().is_empty() {
            return Err(Ap2Error::MissingField("PaymentItem.label".into()));
        }
        self.amount.validate()?;
        if self.refund_period < 0 {
            return Err(Ap2Error::ValidationError {
                field: "refund_period".into(),
                message: "must be non-negative".into(),
            });
        }
        Ok(())
    }
}

impl Validate for PaymentShippingOption {
    fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            return Err(Ap2Error::MissingField("PaymentShippingOption.id".into()));
        }
        if self.label.trim().is_empty() {
            return Err(Ap2Error::MissingField("PaymentShippingOption.label".into()));
        }
        self.amount.validate()
    }
}

impl Validate for PaymentMethodData {
    fn validate(&self) -> Result<()> {
        if self.supported_methods.trim().is_empty() {
            return Err(Ap2Error::MissingField(
                "PaymentMethodData.supported_methods".into(),
            ));
        }
        Ok(())
    }
}

impl Validate for PaymentDetailsInit {
    fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            return Err(Ap2Error::MissingField("PaymentDetailsInit.id".into()));
        }
        self.total.validate()?;
        for item in &self.display_items {
            item.validate()?;
        }
        Ok(())
    }
}

impl Validate for PaymentRequest {
    fn validate(&self) -> Result<()> {
        if self.method_data.is_empty() {
            return Err(Ap2Error::MissingField("PaymentRequest.method_data".into()));
        }
        for md in &self.method_data {
            md.validate()?;
        }
        self.details.validate()
    }
}

impl Validate for PaymentResponse {
    fn validate(&self) -> Result<()> {
        if self.request_id.trim().is_empty() {
            return Err(Ap2Error::MissingField("PaymentResponse.request_id".into()));
        }
        if self.method_name.trim().is_empty() {
            return Err(Ap2Error::MissingField("PaymentResponse.method_name".into()));
        }
        Ok(())
    }
}

impl Validate for IntentMandate {
    fn validate(&self) -> Result<()> {
        if self.natural_language_description.trim().is_empty() {
            return Err(Ap2Error::MissingField(
                "IntentMandate.natural_language_description".into(),
            ));
        }
        if self.intent_expiry.trim().is_empty() {
            return Err(Ap2Error::MissingField("IntentMandate.intent_expiry".into()));
        }
        Ok(())
    }
}

impl Validate for CartContents {
    fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            return Err(Ap2Error::MissingField("CartContents.id".into()));
        }
        if self.cart_expiry.trim().is_empty() {
            return Err(Ap2Error::MissingField("CartContents.cart_expiry".into()));
        }
        if self.merchant_name.trim().is_empty() {
            return Err(Ap2Error::MissingField("CartContents.merchant_name".into()));
        }
        self.payment_request.validate()
    }
}

impl Validate for CartMandate {
    fn validate(&self) -> Result<()> {
        self.contents.validate()
    }
}

impl Validate for PaymentMandateContents {
    fn validate(&self) -> Result<()> {
        if self.payment_mandate_id.trim().is_empty() {
            return Err(Ap2Error::MissingField(
                "PaymentMandateContents.payment_mandate_id".into(),
            ));
        }
        if self.payment_details_id.trim().is_empty() {
            return Err(Ap2Error::MissingField(
                "PaymentMandateContents.payment_details_id".into(),
            ));
        }
        if self.merchant_agent.trim().is_empty() {
            return Err(Ap2Error::MissingField(
                "PaymentMandateContents.merchant_agent".into(),
            ));
        }
        if self.timestamp.trim().is_empty() {
            return Err(Ap2Error::MissingField(
                "PaymentMandateContents.timestamp".into(),
            ));
        }
        self.payment_details_total.validate()?;
        self.payment_response.validate()
    }
}

impl Validate for PaymentMandate {
    fn validate(&self) -> Result<()> {
        self.payment_mandate_contents.validate()
    }
}

impl Validate for PaymentReceipt {
    fn validate(&self) -> Result<()> {
        if self.payment_mandate_id.trim().is_empty() {
            return Err(Ap2Error::MissingField(
                "PaymentReceipt.payment_mandate_id".into(),
            ));
        }
        if self.payment_id.trim().is_empty() {
            return Err(Ap2Error::MissingField("PaymentReceipt.payment_id".into()));
        }
        if self.timestamp.trim().is_empty() {
            return Err(Ap2Error::MissingField("PaymentReceipt.timestamp".into()));
        }
        self.amount.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PaymentCurrencyAmount, PaymentStatus, Success};

    #[test]
    fn valid_currency() {
        let amt = PaymentCurrencyAmount {
            currency: "USD".into(),
            value: 10.0,
        };
        assert!(amt.validate().is_ok());
    }

    #[test]
    fn invalid_currency() {
        let amt = PaymentCurrencyAmount {
            currency: "us".into(),
            value: 10.0,
        };
        assert!(amt.validate().is_err());
    }

    #[test]
    fn empty_label_fails() {
        let item = PaymentItem {
            label: "".into(),
            amount: PaymentCurrencyAmount {
                currency: "USD".into(),
                value: 1.0,
            },
            pending: None,
            refund_period: 30,
        };
        assert!(item.validate().is_err());
    }

    #[test]
    fn valid_intent_mandate() {
        let im = IntentMandate {
            user_cart_confirmation_required: true,
            natural_language_description: "Buy shoes".into(),
            merchants: None,
            skus: None,
            requires_refundability: None,
            intent_expiry: "2026-12-31T23:59:59Z".into(),
        };
        assert!(im.validate().is_ok());
    }

    #[test]
    fn empty_description_fails() {
        let im = IntentMandate {
            user_cart_confirmation_required: true,
            natural_language_description: "  ".into(),
            merchants: None,
            skus: None,
            requires_refundability: None,
            intent_expiry: "2026-12-31T23:59:59Z".into(),
        };
        assert!(im.validate().is_err());
    }

    #[test]
    fn valid_receipt() {
        let receipt = PaymentReceipt {
            payment_mandate_id: "pm_1".into(),
            timestamp: "2025-09-16T12:00:00Z".into(),
            payment_id: "pay_1".into(),
            amount: PaymentCurrencyAmount {
                currency: "EUR".into(),
                value: 50.0,
            },
            payment_status: PaymentStatus::Success(Success {
                merchant_confirmation_id: "mc_1".into(),
                psp_confirmation_id: None,
                network_confirmation_id: None,
            }),
            payment_method_details: None,
        };
        assert!(receipt.validate().is_ok());
    }
}
