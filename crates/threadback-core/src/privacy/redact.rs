use std::sync::OnceLock;

use regex::{Regex, RegexBuilder, RegexSet};
use serde_json::Value;

use crate::DomainError;

pub const MAX_CUSTOM_PATTERNS: usize = 50;
pub const MAX_PATTERN_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FindingKind {
    PrivateKey,
    BearerToken,
    UrlPassword,
    UrlToken,
    ProviderToken,
    Jwt,
    SecretAssignment,
    Custom,
    /// A home-directory prefix, rewritten to `~`. Not a secret.
    HomePath,
}

impl FindingKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::PrivateKey => "private-key",
            Self::BearerToken => "bearer-token",
            Self::UrlPassword => "url-password",
            Self::UrlToken => "url-token",
            Self::ProviderToken => "api-token",
            Self::Jwt => "jwt",
            Self::SecretAssignment => "secret-value",
            Self::Custom => "custom",
            Self::HomePath => "home-path",
        }
    }

    /// Everything except a rewritten home path is a credential.
    pub fn is_secret(self) -> bool {
        self != Self::HomePath
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Redaction {
    pub text: String,
    pub findings: Vec<Finding>,
    /// A private-key block began and the text ended before it did. A streaming
    /// caller must keep dropping lines until the block ends.
    pub open_private_key: bool,
}

impl Redaction {
    /// Credentials replaced, not counting rewritten home paths.
    pub fn secret_count(&self) -> u32 {
        self.findings
            .iter()
            .filter(|finding| finding.kind.is_secret())
            .map(|finding| finding.count)
            .sum()
    }

    pub fn has_secret(&self) -> bool {
        self.secret_count() > 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Replacement {
    Marker,
    Tilde,
}

struct Rule {
    kind: FindingKind,
    regex: Regex,
    /// Judges the captured text; `None` accepts every match.
    validate: Option<fn(&str) -> bool>,
    replacement: Replacement,
}

struct Span {
    start: usize,
    end: usize,
    kind: FindingKind,
    replacement: Replacement,
}

/// Replaces credentials and personal paths with fixed markers. It is
/// deterministic and idempotent: redacting redacted text changes nothing.
pub struct Redactor {
    rules: Vec<Rule>,
    /// Quick check for whether any rule can match at all.
    any_rule: RegexSet,
    key_begin: Regex,
    key_end: Regex,
}

fn compile(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .size_limit(1 << 22)
        .build()
        .unwrap_or_else(|error| panic!("built-in redaction pattern {pattern:?}: {error}"))
}

fn rule(kind: FindingKind, pattern: &str, validate: Option<fn(&str) -> bool>) -> Rule {
    Rule {
        kind,
        regex: compile(pattern),
        validate,
        replacement: Replacement::Marker,
    }
}

fn home_rule(pattern: &str) -> Rule {
    Rule {
        kind: FindingKind::HomePath,
        regex: compile(pattern),
        validate: None,
        replacement: Replacement::Tilde,
    }
}

fn has_digit(text: &str) -> bool {
    text.bytes().any(|byte| byte.is_ascii_digit())
}

fn has_digit_or_symbol(text: &str) -> bool {
    text.bytes()
        .any(|byte| byte.is_ascii_digit() || b"._~+/=-".contains(&byte))
}

/// Judges the value in `name = value`. It fails closed: only obvious
/// non-secrets (placeholders, literals, references) are left alone.
fn looks_like_secret_value(value: &str) -> bool {
    let bare = value.trim_matches(|c| c == '"' || c == '\'');
    let lower = bare.to_ascii_lowercase();
    const LITERALS: [&str; 16] = [
        "true",
        "false",
        "null",
        "none",
        "nil",
        "undefined",
        "required",
        "optional",
        "string",
        "number",
        "bool",
        "boolean",
        "empty",
        "hidden",
        "masked",
        "redacted",
    ];
    if LITERALS.contains(&lower.as_str()) || lower.starts_with("[redacted") {
        return false;
    }
    let placeholder = bare.starts_with('$')
        || bare.starts_with('<')
        || bare.starts_with("{{")
        || bare.starts_with('%')
        || bare.starts_with("${");
    let reference = lower.starts_with("process.env")
        || lower.starts_with("os.environ")
        || lower.starts_with("env.")
        || bare.contains('(')
        || bare.contains('[')
        || bare.contains("=>");
    !(placeholder || reference)
}

fn builtin_rules() -> Vec<Rule> {
    let mut rules = vec![
        rule(
            FindingKind::BearerToken,
            r#"(?i)\b(?:proxy-)?authorization["']?\s*[:=]\s*["']?(?:bearer|basic|token|digest)\s+(?P<secret>[^\s"',;]+)"#,
            None,
        ),
        rule(
            FindingKind::BearerToken,
            r"(?i)\bbearer\s+(?P<secret>[A-Za-z0-9._~+/=-]{8,})",
            Some(has_digit_or_symbol),
        ),
        rule(
            FindingKind::UrlPassword,
            r"\b[A-Za-z][A-Za-z0-9+.-]*://[^/\s:@]+:(?P<secret>[^/\s@]+)@",
            None,
        ),
        rule(
            FindingKind::UrlToken,
            r#"(?i)[?&](?:access_token|refresh_token|id_token|token|api_key|apikey|key|secret|password|passwd|sig|signature|auth)=(?P<secret>[^&\s"']+)"#,
            None,
        ),
        rule(
            FindingKind::Jwt,
            r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
            None,
        ),
        rule(FindingKind::ProviderToken, r"\bsk-[A-Za-z0-9_-]{20,}", None),
        rule(
            FindingKind::ProviderToken,
            r"\bgh[pousr]_[A-Za-z0-9]{30,}",
            None,
        ),
        rule(
            FindingKind::ProviderToken,
            r"\bgithub_pat_[A-Za-z0-9_]{40,}",
            None,
        ),
        rule(
            FindingKind::ProviderToken,
            r"\bglpat-[A-Za-z0-9_-]{20,}",
            None,
        ),
        rule(
            FindingKind::ProviderToken,
            r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
            None,
        ),
        rule(
            FindingKind::ProviderToken,
            r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
            None,
        ),
        rule(FindingKind::ProviderToken, r"\bAIza[0-9A-Za-z_-]{35}", None),
        rule(FindingKind::ProviderToken, r"\bnpm_[A-Za-z0-9]{36}\b", None),
        rule(FindingKind::ProviderToken, r"\bhf_[A-Za-z0-9]{30,}", None),
        rule(
            FindingKind::ProviderToken,
            r"\bSG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}",
            None,
        ),
        // `<prefix>_<environment>_<body>` keys such as sk_live_..., and
        // ThreadBack's own demo credential. Requiring a digit in the body
        // keeps ordinary snake_case identifiers out.
        rule(
            FindingKind::ProviderToken,
            r"\b[A-Za-z]{2,8}_(?:live|test|demo|prod|secret)_[A-Za-z0-9_]{8,}",
            Some(has_digit),
        ),
        rule(
            FindingKind::SecretAssignment,
            r#"(?i)\b[A-Za-z0-9_.-]*(?:password|passwd|pwd|secret|token|api[_-]?key|apikey|access[_-]?key|private[_-]?key|credential|auth[_-]?key)[A-Za-z0-9_.-]*["']?\s*[:=]\s*(?P<secret>"[^"\n]{4,}"|'[^'\n]{4,}'|[^\s"',;&]{4,})"#,
            Some(looks_like_secret_value),
        ),
        home_rule(r#"(?P<secret>/(?:Users|home)/[^/\s:"'\\]+)"#),
        home_rule(r#"(?P<secret>[A-Za-z]:\\Users\\[^\\\s:"']+)"#),
        home_rule(r"(?P<secret>/root)(?:/|\b)"),
    ];
    // The current user's actual home, wherever it is.
    if let Some(home) = std::env::var_os("HOME").and_then(|home| home.into_string().ok()) {
        let home = home.trim_end_matches('/');
        if home.len() >= 4 && home.starts_with('/') {
            rules.push(home_rule(&format!(
                "(?P<secret>{})(?:/|\\b)",
                regex::escape(home)
            )));
        }
    }
    rules
}

impl Redactor {
    fn from_rules(rules: Vec<Rule>) -> Self {
        let any_rule = RegexSet::new(rules.iter().map(|rule| rule.regex.as_str()))
            .expect("redaction patterns compile together");
        Self {
            rules,
            any_rule,
            key_begin: compile(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----"),
            key_end: compile(r"-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----"),
        }
    }

    /// The rules every ThreadBack process applies before anything is stored.
    pub fn builtin() -> &'static Redactor {
        static BUILTIN: OnceLock<Redactor> = OnceLock::new();
        BUILTIN.get_or_init(|| Self::from_rules(builtin_rules()))
    }

    /// Built-in rules plus user-configured patterns (regular expressions).
    pub fn new(custom_patterns: &[String]) -> Result<Self, DomainError> {
        if custom_patterns.len() > MAX_CUSTOM_PATTERNS {
            return Err(DomainError::invalid_input(
                format!("At most {MAX_CUSTOM_PATTERNS} custom patterns are allowed."),
                "redactionPatterns",
            ));
        }
        let mut rules = builtin_rules();
        for pattern in custom_patterns {
            let trimmed = pattern.trim();
            if trimmed.is_empty() || trimmed.chars().count() > MAX_PATTERN_CHARS {
                return Err(DomainError::invalid_input(
                    format!("A custom pattern must be 1 to {MAX_PATTERN_CHARS} characters."),
                    "redactionPatterns",
                ));
            }
            let regex = RegexBuilder::new(trimmed)
                .size_limit(1 << 18)
                .build()
                .map_err(|_| {
                    DomainError::invalid_input(
                        format!("'{trimmed}' is not a valid pattern."),
                        "redactionPatterns",
                    )
                })?;
            if regex.is_match("") {
                return Err(DomainError::invalid_input(
                    format!("'{trimmed}' matches empty text, so it would redact everything."),
                    "redactionPatterns",
                ));
            }
            rules.push(Rule {
                kind: FindingKind::Custom,
                regex,
                validate: None,
                replacement: Replacement::Marker,
            });
        }
        Ok(Self::from_rules(rules))
    }

    /// A cheap test that is false only when `redact` would change nothing, so
    /// the common line of ordinary output can skip the full pass.
    pub(crate) fn may_need_redaction(&self, text: &str) -> bool {
        text.contains("-----") || self.any_rule.is_match(text)
    }

    pub fn redact(&self, text: &str) -> Redaction {
        let mut spans = Vec::new();
        let open_private_key = self.private_key_spans(text, &mut spans);

        if self.any_rule.is_match(text) {
            for index in self.any_rule.matches(text) {
                let rule = &self.rules[index];
                for captures in rule.regex.captures_iter(text) {
                    let found = captures
                        .name("secret")
                        .or_else(|| captures.get(0))
                        .expect("group 0 always exists");
                    // Already replaced on an earlier pass.
                    if found.as_str().starts_with("[REDACTED:") {
                        continue;
                    }
                    if rule.validate.is_some_and(|accept| !accept(found.as_str())) {
                        continue;
                    }
                    spans.push(Span {
                        start: found.start(),
                        end: found.end(),
                        kind: rule.kind,
                        replacement: rule.replacement,
                    });
                }
            }
        }
        if spans.is_empty() {
            return Redaction {
                text: text.to_owned(),
                findings: Vec::new(),
                open_private_key,
            };
        }

        // Overlapping spans merge into one that covers all of them, so no
        // character of any match survives.
        spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        let mut merged: Vec<Span> = Vec::new();
        for span in spans {
            match merged.last_mut() {
                Some(last) if span.start < last.end => last.end = last.end.max(span.end),
                _ => merged.push(span),
            }
        }

        let mut output = String::with_capacity(text.len());
        let mut cursor = 0;
        let mut counts: Vec<(FindingKind, u32)> = Vec::new();
        for span in &merged {
            output.push_str(&text[cursor..span.start]);
            match span.replacement {
                Replacement::Tilde => output.push('~'),
                Replacement::Marker => {
                    output.push_str("[REDACTED:");
                    output.push_str(span.kind.label());
                    output.push(']');
                }
            }
            cursor = span.end;
            match counts.iter_mut().find(|(kind, _)| *kind == span.kind) {
                Some((_, count)) => *count += 1,
                None => counts.push((span.kind, 1)),
            }
        }
        output.push_str(&text[cursor..]);
        counts.sort();
        Redaction {
            text: output,
            findings: counts
                .into_iter()
                .map(|(kind, count)| Finding { kind, count })
                .collect(),
            open_private_key,
        }
    }

    /// Credentials still present in `text`, for the check made just before
    /// anything is sent.
    pub fn find_secrets(&self, text: &str) -> Vec<Finding> {
        self.redact(text)
            .findings
            .into_iter()
            .filter(|finding| finding.kind.is_secret())
            .collect()
    }

    /// The first match of an `END ... PRIVATE KEY` marker in `text`, as the
    /// offset just past it.
    pub(crate) fn private_key_end(&self, text: &str) -> Option<usize> {
        self.key_end.find(text).map(|found| found.end())
    }

    /// Whole private-key blocks, an unterminated block through the end of the
    /// text, and an unmatched END marker back to the start. Returns whether a
    /// block was left open.
    fn private_key_spans(&self, text: &str, spans: &mut Vec<Span>) -> bool {
        if !text.contains("-----") {
            return false;
        }
        let mut markers: Vec<(usize, usize, bool)> = self
            .key_begin
            .find_iter(text)
            .map(|found| (found.start(), found.end(), true))
            .chain(
                self.key_end
                    .find_iter(text)
                    .map(|found| (found.start(), found.end(), false)),
            )
            .collect();
        markers.sort();

        let mut open_from: Option<usize> = None;
        let mut consumed = 0;
        for (start, end, is_begin) in markers {
            let span = |from: usize, to: usize| Span {
                start: from,
                end: to,
                kind: FindingKind::PrivateKey,
                replacement: Replacement::Marker,
            };
            match (is_begin, open_from) {
                (true, None) => open_from = Some(start),
                (true, Some(_)) => {}
                (false, Some(from)) => {
                    spans.push(span(from, end));
                    open_from = None;
                    consumed = end;
                }
                (false, None) => {
                    spans.push(span(consumed, end));
                    consumed = end;
                }
            }
        }
        match open_from {
            Some(from) => {
                spans.push(Span {
                    start: from,
                    end: text.len(),
                    kind: FindingKind::PrivateKey,
                    replacement: Replacement::Marker,
                });
                true
            }
            None => false,
        }
    }
}

/// Redacts every string inside a JSON value, in place, and reports what was
/// found. Object keys are left alone.
pub fn redact_value(redactor: &Redactor, value: &mut Value) -> Vec<Finding> {
    let mut totals: Vec<Finding> = Vec::new();
    walk(redactor, value, &mut totals);
    totals.sort_by_key(|finding| finding.kind);
    totals
}

fn walk(redactor: &Redactor, value: &mut Value, totals: &mut Vec<Finding>) {
    match value {
        Value::String(text) => {
            let result = redactor.redact(text);
            if !result.findings.is_empty() {
                *text = result.text;
                for finding in result.findings {
                    match totals.iter_mut().find(|total| total.kind == finding.kind) {
                        Some(total) => total.count += finding.count,
                        None => totals.push(finding),
                    }
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| walk(redactor, item, totals)),
        Value::Object(map) => map
            .values_mut()
            .for_each(|item| walk(redactor, item, totals)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn redact(text: &str) -> Redaction {
        Redactor::builtin().redact(text)
    }

    /// No stretch of six or more characters of the secret survives.
    fn assert_no_leak(secret: &str, output: &str) {
        assert!(!output.contains(secret), "secret survived in {output:?}");
        let chars: Vec<char> = secret.chars().collect();
        for window in chars.windows(6.min(chars.len())) {
            let piece: String = window.iter().collect();
            assert!(
                !output.contains(&piece),
                "fragment {piece:?} of the secret survived in {output:?}"
            );
        }
    }

    fn kinds(redaction: &Redaction) -> Vec<FindingKind> {
        redaction
            .findings
            .iter()
            .map(|finding| finding.kind)
            .collect()
    }

    const PEM: &str = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAqZ3k0W1d7YtR2vXn\nL9pQz8sT4uVwXyZaBcDeFgHiJkLmNoPq\n-----END RSA PRIVATE KEY-----";

    #[test]
    fn plain_text_is_left_alone() {
        let text = "FAIL payment retry: expected exactly one provider call, received two\nrun_test_suite_ok payment_test_retry_flow";
        let result = redact(text);
        assert_eq!(result.text, text);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn private_key_blocks_are_removed_whole() {
        let result = redact(&format!("before\n{PEM}\nafter"));
        assert_eq!(result.text, "before\n[REDACTED:private-key]\nafter");
        assert_eq!(kinds(&result), [FindingKind::PrivateKey]);
        assert!(!result.open_private_key);
        assert_no_leak("MIIEowIBAAKCAQEAqZ3k0W1d7YtR2vXn", &result.text);
    }

    #[test]
    fn pgp_and_openssh_blocks_are_recognised() {
        for header in [
            "PGP PRIVATE KEY BLOCK",
            "OPENSSH PRIVATE KEY",
            "EC PRIVATE KEY",
            "PRIVATE KEY",
        ] {
            let text = format!(
                "-----BEGIN {header}-----\nAAAAB3NzaC1yc2EAAAADAQABAAAB\n-----END {header}-----"
            );
            assert_eq!(redact(&text).text, "[REDACTED:private-key]", "{header}");
        }
    }

    #[test]
    fn an_unterminated_block_is_cut_to_the_end_and_reported_open() {
        let result = redact(
            "log line\n-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEF\nAASCBKcwggSj",
        );
        assert_eq!(result.text, "log line\n[REDACTED:private-key]");
        assert!(result.open_private_key);
    }

    #[test]
    fn an_unmatched_end_marker_removes_everything_before_it() {
        let result = redact(
            "MIIEvQIBADANBgkqhkiG9w0BAQEF\nAASCBKcwggSjAgEAAoIBAQC7\n-----END PRIVATE KEY-----\nnext",
        );
        assert_eq!(result.text, "[REDACTED:private-key]\nnext");
    }

    #[test]
    fn two_blocks_and_surrounding_text() {
        let text = format!("a\n{PEM}\nb\n{PEM}\nc");
        let result = redact(&text);
        assert_eq!(
            result.text,
            "a\n[REDACTED:private-key]\nb\n[REDACTED:private-key]\nc"
        );
        assert_eq!(
            result.findings,
            [Finding {
                kind: FindingKind::PrivateKey,
                count: 2
            }]
        );
    }

    #[test]
    fn authorization_headers_keep_their_scheme_and_lose_the_credential() {
        for (input, expected) in [
            (
                "Authorization: Bearer nbf_demo_7F4A91C2_DO_NOT_SEND",
                "Authorization: Bearer [REDACTED:bearer-token]",
            ),
            (
                "authorization=Basic dXNlcjpwYXNzd29yZA==",
                "authorization=Basic [REDACTED:bearer-token]",
            ),
            (
                r#"{"Authorization": "Token abc123def456"}"#,
                r#"{"Authorization": "Token [REDACTED:bearer-token]"}"#,
            ),
            (
                "curl -H 'Proxy-Authorization: Bearer abcDEF123' x",
                "curl -H 'Proxy-Authorization: Bearer [REDACTED:bearer-token]' x",
            ),
        ] {
            let result = redact(input);
            assert_eq!(result.text, expected, "{input}");
            assert!(result.has_secret());
        }
    }

    #[test]
    fn the_fixtures_fake_credential_never_survives_in_any_position() {
        let secret = "nbf_demo_7F4A91C2_DO_NOT_SEND";
        for text in [
            format!("provider request: Authorization: Bearer {secret}"),
            format!("diagnostic token: {secret}"),
            format!("bare {secret} in a sentence"),
            format!("{secret}"),
            format!("x={secret}&y=1"),
            format!("\"{secret}\""),
        ] {
            let result = redact(&text);
            assert_no_leak(secret, &result.text);
            assert!(result.has_secret(), "{text}");
        }
    }

    #[test]
    fn a_bearer_word_in_prose_is_not_a_credential() {
        let text = "the bearer instruments were sold; bearer of bad news";
        assert_eq!(redact(text).text, text);
        assert_eq!(
            redact("Bearer abc12345xyz").text,
            "Bearer [REDACTED:bearer-token]"
        );
    }

    #[test]
    fn urls_lose_embedded_passwords_and_query_tokens() {
        let result = redact("connect postgres://admin:s3cr3tP4ssw0rd@db.internal:5432/app now");
        assert_eq!(
            result.text,
            "connect postgres://admin:[REDACTED:url-password]@db.internal:5432/app now"
        );
        assert_eq!(kinds(&result), [FindingKind::UrlPassword]);

        let result = redact(
            "GET https://api.example.com/v1/items?page=2&access_token=ya29.A0ARrdaM-9xZ&limit=5",
        );
        assert_eq!(
            result.text,
            "GET https://api.example.com/v1/items?page=2&access_token=[REDACTED:url-token]&limit=5"
        );
        assert_eq!(
            redact("https://example.com/docs?page=2").text,
            "https://example.com/docs?page=2"
        );
        assert_eq!(
            redact("https://example.com/a@b").text,
            "https://example.com/a@b"
        );
    }

    #[test]
    fn well_known_provider_tokens_are_recognised() {
        let tokens = [
            "sk-proj-abcdefghij1234567890ABCDEFGH",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "github_pat_11ABCDEFG0abcdefghijklmnopqrstuvwxyz0123456789ABCDEF",
            "glpat-abcdefghij1234567890",
            "xoxb-1234567890-abcdefghijkl",
            "AKIAIOSFODNN7EXAMPLE",
            "AIzaSyA-abcdefghijklmnopqrstuvwxyz01234",
            "npm_abcdefghijklmnopqrstuvwxyz0123456789",
            "sk_live_51HxYzABCdef456GHI",
            "hf_abcdefghijklmnopqrstuvwxyzABCDEF0123",
            "SG.abcdefghijklmnop12.qrstuvwxyzABCDEFGH34",
        ];
        for token in tokens {
            let result = redact(&format!("value is {token} here"));
            assert_eq!(result.text, "value is [REDACTED:api-token] here", "{token}");
            assert_no_leak(token, &result.text);
        }
    }

    #[test]
    fn jwts_are_recognised() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let result = redact(&format!("token {jwt} end"));
        assert!(result.text.contains("[REDACTED:"));
        assert_no_leak(jwt, &result.text);
    }

    #[test]
    fn prefixed_keys_need_a_digit_so_identifiers_are_not_caught() {
        for text in [
            "payment_test_retry_flow",
            "unit_test_helpers_module",
            "my_secret_santa_list",
        ] {
            assert_eq!(redact(text).text, text);
        }
        assert!(redact("rk_live_abcd1234efgh").has_secret());
    }

    #[test]
    fn secret_assignments_lose_only_the_value() {
        for (input, expected) in [
            ("password=hunter2", "password=[REDACTED:secret-value]"),
            (
                "DB_PASSWORD: correct-horse-battery",
                "DB_PASSWORD: [REDACTED:secret-value]",
            ),
            (
                r#"export STRIPE_SECRET_KEY="whsec value 123""#,
                "export STRIPE_SECRET_KEY=[REDACTED:secret-value]",
            ),
            (
                r#"{"client_secret": "x9y8z7w6v5"}"#,
                r#"{"client_secret": [REDACTED:secret-value]}"#,
            ),
            (
                "api_key = 'abc-123-def'",
                "api_key = [REDACTED:secret-value]",
            ),
            (
                "aws_secret_access_key=wJalrXUtnFEMI/K7MDENG/bPxRfiCY",
                "aws_secret_access_key=[REDACTED:secret-value]",
            ),
        ] {
            assert_eq!(redact(input).text, expected, "{input}");
        }
    }

    #[test]
    fn assignments_that_are_not_secrets_are_left_alone() {
        for text in [
            "token_count=5",
            "password: true",
            "api_key=$API_KEY",
            "token = process.env.TOKEN",
            "secret=<your secret here>",
            "password = getPassword()",
            "tokens: [1,2,3]",
            "author=Charan",
            "password=[REDACTED:secret-value]",
        ] {
            assert_eq!(redact(text).text, text, "{text}");
        }
    }

    #[test]
    fn home_directories_become_a_tilde_and_are_not_counted_as_secrets() {
        let result = redact(
            "error at /Users/charansriteja/Documents/threadback/src/a.rs:4 and /home/ci/app and C:\\Users\\Charan\\proj\\b.txt",
        );
        assert_eq!(
            result.text,
            "error at ~/Documents/threadback/src/a.rs:4 and ~/app and ~\\proj\\b.txt"
        );
        assert_eq!(kinds(&result), [FindingKind::HomePath]);
        assert!(!result.has_secret());
        assert!(Redactor::builtin().find_secrets("/Users/x/y").is_empty());
        assert!(
            !redact("see /Users/charansriteja/x")
                .text
                .contains("charansriteja")
        );
    }

    #[test]
    fn redaction_is_idempotent() {
        let text = format!(
            "Authorization: Bearer nbf_demo_7F4A91C2_DO_NOT_SEND\n{PEM}\npostgres://a:b1c2d3e4@h/x\npassword=hunter2 /Users/me/p ghp_abcdefghijklmnopqrstuvwxyz0123456789"
        );
        let once = redact(&text);
        let twice = redact(&once.text);
        assert_eq!(once.text, twice.text);
        assert!(twice.findings.is_empty());
    }

    #[test]
    fn overlapping_matches_merge_without_leaving_fragments() {
        let secret = "Zq8vKd2mXw9PtL4nB6";
        let result = redact(&format!(
            "Authorization: Bearer {secret}1 password=hunter22 token={secret}2"
        ));
        assert_no_leak(secret, &result.text);
    }

    #[test]
    fn custom_patterns_are_applied_in_addition() {
        let redactor = Redactor::new(&["ACME-[0-9]{6}".into(), r"customer-\w+".into()]).unwrap();
        let result = redactor.redact("ticket ACME-123456 for customer-bob");
        assert_eq!(
            result.text,
            "ticket [REDACTED:custom] for [REDACTED:custom]"
        );
        assert_eq!(
            result.findings,
            [Finding {
                kind: FindingKind::Custom,
                count: 2
            }]
        );
        assert!(
            redactor.redact("password=hunter2").has_secret(),
            "built-in rules still apply"
        );
    }

    #[test]
    fn bad_custom_patterns_are_rejected_with_a_reason() {
        let bad = |pattern: &str| Redactor::new(&[pattern.into()]).err().unwrap();
        assert!(bad("(unclosed").message.contains("not a valid pattern"));
        assert!(bad("   ").message.contains("1 to 200"));
        assert!(bad(&"a".repeat(201)).message.contains("1 to 200"));
        assert!(bad("a*").message.contains("matches empty text"));
        let many: Vec<String> = (0..51).map(|i| format!("x{i}")).collect();
        assert!(
            Redactor::new(&many)
                .err()
                .unwrap()
                .message
                .contains("At most 50")
        );
        assert_eq!(bad("(unclosed").field.as_deref(), Some("redactionPatterns"));
    }

    #[test]
    fn json_values_are_redacted_through_every_string() {
        let mut value = json!({
            "text": "token=abcd1234efgh",
            "list": ["fine", "Authorization: Bearer abc123def456"],
            "nested": { "command": "curl postgres://u:p4ssw0rd9@h/db" },
            "number": 5,
        });
        let findings = redact_value(Redactor::builtin(), &mut value);
        let serialized = value.to_string();
        for leaked in ["abcd1234efgh", "abc123def456", "p4ssw0rd9"] {
            assert!(!serialized.contains(leaked), "{leaked} in {serialized}");
        }
        assert_eq!(value["number"], 5);
        assert_eq!(value["list"][0], "fine");
        assert_eq!(findings.iter().map(|f| f.count).sum::<u32>(), 3);
    }

    #[test]
    fn long_and_odd_inputs_are_handled() {
        let long = "word ".repeat(200_000);
        assert_eq!(redact(&long).text.len(), long.len());
        assert_eq!(redact("").text, "");
        assert_eq!(
            redact("日本語のテキスト password=パスワード1234").text,
            "日本語のテキスト password=[REDACTED:secret-value]"
        );
    }
}
