//! AP2 Merchant Agent Example
//!
//! Demonstrates a merchant agent that handles the AP2 (Agent Payments Protocol) flow:
//!
//! 1. Receives an **IntentMandate** from a shopper agent ("find red shoes under $100")
//! 2. Responds with a **CartMandate** artifact containing matching products + payment details
//! 3. Receives a **PaymentMandate** with the shopper's chosen payment method
//! 4. Returns a **PaymentReceipt** confirming the transaction
//!
//! If a plain text message is received (no AP2 mandate), the agent responds with
//! a catalog listing.
//!
//! Run with:
//!   cargo run --example ap2_merchant_agent --features ap2

use a2a_agents::core::{AgentBuilder, BuildError};
use a2a_ap2::{
    find_intent_mandate, find_payment_mandate,
    cart_mandate_artifact, payment_receipt_to_part,
    CartContents, CartMandate,
    PaymentCurrencyAmount, PaymentDetailsInit, PaymentItem, PaymentMethodData,
    PaymentOptions, PaymentReceipt, PaymentRequest, PaymentStatus, Success,
    Validate,
};
use a2a_rs::{
    domain::{A2AError, Artifact, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
    InMemoryTaskStorage,
};
use async_trait::async_trait;
use uuid::Uuid;

/// A simple product in our catalog.
struct Product {
    sku: &'static str,
    name: &'static str,
    price: f64,
    description: &'static str,
}

/// Hardcoded shoe catalog for the demo.
const CATALOG: &[Product] = &[
    Product {
        sku: "SHOE-001",
        name: "Classic Red Sneakers",
        price: 79.99,
        description: "Comfortable everyday sneakers in vibrant red",
    },
    Product {
        sku: "SHOE-002",
        name: "Blue Running Shoes",
        price: 119.99,
        description: "Lightweight performance running shoes",
    },
    Product {
        sku: "SHOE-003",
        name: "Black Leather Boots",
        price: 149.99,
        description: "Durable leather boots for all weather",
    },
    Product {
        sku: "SHOE-004",
        name: "White Canvas Slip-Ons",
        price: 49.99,
        description: "Casual slip-on shoes in classic white canvas",
    },
];

/// Merchant handler that processes AP2 payment flows.
#[derive(Clone)]
struct MerchantHandler;

impl MerchantHandler {
    /// Build a CartMandate from matching products.
    fn build_cart(
        &self,
        products: &[&Product],
        cart_id: &str,
    ) -> Result<CartMandate, A2AError> {
        let display_items: Vec<PaymentItem> = products
            .iter()
            .map(|p| PaymentItem {
                label: p.name.to_string(),
                amount: PaymentCurrencyAmount {
                    currency: "USD".to_string(),
                    value: p.price,
                },
                pending: None,
                refund_period: 30,
            })
            .collect();

        let total_value: f64 = products.iter().map(|p| p.price).sum();

        let cart = CartMandate {
            contents: CartContents {
                id: cart_id.to_string(),
                user_cart_confirmation_required: true,
                payment_request: PaymentRequest {
                    method_data: vec![
                        PaymentMethodData {
                            supported_methods: "CARD".to_string(),
                            data: None,
                        },
                        PaymentMethodData {
                            supported_methods: "google-pay".to_string(),
                            data: None,
                        },
                    ],
                    details: PaymentDetailsInit {
                        id: format!("order-{}", cart_id),
                        display_items,
                        shipping_options: None,
                        modifiers: None,
                        total: PaymentItem {
                            label: "Order Total".to_string(),
                            amount: PaymentCurrencyAmount {
                                currency: "USD".to_string(),
                                value: total_value,
                            },
                            pending: None,
                            refund_period: 30,
                        },
                    },
                    options: Some(PaymentOptions {
                        request_payer_name: Some(true),
                        request_payer_email: Some(true),
                        request_payer_phone: None,
                        request_shipping: Some(true),
                        shipping_type: Some("shipping".to_string()),
                    }),
                    shipping_address: None,
                },
                cart_expiry: "2026-12-31T23:59:59Z".to_string(),
                merchant_name: "AP2 Shoe Store".to_string(),
            },
            merchant_authorization: None,
        };

        cart.contents.validate().map_err(|e| A2AError::InvalidRequest(e.to_string()))?;
        Ok(cart)
    }

    /// Match products from catalog based on the shopper's intent description.
    fn match_products(&self, description: &str) -> Vec<&Product> {
        let desc_lower = description.to_lowercase();

        // Extract budget if mentioned (e.g., "under $100")
        let budget: Option<f64> = desc_lower
            .find("under $")
            .or_else(|| desc_lower.find("under $"))
            .and_then(|pos| {
                let after = &desc_lower[pos + 7..];
                after
                    .split(|c: char| !c.is_ascii_digit() && c != '.')
                    .next()
                    .and_then(|s| s.parse().ok())
            });

        CATALOG
            .iter()
            .filter(|p| {
                let name_lower = p.name.to_lowercase();
                let desc_match = desc_lower.split_whitespace().any(|word| {
                    name_lower.contains(word)
                        || p.description.to_lowercase().contains(word)
                });
                let within_budget = budget.map(|b| p.price < b).unwrap_or(true);
                desc_match && within_budget
            })
            .collect()
    }

    /// Format catalog as a text listing.
    fn catalog_text(&self) -> String {
        let mut text = String::from("Welcome to the AP2 Shoe Store! Here's our catalog:\n\n");
        for p in CATALOG {
            text.push_str(&format!(
                "  {} - {} (${:.2})\n    {}\n\n",
                p.sku, p.name, p.price, p.description
            ));
        }
        text.push_str(
            "To purchase, send an AP2 IntentMandate describing what you'd like, \
             or tell me what you're looking for!",
        );
        text
    }
}

#[async_trait]
impl AsyncMessageHandler for MerchantHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let context_id = message.context_id.clone().unwrap_or_default();

        // --- Try AP2 IntentMandate flow ---
        if let Some(intent) = find_intent_mandate(message)
            .map_err(|e| A2AError::InvalidRequest(e.to_string()))?
        {
            tracing::info!(
                task_id,
                description = %intent.natural_language_description,
                "Received IntentMandate"
            );

            let matched = self.match_products(&intent.natural_language_description);

            if matched.is_empty() {
                let response = Message::builder()
                    .role(Role::Agent)
                    .parts(vec![Part::text(
                        "Sorry, no products matched your request. Try browsing our catalog!".to_string(),
                    )])
                    .message_id(Uuid::new_v4().to_string())
                    .context_id(context_id.clone())
                    .build();

                return Ok(Task::builder()
                    .id(task_id.to_string())
                    .context_id(context_id)
                    .status(TaskStatus {
                        state: TaskState::Completed,
                        message: Some(response.clone()),
                        timestamp: Some(chrono::Utc::now()),
                    })
                    .history(vec![message.clone(), response])
                    .build());
            }

            // Build cart from matched products
            let cart_id = Uuid::new_v4().to_string();
            let cart = self.build_cart(&matched, &cart_id)?;

            let cart_artifact = cart_mandate_artifact(
                &cart,
                Uuid::new_v4().to_string(),
                Some("Shopping Cart".to_string()),
            )
            .map_err(|e| A2AError::InvalidRequest(e.to_string()))?;

            let summary = format!(
                "Found {} item(s) matching your request. Total: ${:.2}.\n\
                 Please review the cart artifact and send a PaymentMandate to complete the purchase.",
                matched.len(),
                matched.iter().map(|p| p.price).sum::<f64>(),
            );

            let response = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text(summary)])
                .message_id(Uuid::new_v4().to_string())
                .context_id(context_id.clone())
                .build();

            return Ok(Task::builder()
                .id(task_id.to_string())
                .context_id(context_id)
                .status(TaskStatus {
                    state: TaskState::InputRequired,
                    message: Some(response.clone()),
                    timestamp: Some(chrono::Utc::now()),
                })
                .history(vec![message.clone(), response])
                .artifacts(vec![cart_artifact])
                .build());
        }

        // --- Try AP2 PaymentMandate flow ---
        if let Some(payment) = find_payment_mandate(message)
            .map_err(|e| A2AError::InvalidRequest(e.to_string()))?
        {
            tracing::info!(
                task_id,
                mandate_id = %payment.payment_mandate_contents.payment_mandate_id,
                "Received PaymentMandate — processing payment"
            );

            let receipt = PaymentReceipt {
                payment_mandate_id: payment
                    .payment_mandate_contents
                    .payment_mandate_id
                    .clone(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payment_id: format!("pay-{}", Uuid::new_v4()),
                amount: payment
                    .payment_mandate_contents
                    .payment_details_total
                    .amount
                    .clone(),
                payment_status: PaymentStatus::Success(Success {
                    merchant_confirmation_id: format!("mc-{}", Uuid::new_v4()),
                    psp_confirmation_id: Some(format!("psp-{}", Uuid::new_v4())),
                    network_confirmation_id: None,
                }),
                payment_method_details: None,
            };

            receipt
                .validate()
                .map_err(|e| A2AError::InvalidRequest(e.to_string()))?;

            let receipt_part = payment_receipt_to_part(&receipt)
                .map_err(|e| A2AError::InvalidRequest(e.to_string()))?;

            let response = Message::builder()
                .role(Role::Agent)
                .parts(vec![
                    Part::text(format!(
                        "Payment of ${:.2} {} processed successfully! \
                         Confirmation: {}",
                        receipt.amount.value,
                        receipt.amount.currency,
                        match &receipt.payment_status {
                            PaymentStatus::Success(s) =>
                                s.merchant_confirmation_id.clone(),
                            _ => "N/A".to_string(),
                        },
                    )),
                    receipt_part,
                ])
                .message_id(Uuid::new_v4().to_string())
                .context_id(context_id.clone())
                .build();

            let receipt_artifact = Artifact {
                artifact_id: Uuid::new_v4().to_string(),
                name: Some("Payment Receipt".to_string()),
                description: None,
                parts: response.parts.clone(),
                metadata: None,
                extensions: None,
            };

            return Ok(Task::builder()
                .id(task_id.to_string())
                .context_id(context_id)
                .status(TaskStatus {
                    state: TaskState::Completed,
                    message: Some(response.clone()),
                    timestamp: Some(chrono::Utc::now()),
                })
                .history(vec![message.clone(), response])
                .artifacts(vec![receipt_artifact])
                .build());
        }

        // --- Plain text: show catalog ---
        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(self.catalog_text())])
            .message_id(Uuid::new_v4().to_string())
            .context_id(context_id.clone())
            .build();

        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id(context_id)
            .status(TaskStatus {
                state: TaskState::Completed,
                message: Some(response.clone()),
                timestamp: Some(chrono::Utc::now()),
            })
            .history(vec![message.clone(), response])
            .build())
    }
}

#[tokio::main]
async fn main() -> Result<(), BuildError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    println!("AP2 Merchant Agent - Shoe Store Demo");
    println!("=====================================");
    println!();
    println!("This agent implements the AP2 payment protocol flow:");
    println!("  1. Send a text message to browse the catalog");
    println!("  2. Send an IntentMandate to search for products");
    println!("  3. Review the CartMandate artifact returned");
    println!("  4. Send a PaymentMandate to complete the purchase");
    println!("  5. Receive a PaymentReceipt confirming the transaction");
    println!();

    AgentBuilder::from_file("examples/ap2_merchant.toml")?
        .with_handler(MerchantHandler)
        .with_storage(InMemoryTaskStorage::new())
        .build()?
        .run()
        .await
        .map_err(|e| BuildError::RuntimeError(e.to_string()))?;

    Ok(())
}
