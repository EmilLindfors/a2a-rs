//! The decision adapter over a socket: the request a host receives, and what
//! the adapter makes of the replies the documented hosts send.

use a2a_llm::decision::{
    Answer, DecisionEndpoint, DecisionError, DecisionModel, DecisionProvider, HttpDecisionModel,
    Questions,
};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn model(server: &MockServer, api_key: Option<&str>) -> HttpDecisionModel {
    HttpDecisionModel::new(DecisionEndpoint {
        provider: DecisionProvider::SystemOne,
        model: "typesafe/jev-1.13".to_string(),
        url: format!("{}/v1/systemone", server.uri()),
        api_key: api_key.map(str::to_string),
        key_from: api_key.map(|_| "config"),
    })
    .expect("the HTTP client builds")
}

fn triage() -> Questions {
    serde_json::from_value(json!({
        "refund": {"type": "noul", "instructions": "the customer asks for their money back"},
        "team": {"type": "choice", "instructions": "who should handle this ticket",
                 "criteria": {"billing": "invoices and payments", "technical": "bugs"}},
        "urgency": {"type": "score", "instructions": "how soon this needs an answer",
                    "criteria": ["this week", "today", "now"]},
    }))
    .unwrap()
}

/// The response in the shape OpenRouter's reference documents, with a field
/// only Mapika's server sends.
#[tokio::test]
async fn the_request_is_the_wire_format_and_the_answers_come_back_typed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(header("authorization", "Bearer sk-test"))
        .and(body_json(json!({
            "model": "typesafe/jev-1.13",
            "state": "I was charged twice, please refund one of them",
            "questions": {
                "refund": {"type": "noul", "instructions": "the customer asks for their money back"},
                "team": {"type": "choice", "instructions": "who should handle this ticket",
                         "criteria": {"billing": "invoices and payments", "technical": "bugs"}},
                "urgency": {"type": "score", "instructions": "how soon this needs an answer",
                            "criteria": ["this week", "today", "now"]},
            },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "dec-1",
            "model": "typesafe/jev-1.13-20260917",
            "provider": "TypeSafe",
            "answers": {
                "refund": {"type": "noul", "noul": 0.99},
                "team": {"type": "choice", "choice": "billing", "confidence": 0.97,
                         "probabilities": {"billing": 0.99, "technical": 0.01},
                         "certainty": 0.98},
                "urgency": {"type": "score", "score": 1.15, "confidence": 0.6,
                            "legend": {"0": "this week", "1": "today", "2": "now"},
                            "probabilities": {"0": 0.0, "1": 0.85, "2": 0.15}},
            },
            "usage": {"input_tokens": 212, "output_tokens": 0, "cost": 0.0000089},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let decision = model(&server, Some("sk-test"))
        .decide(
            &json!("I was charged twice, please refund one of them"),
            &triage(),
        )
        .await
        .expect("the decision is had");

    assert_eq!(decision.model, "typesafe/jev-1.13-20260917");
    assert_eq!(decision.answers["refund"], Answer::Noul { noul: 0.99 });
    assert!(matches!(
        &decision.answers["team"],
        Answer::Choice { choice, .. } if choice == "billing"
    ));
    assert!(matches!(
        decision.answers["urgency"],
        Answer::Score { score, .. } if (score - 1.15).abs() < 1e-9
    ));
    assert_eq!(decision.usage.expect("usage is reported").input_tokens, 212);
}

#[tokio::test]
async fn a_refusal_carries_the_hosts_own_words() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {"code": 400, "message": "noul requires criteria with 'true' and 'false'"},
        })))
        .mount(&server)
        .await;

    let err = model(&server, None)
        .decide(&json!("x"), &triage())
        .await
        .unwrap_err();
    match err {
        DecisionError::Refused { status, message } => {
            assert_eq!(status, 400);
            assert_eq!(message, "noul requires criteria with 'true' and 'false'");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A host that answers 200 with one question missing has not answered what
/// was asked, and a caller must hear that rather than find the key absent.
#[tokio::test]
async fn a_reply_that_drops_a_question_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "m",
            "answers": {
                "refund": {"type": "noul", "noul": 0.2},
                "team": {"type": "choice", "choice": "technical"},
            },
            "usage": {"input_tokens": 10, "output_tokens": 0},
        })))
        .mount(&server)
        .await;

    let err = model(&server, None)
        .decide(&json!("x"), &triage())
        .await
        .unwrap_err();
    assert!(
        matches!(&err, DecisionError::Mismatch(why) if why.contains("urgency")),
        "{err:?}"
    );
}
