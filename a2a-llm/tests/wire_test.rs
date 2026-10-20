//! What the providers make of responses seen from real endpoints, pinned over
//! a socket so the whole path — request, SSE framing, chunk parsing, event
//! shaping — is the one a caller gets.

use a2a_llm::gemini::{GeminiConfig, GeminiProvider};
use a2a_llm::openai::{OpenAiConfig, OpenAiProvider, ReasoningDialect};
use a2a_llm::{ChatMessage, ContentPart, FinishReason, LlmProvider, LlmRequest, LlmStreamEvent};
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
        routing: None,
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

/// Gemini's `parameters` is an OpenAPI `Schema`, not JSON Schema, and the API
/// refuses a keyword it does not know with the same 400 it gives an unknown
/// `thinkingConfig` field. Seen 2026-09-05 from strata's generated schemas
/// reaching Gemini through korps: they passed only after `const` and `format`
/// were stripped at the source. The provider strips them now, so what reaches
/// the socket is the schema Gemini takes, whatever the tool's author wrote.
#[tokio::test]
async fn a_tool_schema_reaches_gemini_in_the_subset_it_takes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/test-model:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"candidates":[{"content":{"parts":[{"text":"ok"}],"role":"model"},"finishReason":"STOP"}]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let tool = a2a_llm::ToolDefinition::new(
        "write_file",
        "Write a file",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "kind": { "type": "string", "const": "sql" },
                "path": { "type": "string", "format": "uri" },
                "retries": { "type": "integer", "format": "uint32", "default": 0 },
                "tags": { "type": ["array", "null"], "items": { "type": "string" }, "minItems": 1 },
            },
            "required": ["kind", "path"],
        }),
    );

    gemini(&server)
        .chat_completion(LlmRequest::new(vec![ChatMessage::user("hi")]).tools(vec![tool]))
        .await
        .expect("the call succeeds");

    let requests = server
        .received_requests()
        .await
        .expect("requests are recorded");
    let body: serde_json::Value =
        serde_json::from_slice(&requests[0].body).expect("the body is JSON");
    assert_eq!(
        body["tools"][0]["functionDeclarations"][0]["parameters"],
        serde_json::json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["sql"] },
                "path": { "type": "string" },
                "retries": { "type": "integer", "default": 0 },
                "tags": { "type": "array", "nullable": true, "items": { "type": "string" }, "minItems": 1 },
            },
            "required": ["kind", "path"],
        })
    );
}

/// The point of message parts: the bytes leave the process. Pinned on the
/// *streaming* path, which is the one an agent actually runs on and the one
/// that used to hold its own copy of the message mapping — a copy is exactly
/// where a feature lands on one path and not the other.
#[tokio::test]
async fn a_file_reaches_gemini_as_bytes_on_the_streaming_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/test-model:streamGenerateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"a duck\"}],\"role\":\"model\"}}]}\n\n",
            "text/event-stream",
        ))
        .mount(&server)
        .await;

    let stream = gemini(&server)
        .chat_completion_stream(LlmRequest::new(vec![ChatMessage::user(vec![
            ContentPart::text("what is this"),
            ContentPart::blob("image/png", b"\x89PNG".to_vec()).named("duck.png"),
        ])]))
        .await
        .expect("the stream opens");
    let _: Vec<_> = stream.collect().await;

    let requests = server
        .received_requests()
        .await
        .expect("requests are recorded");
    let body: serde_json::Value =
        serde_json::from_slice(&requests[0].body).expect("the body is JSON");
    assert_eq!(
        body["contents"][0]["parts"],
        serde_json::json!([
            { "text": "what is this" },
            { "inlineData": { "mimeType": "image/png", "data": "iVBORw==" } },
        ]),
        "body: {body}"
    );
}

/// The same file, the same path, the other provider — where an image is a
/// `data:` URL rather than bare base64.
#[tokio::test]
async fn a_file_reaches_openai_as_bytes_on_the_streaming_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"a duck\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            "text/event-stream",
        ))
        .mount(&server)
        .await;

    let stream = openai(&server)
        .chat_completion_stream(LlmRequest::new(vec![ChatMessage::user(vec![
            ContentPart::text("what is this"),
            ContentPart::blob("image/png", b"\x89PNG".to_vec()).named("duck.png"),
        ])]))
        .await
        .expect("the stream opens");
    let _: Vec<_> = stream.collect().await;

    let requests = server
        .received_requests()
        .await
        .expect("requests are recorded");
    let body: serde_json::Value =
        serde_json::from_slice(&requests[0].body).expect("the body is JSON");
    assert_eq!(
        body["messages"][0]["content"],
        serde_json::json!([
            { "type": "text", "text": "what is this" },
            { "type": "image_url", "image_url": { "url": "data:image/png;base64,iVBORw==" } },
        ]),
        "body: {body}"
    );
}

/// strata's query tool, cut down: `time_grain` is optional, and an enum.
fn query_tool() -> a2a_llm::ToolDefinition {
    a2a_llm::ToolDefinition::new(
        "query",
        "Query a view",
        serde_json::json!({
            "type": "object",
            "properties": {
                "metrics": { "type": "array", "items": { "type": "string" } },
                "time_grain": { "type": "string", "enum": ["day", "week"] },
            },
            "required": ["metrics"],
        }),
    )
}

/// A model that fills every property it is shown cannot leave an optional
/// enum out, and picks a member. Measured on `openai/gpt-6-luna` through
/// OpenRouter on 2026-10-17: four calls of four set `time_grain` when told to
/// leave it out, and four of four sent `null` once the schema allowed it. So
/// the request offers `null`, and the call the tool gets has none.
#[tokio::test]
async fn an_optional_property_can_be_left_out_by_a_model_that_fills_every_one() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
                {"id":"c1","type":"function","function":{"name":"query",
                 "arguments":"{\"metrics\":[\"avg\"],\"time_grain\":null}"}}]},
               "finish_reason":"tool_calls"}]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let response = openai(&server)
        .chat_completion(LlmRequest::new(vec![ChatMessage::user("hi")]).tools(vec![query_tool()]))
        .await
        .expect("the call succeeds");

    let requests = server.received_requests().await.expect("recorded");
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).expect("JSON");
    assert_eq!(
        body["tools"][0]["function"]["parameters"]["properties"]["time_grain"],
        serde_json::json!({ "type": ["string", "null"], "enum": ["day", "week", null] })
    );
    let calls = response.tool_calls.expect("a call");
    assert_eq!(calls[0].arguments, r#"{"metrics":["avg"]}"#);
}

/// The same on the streaming path, where the arguments arrive in pieces and
/// the call is whole only at the end.
#[tokio::test]
async fn an_optional_property_left_out_on_the_streaming_path_reaches_the_tool_left_out() {
    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",",
        "\"type\":\"function\",\"function\":{\"name\":\"query\",\"arguments\":\"{\\\"metrics\\\":[\\\"avg\\\"],\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
        "\"function\":{\"arguments\":\"\\\"time_grain\\\":null}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .mount(&server)
        .await;

    let stream = openai(&server)
        .chat_completion_stream(
            LlmRequest::new(vec![ChatMessage::user("hi")]).tools(vec![query_tool()]),
        )
        .await
        .expect("the stream opens");
    let events: Vec<LlmStreamEvent> = stream.map(|e| e.expect("no event fails")).collect().await;

    let requests = server.received_requests().await.expect("recorded");
    let sent: serde_json::Value = serde_json::from_slice(&requests[0].body).expect("JSON");
    assert_eq!(
        sent["tools"][0]["function"]["parameters"]["properties"]["time_grain"]["enum"],
        serde_json::json!(["day", "week", null])
    );
    let call = events
        .iter()
        .find_map(|e| match e {
            LlmStreamEvent::ToolCall(call) => Some(call),
            _ => None,
        })
        .expect("the call is emitted whole");
    assert_eq!(call.arguments, r#"{"metrics":["avg"]}"#);
}

/// A request that thinks goes to the thinking model and the rest to the
/// model, each with the configured routing: a turn asks for thinking and
/// gets a model that does it, and both stay on one provider's cache.
#[tokio::test]
async fn a_request_that_thinks_goes_to_the_thinking_model_on_the_pinned_provider() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;
    let selected = a2a_llm::provider_from_settings(&a2a_llm::LlmSettings {
        provider: "openrouter".to_string(),
        api_key: Some("key".to_string()),
        base_url: Some(format!("{}/v1", server.uri())),
        model: Some("openai/gpt-6-luna".to_string()),
        thinking_model: Some("openai/gpt-6.1-sol".to_string()),
        reasoning: Some(a2a_llm::Reasoning::Off),
        routing: Some(a2a_llm::ProviderRouting::prefer("openai")),
        ..Default::default()
    })
    .expect("builds");

    let ask = || LlmRequest::new(vec![ChatMessage::user("hi")]);
    for request in [
        ask(),
        ask().reasoning(a2a_llm::Reasoning::Effort(a2a_llm::ReasoningEffort::High)),
        ask().reasoning(a2a_llm::Reasoning::Off),
        ask().reasoning(a2a_llm::Reasoning::Budget(2000)),
    ] {
        selected
            .provider
            .chat_completion(request)
            .await
            .expect("the call succeeds");
    }

    let requests = server.received_requests().await.expect("recorded");
    let bodies: Vec<serde_json::Value> = requests
        .iter()
        .map(|r| serde_json::from_slice(&r.body).expect("JSON"))
        .collect();
    let models: Vec<&str> = bodies
        .iter()
        .map(|b| b["model"].as_str().unwrap())
        .collect();
    assert_eq!(
        models,
        [
            "openai/gpt-6-luna",
            "openai/gpt-6.1-sol",
            "openai/gpt-6-luna",
            "openai/gpt-6.1-sol"
        ]
    );
    for body in &bodies {
        assert_eq!(
            body["provider"],
            serde_json::json!({ "order": ["openai"], "allow_fallbacks": true }),
            "{body}"
        );
    }
    assert_eq!(
        bodies[0]["reasoning"],
        serde_json::json!({ "enabled": false })
    );
    assert_eq!(bodies[1]["reasoning"]["effort"], "high");
}
