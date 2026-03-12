//! Demonstrates the complete AP2 payment flow using A2A protocol messages.
//!
//! Steps:
//! 1. Shopper agent creates an IntentMandate → A2A Message
//! 2. Merchant agent responds with a CartMandate → A2A Artifact
//! 3. Shopper creates a PaymentMandate → A2A Message
//! 4. Payment processor returns a PaymentReceipt

use a2a_ap2::{
    // Types
    Ap2Role, CartContents, CartMandate, IntentMandate, PaymentCurrencyAmount, PaymentDetailsInit,
    PaymentItem, PaymentMandate, PaymentMandateContents, PaymentMethodData, PaymentOptions,
    PaymentReceipt, PaymentRequest, PaymentResponse, PaymentStatus, Success,
    // Helpers
    ap2_extension, cart_mandate_artifact, find_cart_mandate, find_intent_mandate,
    find_payment_mandate, intent_mandate_message, payment_mandate_message, payment_receipt_to_part,
    // Validation
    Validate,
};

fn main() {
    println!("=== AP2 Payment Flow Demo ===\n");

    // -----------------------------------------------------------------------
    // Step 0: Agent cards declare AP2 support
    // -----------------------------------------------------------------------
    let shopper_ext = ap2_extension(vec![Ap2Role::Shopper], false);
    let merchant_ext = ap2_extension(vec![Ap2Role::Merchant], true);
    println!(
        "Shopper extension:\n{}\n",
        serde_json::to_string_pretty(&shopper_ext).unwrap()
    );
    println!(
        "Merchant extension:\n{}\n",
        serde_json::to_string_pretty(&merchant_ext).unwrap()
    );

    // -----------------------------------------------------------------------
    // Step 1: Shopper creates an IntentMandate
    // -----------------------------------------------------------------------
    let intent = IntentMandate {
        user_cart_confirmation_required: true,
        natural_language_description: "Cool red basketball shoes, size 10, under $150".into(),
        merchants: Some(vec!["nike-store".into(), "adidas-store".into()]),
        skus: None,
        requires_refundability: Some(true),
        intent_expiry: "2026-12-31T23:59:59Z".into(),
    };
    intent.validate().expect("intent should be valid");

    let intent_msg = intent_mandate_message(&intent, "msg-intent-001".into())
        .expect("should create intent message");
    println!(
        "1. IntentMandate Message:\n{}\n",
        serde_json::to_string_pretty(&intent_msg).unwrap()
    );

    // Verify round-trip
    let extracted_intent = find_intent_mandate(&intent_msg)
        .expect("extraction should succeed")
        .expect("should find intent");
    assert_eq!(intent, extracted_intent);

    // -----------------------------------------------------------------------
    // Step 2: Merchant responds with a CartMandate
    // -----------------------------------------------------------------------
    let cart = CartMandate {
        contents: CartContents {
            id: "cart_shoes_456".into(),
            user_cart_confirmation_required: true,
            payment_request: PaymentRequest {
                method_data: vec![PaymentMethodData {
                    supported_methods: "CARD".into(),
                    data: Some(
                        [("payment_processor_url".to_string(), serde_json::json!("https://pay.example.com"))]
                            .into_iter()
                            .collect(),
                    ),
                }],
                details: PaymentDetailsInit {
                    id: "order_shoes_456".into(),
                    display_items: vec![PaymentItem {
                        label: "Nike Air Max Red - Size 10".into(),
                        amount: PaymentCurrencyAmount {
                            currency: "USD".into(),
                            value: 129.99,
                        },
                        pending: None,
                        refund_period: 30,
                    }],
                    shipping_options: None,
                    modifiers: None,
                    total: PaymentItem {
                        label: "Total".into(),
                        amount: PaymentCurrencyAmount {
                            currency: "USD".into(),
                            value: 129.99,
                        },
                        pending: None,
                        refund_period: 30,
                    },
                },
                options: Some(PaymentOptions {
                    request_payer_name: Some(true),
                    request_payer_email: Some(true),
                    request_payer_phone: Some(false),
                    request_shipping: Some(true),
                    shipping_type: Some("shipping".into()),
                }),
                shipping_address: None,
            },
            cart_expiry: "2026-01-15T12:00:00Z".into(),
            merchant_name: "Nike Store".into(),
        },
        merchant_authorization: Some("eyJhbGciOiJSUzI1NiJ9.eyJjYXJ0X2hhc2giOiIuLi4ifQ.signature".into()),
    };
    cart.validate().expect("cart should be valid");

    let cart_artifact = cart_mandate_artifact(&cart, "artifact-cart-001".into(), Some("Shopping Cart".into()))
        .expect("should create cart artifact");
    println!(
        "2. CartMandate Artifact:\n{}\n",
        serde_json::to_string_pretty(&cart_artifact).unwrap()
    );

    let extracted_cart = find_cart_mandate(&cart_artifact)
        .expect("extraction should succeed")
        .expect("should find cart");
    assert_eq!(cart, extracted_cart);

    // -----------------------------------------------------------------------
    // Step 3: Shopper creates a PaymentMandate
    // -----------------------------------------------------------------------
    let payment = PaymentMandate {
        payment_mandate_contents: PaymentMandateContents {
            payment_mandate_id: "pm_789".into(),
            payment_details_id: "order_shoes_456".into(),
            payment_details_total: PaymentItem {
                label: "Total".into(),
                amount: PaymentCurrencyAmount {
                    currency: "USD".into(),
                    value: 129.99,
                },
                pending: None,
                refund_period: 30,
            },
            payment_response: PaymentResponse {
                request_id: "order_shoes_456".into(),
                method_name: "CARD".into(),
                details: Some(
                    [("token".to_string(), serde_json::json!("tok_visa_4242"))]
                        .into_iter()
                        .collect(),
                ),
                shipping_address: None,
                shipping_option: None,
                payer_name: Some("Alice Smith".into()),
                payer_email: Some("alice@example.com".into()),
                payer_phone: None,
            },
            merchant_agent: "nike-store-agent".into(),
            timestamp: "2026-01-10T14:30:00Z".into(),
        },
        user_authorization: Some("eyJhbGciOiJFUzI1NiJ9...vdc_presentation".into()),
    };
    payment.validate().expect("payment should be valid");

    let payment_msg = payment_mandate_message(&payment, "msg-payment-001".into())
        .expect("should create payment message");
    println!(
        "3. PaymentMandate Message:\n{}\n",
        serde_json::to_string_pretty(&payment_msg).unwrap()
    );

    let extracted_payment = find_payment_mandate(&payment_msg)
        .expect("extraction should succeed")
        .expect("should find payment");
    assert_eq!(payment, extracted_payment);

    // -----------------------------------------------------------------------
    // Step 4: Payment processor returns a PaymentReceipt
    // -----------------------------------------------------------------------
    let receipt = PaymentReceipt {
        payment_mandate_id: "pm_789".into(),
        timestamp: "2026-01-10T14:30:05Z".into(),
        payment_id: "pay_confirmed_001".into(),
        amount: PaymentCurrencyAmount {
            currency: "USD".into(),
            value: 129.99,
        },
        payment_status: PaymentStatus::Success(Success {
            merchant_confirmation_id: "mc_nike_001".into(),
            psp_confirmation_id: Some("psp_stripe_001".into()),
            network_confirmation_id: Some("visa_auth_001".into()),
        }),
        payment_method_details: None,
    };
    receipt.validate().expect("receipt should be valid");

    let receipt_part = payment_receipt_to_part(&receipt).expect("should create receipt part");
    println!(
        "4. PaymentReceipt Part:\n{}\n",
        serde_json::to_string_pretty(&receipt_part).unwrap()
    );

    println!("=== Payment flow completed successfully! ===");
}
