//! A real HTTP server on loopback with scripted responses, shared by the
//! model-client tests. It records every request it receives.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use serde_json::{Value, json};

#[derive(Clone)]
pub struct Scripted {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
    pub delay: Duration,
    pub hang_up: bool,
}

impl Scripted {
    pub fn ok(content: &str) -> Self {
        let body = json!({
            "id": "chatcmpl-1",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 120, "completion_tokens": 45, "total_tokens": 165}
        });
        Self {
            status: 200,
            headers: vec![],
            body: body.to_string(),
            delay: Duration::ZERO,
            hang_up: false,
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![],
            body: body.into(),
            delay: Duration::ZERO,
            hang_up: false,
        }
    }

    pub fn error(status: u16, message: &str) -> Self {
        Self::status(status, &json!({"error": {"message": message}}).to_string())
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn header(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Recorded {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

pub struct FakeServer {
    pub endpoint: String,
    recorded: Arc<Mutex<Vec<Recorded>>>,
}

impl FakeServer {
    pub fn start(responses: Vec<Scripted>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let seen = recorded.clone();
        thread::spawn(move || {
            for response in responses {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                serve(stream, response, &seen);
            }
        });
        Self { endpoint, recorded }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.recorded.lock().unwrap().clone()
    }
}

pub fn serve(mut stream: TcpStream, response: Scripted, seen: &Arc<Mutex<Vec<Recorded>>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or("").to_owned(),
        parts.next().unwrap_or("").to_owned(),
    );
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_lowercase(), value.trim().to_owned());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).unwrap();
    seen.lock().unwrap().push(Recorded {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    });

    thread::sleep(response.delay);
    if response.hang_up {
        return;
    }
    let mut text = format!(
        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n",
        response.status,
        response.body.len()
    );
    for (name, value) in &response.headers {
        text.push_str(&format!("{name}: {value}\r\n"));
    }
    text.push_str("\r\n");
    let _ = stream.write_all(text.as_bytes());
    let _ = stream.write_all(response.body.as_bytes());
}
