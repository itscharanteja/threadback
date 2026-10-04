use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use reqwest::{
    StatusCode,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue, RETRY_AFTER},
    redirect::Policy,
};
use serde_json::{Value, json};

use super::{
    diagnostics::ModelCallRecord,
    generate::{CapsuleModel, ModelReply},
    prompt::{ChatMessage, PromptRequest},
    validate::clip,
};
use crate::{
    ApiKey, ConnectionTestResult, DomainError, ErrorCode, ModelSettings, Redactor,
    StructuredOutput, capsule_schema,
};

const MAX_OUTPUT_TOKENS: u32 = 6_000;
const PING_OUTPUT_TOKENS: u32 = 64;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CANCEL_POLL: Duration = Duration::from_millis(20);
/// How many times the client may step down to a simpler response format
/// because the service rejected the one it asked for.
const MAX_FORMAT_DOWNGRADES: u8 = 2;

/// Lets another thread stop a request that is in flight.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

async fn wait_cancelled(token: &CancelToken) {
    while !token.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

fn mode_to_u8(mode: StructuredOutput) -> u8 {
    match mode {
        StructuredOutput::JsonSchema => 0,
        StructuredOutput::JsonObject => 1,
        StructuredOutput::Off => 2,
    }
}

fn mode_from_u8(value: u8) -> StructuredOutput {
    match value {
        0 => StructuredOutput::JsonSchema,
        1 => StructuredOutput::JsonObject,
        _ => StructuredOutput::Off,
    }
}

/// Talks to Nebius Token Factory (or any OpenAI-compatible endpoint). It holds
/// the key only for the life of the client and never prints it.
#[derive(Debug)]
pub struct TokenFactoryClient {
    settings: ModelSettings,
    api_key: ApiKey,
    cancel: CancelToken,
    mode: AtomicU8,
    records: Mutex<Vec<ModelCallRecord>>,
}

struct Success {
    text: String,
    record: ModelCallRecord,
}

struct Failure {
    error: DomainError,
    record: ModelCallRecord,
    /// The service's own explanation, shortened and redacted.
    provider_message: Option<String>,
}

impl TokenFactoryClient {
    pub fn new(settings: ModelSettings, api_key: ApiKey, cancel: CancelToken) -> Self {
        let mode = AtomicU8::new(mode_to_u8(settings.structured_output));
        Self {
            settings,
            api_key,
            cancel,
            mode,
            records: Mutex::new(Vec::new()),
        }
    }

    fn mode(&self) -> StructuredOutput {
        mode_from_u8(self.mode.load(Ordering::SeqCst))
    }

    /// The response format in use now, which may be simpler than the one
    /// configured if the service rejected it.
    pub fn structured_output_in_use(&self) -> StructuredOutput {
        self.mode()
    }

    /// Asks the service to answer one tiny question that contains none of the
    /// user's evidence, to check the endpoint, key, and model together.
    pub fn test_connection(&self) -> ConnectionTestResult {
        let request = PromptRequest {
            prompt_version: super::PROMPT_VERSION,
            schema_version: super::SCHEMA_VERSION,
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "Reply with the single word OK.".into(),
            }],
            response_schema: Value::Null,
            payload_hash: String::new(),
            attempt: 1,
        };
        let outcome = self.run_blocking(
            &request,
            StructuredOutput::Off,
            PING_OUTPUT_TOKENS,
            false,
            false,
        );
        let latency = self
            .take_diagnostics()
            .last()
            .map(|record| record.latency_ms);
        match outcome {
            Ok(reply) => ConnectionTestResult {
                ok: true,
                message: if reply.text.is_empty() {
                    format!(
                        "Connected. {} answered in {} ms. It used its short test budget thinking and returned no text, which is fine for a connection test.",
                        self.settings.model_id,
                        latency.unwrap_or(0)
                    )
                } else {
                    format!(
                        "Connected. {} answered in {} ms.",
                        self.settings.model_id,
                        latency.unwrap_or(0)
                    )
                },
                latency_ms: latency,
                model_id: self.settings.model_id.clone(),
            },
            Err(error) => ConnectionTestResult {
                ok: false,
                message: error.message,
                latency_ms: latency,
                model_id: self.settings.model_id.clone(),
            },
        }
    }

    fn run_blocking(
        &self,
        request: &PromptRequest,
        start_mode: StructuredOutput,
        max_tokens: u32,
        allow_downgrade: bool,
        require_text: bool,
    ) -> Result<ModelReply, DomainError> {
        if self.cancel.is_cancelled() {
            return Err(cancelled());
        }
        // A runtime of its own on a worker thread, so this works whether or
        // not the caller is already inside another runtime.
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| {
                            DomainError::internal("The network runtime could not start.")
                        })?;
                    runtime.block_on(self.run(
                        request,
                        start_mode,
                        max_tokens,
                        allow_downgrade,
                        require_text,
                    ))
                })
                .join()
                .unwrap_or_else(|_| {
                    Err(DomainError::internal(
                        "The model request stopped unexpectedly.",
                    ))
                })
        })
    }

    async fn run(
        &self,
        request: &PromptRequest,
        start_mode: StructuredOutput,
        max_tokens: u32,
        allow_downgrade: bool,
        require_text: bool,
    ) -> Result<ModelReply, DomainError> {
        let mut mode = start_mode;
        let mut downgrades = 0;
        loop {
            match self
                .post_once(request, mode, max_tokens, require_text)
                .await
            {
                Ok(success) => {
                    self.push(success.record);
                    return Ok(ModelReply { text: success.text });
                }
                Err(failure) => {
                    let rejected_format = allow_downgrade
                        && mode != StructuredOutput::Off
                        && downgrades < MAX_FORMAT_DOWNGRADES
                        && matches!(failure.record.http_status, Some(400 | 422))
                        && failure
                            .provider_message
                            .as_deref()
                            .is_some_and(mentions_response_format);
                    self.push(failure.record);
                    if !rejected_format {
                        return Err(failure.error);
                    }
                    // Same payload, same destination, a simpler way of asking.
                    mode = match mode {
                        StructuredOutput::JsonSchema => StructuredOutput::JsonObject,
                        _ => StructuredOutput::Off,
                    };
                    self.mode.store(mode_to_u8(mode), Ordering::SeqCst);
                    downgrades += 1;
                }
            }
        }
    }

    fn push(&self, record: ModelCallRecord) {
        if let Ok(mut records) = self.records.lock() {
            records.push(record);
        }
    }

    fn body(&self, request: &PromptRequest, mode: StructuredOutput, max_tokens: u32) -> Value {
        let mut body = json!({
            "model": self.settings.model_id,
            "messages": request.messages,
            "temperature": 0,
            "max_tokens": max_tokens,
        });
        match mode {
            StructuredOutput::JsonSchema => {
                let schema = if request.response_schema.is_null() {
                    capsule_schema()
                } else {
                    request.response_schema.clone()
                };
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": { "name": "resume_capsule", "schema": schema, "strict": true },
                });
            }
            StructuredOutput::JsonObject => {
                body["response_format"] = json!({ "type": "json_object" })
            }
            StructuredOutput::Off => {}
        }
        body
    }

    fn record(
        &self,
        request: &PromptRequest,
        mode: StructuredOutput,
        status: &str,
        http_status: Option<u16>,
        started: Instant,
    ) -> ModelCallRecord {
        ModelCallRecord {
            model_id: self.settings.model_id.clone(),
            endpoint_host: self.settings.destination_host(),
            status: status.to_owned(),
            http_status,
            latency_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            finish_reason: None,
            structured_output: mode,
            attempt: request.attempt,
        }
    }

    fn failure(
        &self,
        request: &PromptRequest,
        mode: StructuredOutput,
        label: &str,
        http_status: Option<u16>,
        started: Instant,
        error: DomainError,
        provider_message: Option<String>,
    ) -> Failure {
        Failure {
            error,
            record: self.record(request, mode, label, http_status, started),
            provider_message,
        }
    }

    async fn post_once(
        &self,
        request: &PromptRequest,
        mode: StructuredOutput,
        max_tokens: u32,
        require_text: bool,
    ) -> Result<Success, Failure> {
        let started = Instant::now();
        let fail =
            |label: &str, status: Option<u16>, error: DomainError, message: Option<String>| {
                self.failure(request, mode, label, status, started, error, message)
            };

        let client = reqwest::Client::builder()
            // A redirect could carry the request, and the key, somewhere
            // the user never chose.
            .redirect(Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(u64::from(self.settings.timeout_secs)))
            .user_agent(concat!("ThreadBack/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| {
                fail(
                    "network",
                    None,
                    internal("The network client could not start."),
                    None,
                )
            })?;

        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", self.api_key.expose()))
            .map_err(|_| {
                fail(
                    "authentication",
                    None,
                    DomainError::model(
                        ErrorCode::ModelAuthentication,
                        "The API key contains characters that cannot be sent. Enter it again in Settings.",
                        false,
                    ),
                    None,
                )
            })?;
        authorization.set_sensitive(true);

        let payload = serde_json::to_vec(&self.body(request, mode, max_tokens)).map_err(|_| {
            fail(
                "rejected",
                None,
                internal("The request could not be built."),
                None,
            )
        })?;
        let send = client
            .post(self.settings.chat_completions_url())
            .header(AUTHORIZATION, authorization)
            .header(CONTENT_TYPE, "application/json")
            .body(payload)
            .send();

        let response = tokio::select! {
            result = send => result,
            () = wait_cancelled(&self.cancel) => {
                return Err(fail("cancelled", None, cancelled(), None));
            }
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let (label, mapped) = transport_error(&error, self.settings.timeout_secs);
                return Err(fail(label, None, mapped, None));
            }
        };

        let status = response.status();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());

        let mut response = response;
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk,
                () = wait_cancelled(&self.cancel) => {
                    return Err(fail("cancelled", Some(status.as_u16()), cancelled(), None));
                }
            };
            match chunk {
                Ok(Some(chunk)) => {
                    if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                        return Err(fail(
                            "unreadable",
                            Some(status.as_u16()),
                            unreadable("The model service's reply was too large."),
                            None,
                        ));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(error) => {
                    let (label, mapped) = transport_error(&error, self.settings.timeout_secs);
                    return Err(fail(label, Some(status.as_u16()), mapped, None));
                }
            }
        }

        if status.is_success() {
            return match parse_success(&bytes) {
                Ok(parsed) => {
                    let mut record =
                        self.record(request, mode, "ok", Some(status.as_u16()), started);
                    record.prompt_tokens = parsed.prompt_tokens;
                    record.completion_tokens = parsed.completion_tokens;
                    record.total_tokens = parsed.total_tokens;
                    record.finish_reason = parsed.finish_reason.clone();
                    if parsed.text.is_empty() && require_text {
                        record.status = "no_text".to_owned();
                        return Err(Failure {
                            error: no_text_error(&parsed, &bytes),
                            record,
                            provider_message: None,
                        });
                    }
                    Ok(Success {
                        text: parsed.text,
                        record,
                    })
                }
                Err(shape) => Err(fail(
                    "unreadable",
                    Some(status.as_u16()),
                    unreadable(&format!(
                        "The model service sent a reply ThreadBack could not read. What arrived: {shape}"
                    )),
                    None,
                )),
            };
        }

        let provider_message = provider_message(&bytes);
        let (label, error) = status_error(status, retry_after, provider_message.as_deref());
        Err(fail(label, Some(status.as_u16()), error, provider_message))
    }
}

impl CapsuleModel for TokenFactoryClient {
    fn complete(&self, request: &PromptRequest) -> Result<ModelReply, DomainError> {
        self.run_blocking(request, self.mode(), MAX_OUTPUT_TOKENS, true, true)
    }

    fn take_diagnostics(&self) -> Vec<ModelCallRecord> {
        self.records
            .lock()
            .map(|mut records| std::mem::take(&mut *records))
            .unwrap_or_default()
    }
}

fn internal(message: &str) -> DomainError {
    DomainError::internal(message.to_owned())
}

fn cancelled() -> DomainError {
    DomainError::model(
        ErrorCode::Cancelled,
        "Generation was cancelled. Nothing more was sent.",
        true,
    )
}

fn unreadable(message: &str) -> DomainError {
    DomainError::model(ErrorCode::ModelServer, message.to_owned(), true)
}

fn transport_error(error: &reqwest::Error, timeout_secs: u32) -> (&'static str, DomainError) {
    if error.is_timeout() {
        (
            "timeout",
            DomainError::model(
                ErrorCode::ModelTimeout,
                format!(
                    "The model service did not answer within {timeout_secs} seconds. Your checkpoint is kept; you can try again."
                ),
                true,
            ),
        )
    } else {
        (
            "network",
            DomainError::model(
                ErrorCode::ModelNetwork,
                "Could not reach the model service. Check your connection and the endpoint. Your checkpoint is kept; you can try again.",
                true,
            ),
        )
    }
}

fn status_error(
    status: StatusCode,
    retry_after: Option<u64>,
    provider_message: Option<&str>,
) -> (&'static str, DomainError) {
    let detail = provider_message
        .map(|message| format!(" ({message})"))
        .unwrap_or_default();
    match status.as_u16() {
        401 | 403 => (
            "authentication",
            DomainError::model(
                ErrorCode::ModelAuthentication,
                "The model service rejected the API key. Check it in Settings.",
                false,
            ),
        ),
        429 => (
            "rate_limited",
            DomainError::model(
                ErrorCode::ModelRateLimited,
                match retry_after {
                    Some(seconds) => format!(
                        "The model service is limiting requests. Try again in about {seconds} seconds."
                    ),
                    None => "The model service is limiting requests. Try again shortly.".into(),
                },
                true,
            ),
        ),
        408 => (
            "timeout",
            DomainError::model(
                ErrorCode::ModelTimeout,
                "The model service timed out. Your checkpoint is kept; you can try again.",
                true,
            ),
        ),
        404 => (
            "rejected",
            DomainError::model(
                ErrorCode::ModelRejected,
                format!(
                    "The model service did not find that endpoint or model. Check the endpoint and model ID in Settings.{detail}"
                ),
                false,
            ),
        ),
        300..=399 => (
            "server",
            DomainError::model(
                ErrorCode::ModelServer,
                "The model service tried to redirect the request. ThreadBack does not follow redirects; check the endpoint in Settings.",
                false,
            ),
        ),
        400..=499 => (
            "rejected",
            DomainError::model(
                ErrorCode::ModelRejected,
                format!("The model service refused the request.{detail}"),
                false,
            ),
        ),
        _ => (
            "server",
            DomainError::model(
                ErrorCode::ModelServer,
                format!(
                    "The model service had a problem (HTTP {}). Your checkpoint is kept; you can try again.",
                    status.as_u16()
                ),
                true,
            ),
        ),
    }
}

/// The service's own explanation of an error, on one line, shortened, and
/// passed through redaction so it can never carry a credential onward.
fn provider_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let raw = value["error"]["message"]
        .as_str()
        .or_else(|| value["error"].as_str())
        .or_else(|| value["message"].as_str())
        .or_else(|| value["detail"].as_str())?;
    let message = clip(raw, 200);
    Some(Redactor::builtin().redact(&message).text).filter(|text| !text.trim().is_empty())
}

fn mentions_response_format(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "response_format",
        "json_schema",
        "json_object",
        "structured",
        "guided_json",
        "schema",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

struct Parsed {
    text: String,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
    finish_reason: Option<String>,
    /// How long the model's separate reasoning field was, if it sent one.
    reasoning_chars: Option<usize>,
}

/// Reads a successful reply. A reply with the usual structure but no text
/// (content null or empty, as a reasoning model produces when it runs out of
/// tokens while thinking) is `Ok` with empty text; the caller decides whether
/// that is acceptable. Anything without the usual structure is an `Err`
/// carrying a description of its shape.
fn parse_success(bytes: &[u8]) -> Result<Parsed, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| shape_of(bytes))?;
    let choice = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .ok_or_else(|| shape_of(bytes))?;
    let message = choice.get("message").ok_or_else(|| shape_of(bytes))?;
    let content = match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        Some(Value::Null) | None => String::new(),
        Some(_) => return Err(shape_of(bytes)),
    };
    let number = |name: &str| {
        value["usage"][name]
            .as_u64()
            .map(|n| n.min(u64::from(u32::MAX)) as u32)
    };
    let reasoning_chars = ["reasoning_content", "reasoning"]
        .iter()
        .find_map(|key| message.get(*key).and_then(Value::as_str))
        .map(|text| text.chars().count());
    Ok(Parsed {
        text: strip_reasoning(&content),
        prompt_tokens: number("prompt_tokens"),
        completion_tokens: number("completion_tokens"),
        total_tokens: number("total_tokens"),
        finish_reason: choice["finish_reason"].as_str().map(str::to_owned),
        reasoning_chars,
    })
}

fn no_text_error(parsed: &Parsed, bytes: &[u8]) -> DomainError {
    let why = match (parsed.finish_reason.as_deref(), parsed.completion_tokens) {
        (Some("length"), Some(tokens)) => format!(
            " because it used all {tokens} of its output tokens (reasoning models spend tokens thinking before they answer)"
        ),
        (Some("length"), None) => " because it ran out of output tokens".to_owned(),
        (Some(reason), _) => format!(" (it stopped because: {reason})"),
        (None, _) => String::new(),
    };
    let reasoning = parsed
        .reasoning_chars
        .map(|chars| format!(" It did send {chars} characters of reasoning in a separate field."))
        .unwrap_or_default();
    DomainError::model(
        ErrorCode::ModelServer,
        format!(
            "The model returned no text{why}.{reasoning} Try again, or choose a different model in Settings. What arrived: {}",
            shape_of(bytes)
        ),
        true,
    )
}

/// A description of a reply's structure for an error message: field names,
/// types, and lengths. It contains none of the reply's text, so it is safe to
/// show and to copy into a bug report.
pub(crate) fn shape_of(bytes: &[u8]) -> String {
    fn name(raw: &str) -> String {
        raw.chars()
            .take(30)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '?'
                }
            })
            .collect()
    }
    fn keys(value: &Value) -> String {
        match value.as_object() {
            Some(map) => {
                let mut names: Vec<String> = map.keys().take(12).map(|key| name(key)).collect();
                if map.len() > 12 {
                    names.push("…".into());
                }
                format!("[{}]", names.join(", "))
            }
            None => "none".into(),
        }
    }
    fn kind(value: Option<&Value>) -> String {
        match value {
            None => "absent".into(),
            Some(Value::Null) => "null".into(),
            Some(Value::String(text)) => format!("a string of {} characters", text.chars().count()),
            Some(Value::Array(items)) => format!("a list of {}", items.len()),
            Some(Value::Object(_)) => "an object".into(),
            Some(Value::Number(_)) => "a number".into(),
            Some(Value::Bool(_)) => "a boolean".into(),
        }
    }

    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return format!("not JSON ({} bytes)", bytes.len());
    };
    let Some(top) = value.as_object() else {
        return format!("JSON that is not an object ({} bytes)", bytes.len());
    };
    let mut parts = vec![format!("a JSON object with fields {}", keys(&value))];
    if let Some(error) = top.get("error") {
        parts.push(format!("an error field that is {}", kind(Some(error))));
    }
    match top.get("choices") {
        None => parts.push("no choices field".into()),
        Some(choices) => {
            parts.push(format!("choices is {}", kind(Some(choices))));
            if let Some(choice) = choices.get(0) {
                parts.push(format!("choice fields {}", keys(choice)));
                parts.push(format!(
                    "finish_reason is {}",
                    match choice.get("finish_reason") {
                        Some(Value::String(reason)) => name(reason),
                        other => kind(other),
                    }
                ));
                match choice.get("message") {
                    None => parts.push("no message".into()),
                    Some(message) => {
                        parts.push(format!("message fields {}", keys(message)));
                        parts.push(format!("content is {}", kind(message.get("content"))));
                        for field in ["reasoning_content", "reasoning"] {
                            if message.get(field).is_some() {
                                parts.push(format!("{field} is {}", kind(message.get(field))));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(tokens) = top
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(Value::as_u64)
    {
        parts.push(format!("{tokens} completion tokens"));
    }
    parts.join("; ")
}

/// Some reasoning models put their working in a leading `<think>` block. The
/// block is dropped so the capsule that follows is judged on its own.
pub(crate) fn strip_reasoning(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return rest[end + "</think>".len()..].trim().to_owned();
        }
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_think_block_is_removed_and_nothing_else_is() {
        assert_eq!(
            strip_reasoning("<think>hmm</think>\n{\"a\":1}"),
            "{\"a\":1}"
        );
        assert_eq!(strip_reasoning("  {\"a\":1}  "), "{\"a\":1}");
        assert_eq!(
            strip_reasoning("{\"a\":\"<think>x</think>\"}"),
            "{\"a\":\"<think>x</think>\"}"
        );
        assert_eq!(
            strip_reasoning("<think>never closed {\"a\":1}"),
            "<think>never closed {\"a\":1}"
        );
        assert_eq!(
            strip_reasoning("<think>a</think><think>b</think>{}"),
            "<think>b</think>{}"
        );
    }

    #[test]
    fn format_rejections_are_recognised_by_what_the_service_says() {
        for yes in [
            "Unsupported response_format",
            "json_schema is not supported",
            "Structured output unavailable",
            "invalid schema",
        ] {
            assert!(mentions_response_format(yes), "{yes}");
        }
        for no in ["Invalid model", "context length exceeded", "bad request"] {
            assert!(!mentions_response_format(no), "{no}");
        }
    }

    #[test]
    fn provider_messages_are_shortened_and_redacted() {
        let body = serde_json::to_vec(&json!({"error": {"message": format!("bad key sk-proj-abcdefghij1234567890ABCDEFGH {}", "x".repeat(500))}})).unwrap();
        let message = provider_message(&body).unwrap();
        assert!(message.chars().count() <= 201 + "[REDACTED:api-token]".len());
        assert!(!message.contains("sk-proj"));
        assert_eq!(provider_message(b"not json"), None);
        assert_eq!(provider_message(b"{}"), None);
    }

    #[test]
    fn a_reply_without_the_usual_structure_is_unreadable_and_described() {
        for body in [
            json!({}),
            json!({"choices": []}),
            json!({"choices": [{"index": 0}]}),
            json!({"choices": [{"message": {"content": 5}}]}),
        ] {
            let shape = parse_success(body.to_string().as_bytes()).err().unwrap();
            assert!(
                shape.starts_with("a JSON object with fields"),
                "{body}: {shape}"
            );
        }
        assert_eq!(
            parse_success(b"not json").err().unwrap(),
            "not JSON (8 bytes)"
        );
    }

    #[test]
    fn a_reply_with_no_text_is_not_an_error_by_itself() {
        // What a reasoning model sends when it runs out of tokens while thinking.
        let body = json!({
            "choices": [{"message": {"role": "assistant", "content": null, "reasoning_content": "x".repeat(300)}, "finish_reason": "length"}],
            "usage": {"completion_tokens": 64}
        });
        let parsed = parse_success(body.to_string().as_bytes()).unwrap();
        assert!(parsed.text.is_empty());
        assert_eq!(parsed.finish_reason.as_deref(), Some("length"));
        assert_eq!(parsed.reasoning_chars, Some(300));

        let message = no_text_error(&parsed, body.to_string().as_bytes()).message;
        assert!(
            message.contains("used all 64 of its output tokens"),
            "{message}"
        );
        assert!(message.contains("300 characters of reasoning"), "{message}");
        assert!(message.contains("content is null"), "{message}");
        assert!(message.contains("finish_reason is length"), "{message}");
    }

    #[test]
    fn the_shape_of_a_reply_never_contains_its_text() {
        let body = json!({
            "id": "chatcmpl-SECRET-ID-123",
            "choices": [{"message": {"content": "THE-USERS-EVIDENCE-TEXT", "reasoning": "PRIVATE-THINKING"}, "finish_reason": "stop"}],
            "usage": {"completion_tokens": 9},
            "weird key with spaces and \"quotes\"": 1
        });
        let shape = shape_of(body.to_string().as_bytes());
        for forbidden in ["SECRET-ID", "EVIDENCE-TEXT", "PRIVATE-THINKING", "quotes\""] {
            assert!(!shape.contains(forbidden), "{forbidden} in {shape}");
        }
        assert!(
            shape.contains("content is a string of 23 characters"),
            "{shape}"
        );
        assert!(
            shape.contains("reasoning is a string of 16 characters"),
            "{shape}"
        );
        assert!(shape.contains("9 completion tokens"));
        assert_eq!(shape_of(b"<html>502</html>"), "not JSON (16 bytes)");
        assert_eq!(shape_of(b"[1,2]"), "JSON that is not an object (5 bytes)");
        assert!(
            shape_of(json!({"error": {"message": "bad"}}).to_string().as_bytes())
                .contains("an error field that is an object")
        );
    }

    #[test]
    fn content_may_be_a_string_or_a_list_of_text_parts() {
        let string = json!({"choices": [{"message": {"content": "{}"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}});
        let parsed = parse_success(string.to_string().as_bytes()).unwrap();
        assert_eq!(
            (
                parsed.text.as_str(),
                parsed.prompt_tokens,
                parsed.completion_tokens,
                parsed.total_tokens
            ),
            ("{}", Some(10), Some(5), Some(15))
        );
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
        let parts = json!({"choices": [{"message": {"content": [{"type": "text", "text": "{"}, {"type": "text", "text": "}"}]}}]});
        assert_eq!(
            parse_success(parts.to_string().as_bytes()).unwrap().text,
            "{}"
        );
    }
}
