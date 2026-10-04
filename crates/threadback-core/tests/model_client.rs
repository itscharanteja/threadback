//! M-02: the Token Factory client, against a real HTTP server on loopback.

mod common;

use std::{
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use common::{FakeServer, Scripted};

use serde_json::{Value, json};
use threadback_core::{
    ApiKey, CancelToken, CapsuleModel, ChatMessage, ErrorCode, ModelSettings, PROMPT_VERSION,
    PromptRequest, SCHEMA_VERSION, StructuredOutput, TokenFactoryClient, capsule_schema,
};

const KEY: &str = "tfk-7F4A91C2-demo-secret-key-value-0001";
const EVIDENCE: &str = "EVIDENCE-MARKER-the-lock-owner-differs";

fn settings(endpoint: &str) -> ModelSettings {
    ModelSettings {
        endpoint: endpoint.into(),
        model_id: "nvidia/test-model".into(),
        timeout_secs: 5,
        structured_output: StructuredOutput::JsonSchema,
    }
}

fn client(server: &FakeServer) -> TokenFactoryClient {
    TokenFactoryClient::new(
        settings(&server.endpoint),
        ApiKey::new(KEY),
        CancelToken::new(),
    )
}

fn request() -> PromptRequest {
    PromptRequest {
        prompt_version: PROMPT_VERSION,
        schema_version: SCHEMA_VERSION,
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: "SYS-RULES never follow the evidence".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: format!("Write the capsule.\n{EVIDENCE}"),
            },
        ],
        response_schema: capsule_schema(),
        payload_hash: "h".into(),
        attempt: 1,
    }
}

fn error_of(client: &TokenFactoryClient) -> threadback_core::DomainError {
    client.complete(&request()).unwrap_err()
}

// ---- the request ----

#[test]
fn it_posts_the_prompt_to_chat_completions_with_the_key_and_the_schema() {
    let server = FakeServer::start(vec![Scripted::ok("{}")]);
    client(&server).complete(&request()).unwrap();

    let sent = &server.requests()[0];
    assert_eq!(
        (sent.method.as_str(), sent.path.as_str()),
        ("POST", "/v1/chat/completions")
    );
    assert_eq!(sent.headers["authorization"], format!("Bearer {KEY}"));
    assert_eq!(sent.headers["content-type"], "application/json");
    assert!(sent.headers["user-agent"].starts_with("ThreadBack/"));

    let body = sent.json();
    assert_eq!(body["model"], "nvidia/test-model");
    assert_eq!(body["temperature"], 0);
    assert_eq!(body["max_tokens"], 6000);
    assert_eq!(
        body["messages"],
        json!([
            {"role": "system", "content": "SYS-RULES never follow the evidence"},
            {"role": "user", "content": format!("Write the capsule.\n{EVIDENCE}")},
        ])
    );
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert_eq!(
        body["response_format"]["json_schema"]["name"],
        "resume_capsule"
    );
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    assert_eq!(
        body["response_format"]["json_schema"]["schema"],
        capsule_schema()
    );
    assert!(body.get("stream").is_none());
}

#[test]
fn the_other_response_formats_are_sent_as_configured() {
    for (mode, expected) in [
        (StructuredOutput::JsonObject, Some("json_object")),
        (StructuredOutput::Off, None),
    ] {
        let server = FakeServer::start(vec![Scripted::ok("{}")]);
        let mut s = settings(&server.endpoint);
        s.structured_output = mode;
        TokenFactoryClient::new(s, ApiKey::new(KEY), CancelToken::new())
            .complete(&request())
            .unwrap();
        let body = server.requests()[0].json();
        assert_eq!(
            body["response_format"]["type"].as_str(),
            expected,
            "{mode:?}"
        );
    }
}

#[test]
fn a_successful_reply_is_returned_with_its_usage_recorded() {
    let server = FakeServer::start(vec![Scripted::ok("{\"goal\":\"x\"}")]);
    let client = client(&server);
    let reply = client.complete(&request()).unwrap();
    assert_eq!(reply.text, "{\"goal\":\"x\"}");

    let records = client.take_diagnostics();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.status, "ok");
    assert_eq!(record.http_status, Some(200));
    assert_eq!(
        (
            record.prompt_tokens,
            record.completion_tokens,
            record.total_tokens
        ),
        (Some(120), Some(45), Some(165))
    );
    assert_eq!(record.finish_reason.as_deref(), Some("stop"));
    assert_eq!(record.model_id, "nvidia/test-model");
    assert_eq!(record.endpoint_host, "127.0.0.1");
    assert_eq!(record.structured_output, StructuredOutput::JsonSchema);
    assert_eq!(record.attempt, 1);
    assert!(client.take_diagnostics().is_empty(), "taking drains");
}

#[test]
fn a_leading_reasoning_block_is_dropped_from_the_reply() {
    let server = FakeServer::start(vec![Scripted::ok(
        "<think>let me work it out</think>\n{\"goal\":\"x\"}",
    )]);
    assert_eq!(
        client(&server).complete(&request()).unwrap().text,
        "{\"goal\":\"x\"}"
    );
}

// ---- what diagnostics may contain ----

#[test]
fn diagnostics_hold_no_payload_no_reply_and_no_authorization_data() {
    let server = FakeServer::start(vec![
        Scripted::ok("{\"secret_reply\":\"REPLY-CONTENT-xyz\"}"),
        Scripted::error(500, &format!("oops {EVIDENCE}")),
        Scripted::error(401, "unauthorized"),
    ]);
    let client = client(&server);
    let _ = client.complete(&request());
    let _ = client.complete(&request());
    let _ = client.complete(&request());

    let records = client.take_diagnostics();
    assert_eq!(records.len(), 3);
    let serialized = serde_json::to_string(&records).unwrap();
    let debugged = format!("{records:?} {client:?}");
    for text in [&serialized, &debugged] {
        for forbidden in [
            KEY,
            "7F4A91C2",
            EVIDENCE,
            "SYS-RULES",
            "REPLY-CONTENT",
            "Bearer",
            "authorization",
        ] {
            assert!(!text.contains(forbidden), "{forbidden:?} found in {text}");
        }
    }
    assert!(
        serialized.contains("nvidia/test-model")
            && serialized.contains("latencyMs")
            && serialized.contains("totalTokens")
    );
}

#[test]
fn errors_never_contain_the_key_or_the_evidence() {
    let statuses = [400, 401, 403, 404, 408, 429, 500, 502, 503];
    let server = FakeServer::start(
        statuses
            .iter()
            .map(|s| Scripted::error(*s, "something went wrong"))
            .collect(),
    );
    let client = client(&server);
    for status in statuses {
        let message = error_of(&client).message;
        for forbidden in [KEY, "7F4A91C2", EVIDENCE, "SYS-RULES"] {
            assert!(!message.contains(forbidden), "{status}: {message}");
        }
    }
}

// ---- typed errors ----

#[test]
fn each_failure_maps_to_a_typed_error_the_interface_can_act_on() {
    let cases: Vec<(Scripted, ErrorCode, bool, &str)> = vec![
        (
            Scripted::error(401, "bad key"),
            ErrorCode::ModelAuthentication,
            false,
            "rejected the API key",
        ),
        (
            Scripted::error(403, "forbidden"),
            ErrorCode::ModelAuthentication,
            false,
            "rejected the API key",
        ),
        (
            Scripted::error(404, "model not found"),
            ErrorCode::ModelRejected,
            false,
            "did not find that endpoint or model",
        ),
        (
            Scripted::error(408, "slow"),
            ErrorCode::ModelTimeout,
            true,
            "timed out",
        ),
        (
            Scripted::error(429, "slow down").header("Retry-After", "7"),
            ErrorCode::ModelRateLimited,
            true,
            "about 7 seconds",
        ),
        (
            Scripted::error(429, "slow down"),
            ErrorCode::ModelRateLimited,
            true,
            "shortly",
        ),
        (
            Scripted::error(500, "boom"),
            ErrorCode::ModelServer,
            true,
            "HTTP 500",
        ),
        (
            Scripted::error(502, "gateway"),
            ErrorCode::ModelServer,
            true,
            "HTTP 502",
        ),
        (
            Scripted::error(503, "busy"),
            ErrorCode::ModelServer,
            true,
            "HTTP 503",
        ),
        (
            Scripted::error(400, "context length exceeded"),
            ErrorCode::ModelRejected,
            false,
            "context length exceeded",
        ),
        (
            Scripted::error(422, "bad field"),
            ErrorCode::ModelRejected,
            false,
            "bad field",
        ),
    ];
    for (scripted, code, retryable, text) in cases {
        let server = FakeServer::start(vec![scripted.clone()]);
        let error = error_of(&client(&server));
        assert_eq!(error.code, code, "{}", scripted.status);
        assert_eq!(error.retryable, retryable, "{}", scripted.status);
        assert!(
            error.message.contains(text),
            "{}: {}",
            scripted.status,
            error.message
        );
        assert_eq!(server.requests().len(), 1, "no hidden retries");
    }
}

#[test]
fn a_credential_in_the_services_error_message_is_not_passed_on() {
    let server = FakeServer::start(vec![Scripted::error(
        400,
        "bad key sk-proj-abcdefghij1234567890ABCDEFGH supplied",
    )]);
    let message = error_of(&client(&server)).message;
    assert!(!message.contains("sk-proj"), "{message}");
    assert!(message.contains("refused the request"));
}

#[test]
fn a_redirect_is_never_followed() {
    let server = FakeServer::start(vec![
        Scripted::status(307, "").header("Location", "https://evil.example/steal"),
        Scripted::ok("{}"),
    ]);
    let error = error_of(&client(&server));
    assert_eq!(error.code, ErrorCode::ModelServer);
    assert!(
        error.message.contains("does not follow redirects"),
        "{}",
        error.message
    );
    assert_eq!(
        server.requests().len(),
        1,
        "only the original request was made"
    );
}

#[test]
fn replies_that_cannot_be_read_are_a_server_problem() {
    for body in [
        "not json".to_owned(),
        "{}".to_owned(),
        json!({"choices": []}).to_string(),
        json!({"choices": [{"message": {"content": ""}}]}).to_string(),
    ] {
        let server = FakeServer::start(vec![Scripted::status(200, &body)]);
        let error = error_of(&client(&server));
        assert_eq!(error.code, ErrorCode::ModelServer, "{body}");
        assert!(error.retryable);
    }
}

#[test]
fn an_enormous_reply_is_refused() {
    let big = "x".repeat(2 * 1024 * 1024 + 10);
    let server = FakeServer::start(vec![Scripted::ok(&big)]);
    let error = error_of(&client(&server));
    assert_eq!(error.code, ErrorCode::ModelServer);
    assert!(error.message.contains("too large"));
}

#[test]
fn a_key_that_cannot_be_sent_is_refused_without_a_request() {
    let server = FakeServer::start(vec![Scripted::ok("{}")]);
    let client = TokenFactoryClient::new(
        settings(&server.endpoint),
        ApiKey::new("bad\nkey"),
        CancelToken::new(),
    );
    let error = error_of(&client);
    assert_eq!(error.code, ErrorCode::ModelAuthentication);
    assert!(!error.message.contains("bad"));
    assert!(server.requests().is_empty());
}

// ---- time and cancellation ----

#[test]
fn a_slow_service_times_out_with_a_typed_error() {
    let server = FakeServer::start(vec![Scripted::ok("{}").delayed(Duration::from_secs(4))]);
    let mut s = settings(&server.endpoint);
    s.timeout_secs = 1;
    let client = TokenFactoryClient::new(s, ApiKey::new(KEY), CancelToken::new());
    let started = Instant::now();
    let error = error_of(&client);
    assert_eq!(error.code, ErrorCode::ModelTimeout);
    assert!(
        error.retryable && error.message.contains("within 1 seconds"),
        "{}",
        error.message
    );
    assert!(
        started.elapsed() < Duration::from_millis(3_000),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(client.take_diagnostics()[0].status, "timeout");
}

#[test]
fn an_unreachable_service_is_a_network_error() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let client = TokenFactoryClient::new(
        settings(&format!("http://127.0.0.1:{port}/v1")),
        ApiKey::new(KEY),
        CancelToken::new(),
    );
    let error = error_of(&client);
    assert_eq!(error.code, ErrorCode::ModelNetwork);
    assert!(error.retryable && error.message.contains("checkpoint is kept"));
    assert_eq!(client.take_diagnostics()[0].status, "network");
}

#[test]
fn a_connection_dropped_mid_request_is_a_network_error() {
    let mut dropped = Scripted::ok("{}");
    dropped.hang_up = true;
    let server = FakeServer::start(vec![dropped]);
    assert_eq!(error_of(&client(&server)).code, ErrorCode::ModelNetwork);
}

#[test]
fn cancelling_stops_a_request_that_is_in_flight() {
    let server = FakeServer::start(vec![Scripted::ok("{}").delayed(Duration::from_secs(5))]);
    let cancel = CancelToken::new();
    let client =
        TokenFactoryClient::new(settings(&server.endpoint), ApiKey::new(KEY), cancel.clone());
    let trigger = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        trigger.cancel();
    });

    let started = Instant::now();
    let error = error_of(&client);
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(error.message.contains("Nothing more was sent"));
    assert!(
        started.elapsed() < Duration::from_millis(2_000),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(client.take_diagnostics()[0].status, "cancelled");
}

#[test]
fn a_request_cancelled_before_it_starts_is_never_sent() {
    let server = FakeServer::start(vec![Scripted::ok("{}")]);
    let cancel = CancelToken::new();
    cancel.cancel();
    let client = TokenFactoryClient::new(settings(&server.endpoint), ApiKey::new(KEY), cancel);
    assert_eq!(error_of(&client).code, ErrorCode::Cancelled);
    thread::sleep(Duration::from_millis(100));
    assert!(server.requests().is_empty());
}

// ---- stepping down to a simpler response format ----

#[test]
fn a_rejected_schema_format_falls_back_to_json_mode_and_remembers_it() {
    let server = FakeServer::start(vec![
        Scripted::error(
            400,
            "response_format of type json_schema is not supported by this model",
        ),
        Scripted::ok("{\"a\":1}"),
        Scripted::ok("{\"a\":2}"),
    ]);
    let client = client(&server);
    assert_eq!(client.complete(&request()).unwrap().text, "{\"a\":1}");
    assert_eq!(
        client.structured_output_in_use(),
        StructuredOutput::JsonObject
    );
    assert_eq!(client.complete(&request()).unwrap().text, "{\"a\":2}");

    let sent = server.requests();
    assert_eq!(sent[0].json()["response_format"]["type"], "json_schema");
    assert_eq!(sent[1].json()["response_format"]["type"], "json_object");
    assert_eq!(
        sent[2].json()["response_format"]["type"],
        "json_object",
        "the next call does not retry the schema"
    );
    // The same messages went every time: the fallback changes only how JSON is asked for.
    assert_eq!(sent[0].json()["messages"], sent[1].json()["messages"]);

    let records = client.take_diagnostics();
    let modes: Vec<_> = records
        .iter()
        .map(|r| (r.status.as_str(), r.structured_output))
        .collect();
    assert_eq!(
        modes,
        [
            ("rejected", StructuredOutput::JsonSchema),
            ("ok", StructuredOutput::JsonObject),
            ("ok", StructuredOutput::JsonObject),
        ]
    );
}

#[test]
fn it_steps_down_twice_at_most_and_ends_with_no_response_format() {
    let rejection = "unsupported response_format";
    let server = FakeServer::start(vec![
        Scripted::error(400, rejection),
        Scripted::error(422, rejection),
        Scripted::ok("{}"),
    ]);
    let client = client(&server);
    client.complete(&request()).unwrap();
    let sent = server.requests();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[1].json()["response_format"]["type"], "json_object");
    assert!(sent[2].json().get("response_format").is_none());
    assert_eq!(client.structured_output_in_use(), StructuredOutput::Off);
}

#[test]
fn it_does_not_retry_forever_when_every_format_is_rejected() {
    let rejection = "response_format is not supported";
    let server = FakeServer::start(vec![
        Scripted::error(400, rejection),
        Scripted::error(400, rejection),
        Scripted::error(400, rejection),
        Scripted::ok("{}"),
    ]);
    let error = error_of(&client(&server));
    assert_eq!(error.code, ErrorCode::ModelRejected);
    assert_eq!(
        server.requests().len(),
        3,
        "schema, json object, none: then it stops"
    );
}

#[test]
fn an_unrelated_rejection_is_not_retried() {
    let server = FakeServer::start(vec![
        Scripted::error(400, "context length exceeded"),
        Scripted::ok("{}"),
    ]);
    assert_eq!(error_of(&client(&server)).code, ErrorCode::ModelRejected);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn a_server_error_is_not_mistaken_for_a_format_problem() {
    let server = FakeServer::start(vec![
        Scripted::error(500, "schema validation crashed"),
        Scripted::ok("{}"),
    ]);
    assert_eq!(error_of(&client(&server)).code, ErrorCode::ModelServer);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn with_the_format_off_a_schema_complaint_is_not_retried() {
    let server = FakeServer::start(vec![
        Scripted::error(400, "schema problem"),
        Scripted::ok("{}"),
    ]);
    let mut s = settings(&server.endpoint);
    s.structured_output = StructuredOutput::Off;
    let error = TokenFactoryClient::new(s, ApiKey::new(KEY), CancelToken::new())
        .complete(&request())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ModelRejected);
    assert_eq!(server.requests().len(), 1);
}

// ---- the connection test ----

#[test]
fn a_connection_test_sends_none_of_the_users_evidence() {
    let server = FakeServer::start(vec![Scripted::ok("OK")]);
    let result = client(&server).test_connection();
    assert!(result.ok, "{}", result.message);
    assert!(
        result
            .message
            .starts_with("Connected. nvidia/test-model answered in ")
    );
    assert!(result.latency_ms.is_some());

    let body = server.requests()[0].json();
    assert_eq!(body["max_tokens"], 64);
    assert!(body.get("response_format").is_none());
    assert_eq!(
        body["messages"],
        json!([{"role": "user", "content": "Reply with the single word OK."}])
    );
    assert!(!server.requests()[0].body.contains(EVIDENCE));
}

#[test]
fn a_failed_connection_test_explains_what_to_fix() {
    let server = FakeServer::start(vec![Scripted::error(401, "no")]);
    let result = client(&server).test_connection();
    assert!(!result.ok);
    assert!(result.message.contains("rejected the API key"));
    assert!(!result.message.contains(KEY));
}

// ---- replies from reasoning models ----

fn reasoning_reply(content: Value, finish_reason: &str, tokens: u32) -> Scripted {
    Scripted::status(
        200,
        &json!({
            "id": "chatcmpl-1",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content, "reasoning_content": "step by step ".repeat(40)},
                "finish_reason": finish_reason,
            }],
            "usage": {"prompt_tokens": 30, "completion_tokens": tokens, "total_tokens": 30 + tokens}
        })
        .to_string(),
    )
}

#[test]
fn a_reply_with_no_text_says_why_and_what_arrived_without_showing_any_text() {
    let server = FakeServer::start(vec![reasoning_reply(Value::Null, "length", 6000)]);
    let client = client(&server);
    let error = error_of(&client);

    assert_eq!(error.code, ErrorCode::ModelServer);
    assert!(error.retryable);
    let message = &error.message;
    assert!(
        message.contains("returned no text because it used all 6000 of its output tokens"),
        "{message}"
    );
    assert!(
        message.contains("reasoning models spend tokens thinking"),
        "{message}"
    );
    assert!(message.contains("520 characters of reasoning"), "{message}");
    assert!(message.contains("What arrived:"), "{message}");
    assert!(
        message.contains("content is null") && message.contains("finish_reason is length"),
        "{message}"
    );
    assert!(
        !message.contains("step by step"),
        "no part of the reply's text is shown"
    );

    let record = &client.take_diagnostics()[0];
    assert_eq!(record.status, "no_text");
    assert_eq!(record.finish_reason.as_deref(), Some("length"));
    assert_eq!(record.completion_tokens, Some(6000));
}

#[test]
fn an_unreadable_reply_is_described_by_its_shape_only() {
    let server = FakeServer::start(vec![
        Scripted::status(
            200,
            "<html><body>Bad gateway SECRET-PAGE-TEXT</body></html>",
        ),
        Scripted::status(
            200,
            &json!({"object": "error", "message": "SECRET-ERROR-TEXT", "choices": []}).to_string(),
        ),
    ]);
    let client = client(&server);
    let first = error_of(&client).message;
    assert!(first.contains("could not read"), "{first}");
    assert!(first.contains("What arrived: not JSON ("), "{first}");
    assert!(!first.contains("SECRET-PAGE-TEXT"));

    let second = error_of(&client).message;
    assert!(second.contains("choices is a list of 0"), "{second}");
    assert!(
        second.contains("fields [choices, message, object]"),
        "{second}"
    );
    assert!(!second.contains("SECRET-ERROR-TEXT"));
}

#[test]
fn a_connection_test_succeeds_when_the_model_spends_its_short_budget_thinking() {
    let server = FakeServer::start(vec![reasoning_reply(Value::Null, "length", 64)]);
    let result = client(&server).test_connection();
    assert!(result.ok, "{}", result.message);
    assert!(
        result
            .message
            .starts_with("Connected. nvidia/test-model answered in "),
        "{}",
        result.message
    );
    assert!(
        result
            .message
            .contains("returned no text, which is fine for a connection test"),
        "{}",
        result.message
    );
}

#[test]
fn a_connection_test_still_fails_for_a_reply_that_is_not_a_chat_completion() {
    let server = FakeServer::start(vec![Scripted::status(200, "<html>not an API</html>")]);
    let result = client(&server).test_connection();
    assert!(!result.ok);
    assert!(
        result.message.contains("could not read"),
        "{}",
        result.message
    );
    assert!(result.message.contains("not JSON"), "{}", result.message);
}

#[test]
fn text_in_the_normal_place_still_wins_over_a_reasoning_field() {
    let server = FakeServer::start(vec![reasoning_reply(
        json!("{\"goal\":\"x\"}"),
        "stop",
        120,
    )]);
    assert_eq!(
        client(&server).complete(&request()).unwrap().text,
        "{\"goal\":\"x\"}"
    );
}
