//! What the providers make of responses seen from real endpoints, pinned over
//! a socket so the whole path — request, SSE framing, chunk parsing, event
//! shaping — is the one a caller gets.

use a2a_llm::gemini::{GeminiConfig, GeminiProvider};
use a2a_llm::openai::{OpenAiConfig, OpenAiProvider, ReasoningDialect};
use a2a_llm::{ChatMessage, FinishReason, LlmProvider, LlmRequest, LlmStreamEvent};
use futures::StreamExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn openai(server: &MockServer) -> OpenAiProvider {
    OpenAiProvider::new(OpenAiConfig {
        base_url: format!("{}/v1", server.uri()),
        model: "test-model".to_string(),
        api_key: None,
        extra_headers: Vec::new(),
        reasoning_dialect: ReasoningDialect::OpenRouter,
        reasoning: None,
        stream_usage: true,
    })
    .expect("the HTTP client builds")
}

fn gemini(server: &MockServer) -> GeminiProvider {
    GeminiProvider::new(GeminiConfig {
        base_url: format!("{}/v1beta/models", server.uri()),
        model: "test-model".to_string(),
        api_key: "key".to_string(),
        reasoning: None,
    })
    .expect("the HTTP client builds")
}

/// OpenRouter puts `finish_reason: "stop"` on the last content chunk *and* on
/// the usage-only chunk after it. Seen from korps' live runs on 2026-09-02: a
/// consumer that counted finishes, or took the first as the end of the stream,
/// would have been wrong either way. One fact, reported once.
#[tokio::test]
async fn a_repeated_finish_reason_is_reported_once() {
    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],",
        "\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":12}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .mount(&server)
        .await;

    let stream = openai(&server)
        .chat_completion_stream(LlmRequest::new(vec![ChatMessage::user("hi")]))
        .await
        .expect("the stream opens");
    let events: Vec<LlmStreamEvent> = stream.map(|e| e.expect("no event fails")).collect().await;

    let finishes: Vec<&FinishReason> = events
        .iter()
        .filter_map(|e| match e {
            LlmStreamEvent::Finish(reason) => Some(reason),
            _ => None,
        })
        .collect();
    assert_eq!(finishes, vec![&FinishReason::Stop], "events: {events:?}");

    let content: String = events
        .iter()
        .filter_map(|e| match e {
            LlmStreamEvent::ContentChunk(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(content, "hello world");

    let usage = events.iter().find_map(|e| match e {
        LlmStreamEvent::Usage(usage) => Some(usage),
        _ => None,
    });
    assert_eq!(
        usage.and_then(|u| u.completion_tokens),
        Some(2),
        "the usage-only chunk still reports usage: {events:?}"
    );
}

/// A candidate cut before any content arrived — the whole output budget spent
/// on thinking — carries `finishReason: "MAX_TOKENS"` and no `content`. That
/// is an empty answer whose emptiness is explained, and the explanation has to
/// reach the caller: it names the output-cap knob, where a provider error
/// names nothing.
#[tokio::test]
async fn an_empty_gemini_candidate_keeps_its_finish_reason() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/test-model:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"candidates":[{"finishReason":"MAX_TOKENS","index":0}],
                "usageMetadata":{"promptTokenCount":12,"totalTokenCount":1036,"thoughtsTokenCount":1024}}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let response = gemini(&server)
        .chat_completion(LlmRequest::new(vec![ChatMessage::user("hi")]).max_tokens(1024))
        .await
        .expect("a cut answer is an answer, not a provider fault");

    assert_eq!(response.finish, Some(FinishReason::Length));
    assert_eq!(response.content, None);
    assert!(response.tool_calls.is_none());
    assert_eq!(
        response.usage.and_then(|u| u.reasoning_tokens),
        Some(1024),
        "the usage that explains the cut comes with it"
    );
}

/// No content and no reason is still a provider fault: nothing in the
/// response says what happened, so nothing can be returned as the answer.
#[tokio::test]
async fn a_gemini_candidate_with_nothing_in_it_is_still_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/test-model:generateContent"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(r#"{"candidates":[{"index":0}]}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let err = gemini(&server)
        .chat_completion(LlmRequest::new(vec![ChatMessage::user("hi")]))
        .await
        .expect_err("a truly empty candidate fails");
    assert!(
        matches!(err, a2a_llm::LlmError::ProviderError(ref m) if m.contains("No content")),
        "{err}"
    );
}
