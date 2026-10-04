use super::redact::Redactor;

/// A line this long with no newline is processed as it is, so memory stays
/// bounded for output that never breaks lines.
const MAX_PENDING_BYTES: usize = 1 << 20;

/// Redacts a byte stream line by line as it arrives.
///
/// Only the copy that is stored passes through here; the terminal still gets
/// the raw output. Redacting before any head/tail cut matters: a cut can
/// split a credential in two, and neither half would look like one.
///
/// Output that contains a NUL byte is treated as binary and passed through
/// untouched, because binary output is never stored.
pub struct StreamRedactor {
    redactor: &'static Redactor,
    pending: Vec<u8>,
    /// Inside a private-key block: lines are dropped until it ends.
    in_key: bool,
    redactions: u32,
}

impl Default for StreamRedactor {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamRedactor {
    pub fn new() -> Self {
        Self {
            redactor: Redactor::builtin(),
            pending: Vec::new(),
            in_key: false,
            redactions: 0,
        }
    }

    /// Credentials replaced so far.
    pub fn redactions(&self) -> u32 {
        self.redactions
    }

    /// Feeds more output and returns what can be released now: every
    /// complete line, redacted.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        let data = std::mem::take(&mut self.pending);
        let mut released = Vec::with_capacity(data.len());

        let mut start = 0;
        while let Some(offset) = memchr::memchr(b'\n', &data[start..]) {
            let end = start + offset + 1;
            self.line(&data[start..end], &mut released);
            start = end;
        }
        let rest = &data[start..];
        if rest.len() > MAX_PENDING_BYTES {
            self.line(rest, &mut released);
        } else {
            self.pending = rest.to_vec();
        }
        released
    }

    /// Releases the final line, which may have no newline.
    pub fn finish(&mut self) -> Vec<u8> {
        let line = std::mem::take(&mut self.pending);
        let mut released = Vec::new();
        if !line.is_empty() {
            self.line(&line, &mut released);
        }
        released
    }

    fn line(&mut self, raw: &[u8], out: &mut Vec<u8>) {
        if memchr::memchr(0, raw).is_some() {
            out.extend_from_slice(raw);
            return;
        }
        // Ordinary output takes this path: valid text that no rule can match
        // is copied through without being rebuilt.
        if !self.in_key {
            if let Ok(text) = std::str::from_utf8(raw) {
                let (body, _) = split_line_ending(text);
                if !self.redactor.may_need_redaction(body) {
                    out.extend_from_slice(raw);
                    return;
                }
            }
        }

        let text = String::from_utf8_lossy(raw);
        let (body, ending) = split_line_ending(&text);

        let remainder = if self.in_key {
            match self.redactor.private_key_end(body) {
                Some(end) => {
                    self.in_key = false;
                    // The marker line's own ending was already written after
                    // the replacement on the BEGIN line.
                    if body[end..].trim().is_empty() {
                        return;
                    }
                    &body[end..]
                }
                None => return,
            }
        } else {
            body
        };

        let result = self.redactor.redact(remainder);
        self.redactions += result.secret_count();
        if result.open_private_key {
            self.in_key = true;
        }
        out.extend_from_slice(result.text.as_bytes());
        out.extend_from_slice(ending.as_bytes());
    }
}

fn split_line_ending(text: &str) -> (&str, &str) {
    if let Some(body) = text.strip_suffix("\r\n") {
        (body, "\r\n")
    } else if let Some(body) = text.strip_suffix('\n') {
        (body, "\n")
    } else {
        (text, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&[u8]]) -> (String, u32) {
        let mut stream = StreamRedactor::new();
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend(stream.push(chunk));
        }
        out.extend(stream.finish());
        (String::from_utf8(out).unwrap(), stream.redactions())
    }

    const SECRET: &str = "nbf_demo_7F4A91C2_DO_NOT_SEND";

    #[test]
    fn redacts_each_line_and_keeps_the_rest_byte_for_byte() {
        let input = format!("start\nAuthorization: Bearer {SECRET}\r\nend without newline");
        let (output, count) = run(&[input.as_bytes()]);
        assert_eq!(
            output,
            "start\nAuthorization: Bearer [REDACTED:bearer-token]\r\nend without newline"
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn the_result_does_not_depend_on_where_the_chunks_split() {
        let key = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEF\nAASCBKcwggSjAgEAAoIBAQC7\n-----END PRIVATE KEY-----";
        let input =
            format!("one\nAuthorization: Bearer {SECRET}\n{key}\npassword=hunter2\nlast {SECRET}");
        let bytes = input.as_bytes();
        let (whole, whole_count) = run(&[bytes]);
        assert!(
            !whole.contains(SECRET) && !whole.contains("MIIEvQ"),
            "{whole}"
        );

        for split in 0..bytes.len() {
            let (output, count) = run(&[&bytes[..split], &bytes[split..]]);
            assert_eq!(output, whole, "split at {split}");
            assert_eq!(count, whole_count, "split at {split}");
        }
        let singles: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(run(&singles).0, whole);
    }

    #[test]
    fn a_private_key_spanning_many_lines_is_dropped_and_the_text_after_it_survives() {
        let mut input = String::from("before\n-----BEGIN RSA PRIVATE KEY-----\n");
        for line in 0..50 {
            input.push_str(&format!("MIIEowIBAAKCAQEA{line:04}xYzAbCdEfGhIjKl\n"));
        }
        input.push_str("-----END RSA PRIVATE KEY-----\nafter\n");
        let (output, count) = run(&[input.as_bytes()]);
        assert_eq!(output, "before\n[REDACTED:private-key]\nafter\n");
        assert_eq!(count, 1);
    }

    #[test]
    fn a_key_that_never_ends_removes_the_rest_of_the_stream() {
        let (output, _) =
            run(&[b"log\n-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkq\nAASCBKcwggSjAgEA\n"]);
        assert_eq!(output, "log\n[REDACTED:private-key]\n");
    }

    #[test]
    fn text_on_the_end_marker_line_after_the_key_is_kept_and_redacted() {
        let input =
            format!("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY----- then {SECRET}\n");
        let (output, _) = run(&[input.as_bytes()]);
        assert_eq!(
            output,
            "[REDACTED:private-key]\n then [REDACTED:api-token]\n"
        );
    }

    #[test]
    fn binary_output_is_passed_through_untouched() {
        let mut stream = StreamRedactor::new();
        let input = b"ab\0cd password=hunter2\n";
        let mut out = stream.push(input);
        out.extend(stream.finish());
        assert_eq!(out, input);
        assert_eq!(stream.redactions(), 0);
    }

    #[test]
    fn a_line_that_never_ends_is_released_in_bounded_memory() {
        let mut stream = StreamRedactor::new();
        let mut released = 0;
        for _ in 0..40 {
            released += stream.push(&vec![b'x'; 100_000]).len();
        }
        released += stream.finish().len();
        assert_eq!(released, 4_000_000);
    }

    #[test]
    fn invalid_utf8_does_not_panic_or_stop_redaction() {
        let mut input = b"bad \xff\xfe byte\npassword=hunter2\n".to_vec();
        input.extend_from_slice(b"ok\n");
        let mut stream = StreamRedactor::new();
        let mut out = stream.push(&input);
        out.extend(stream.finish());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("password=[REDACTED:secret-value]") && text.ends_with("ok\n"));
    }

    #[test]
    fn the_empty_stream_is_empty() {
        assert_eq!(run(&[]), (String::new(), 0));
    }
}
