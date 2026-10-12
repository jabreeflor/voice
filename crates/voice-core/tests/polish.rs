//! The optional local-LLM polish pass, against a mock OpenAI-compatible
//! server on 127.0.0.1. Every failure mode must come back as an `Err` (the
//! app then pastes the un-polished text) within the timeout, and nothing
//! may be sent to a non-loopback host.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use voice_core::polish::{
    accept_reply, is_loopback_url, polish, PolishError, DEFAULT_POLISH_URL, POLISH_SYSTEM_PROMPT,
};

const MODEL: &str = "llama3.2:3b";
const SHORT: Duration = Duration::from_millis(800);

struct Mock {
    url: String,
    requests: mpsc::Receiver<(String, Value)>,
}

/// One-shot server: answers the first request with `status` and `body`
/// after `delay`, and hands back (path, parsed JSON body).
fn mock(status: u16, body: &'static str, delay: Duration) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        if let Some(req) = read_request(&mut stream) {
            let _ = tx.send(req);
        }
        thread::sleep(delay);
        let reply = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes());
    });
    Mock {
        url: format!("http://127.0.0.1:{port}/v1/chat/completions"),
        requests: rx,
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, Value)> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let path = line.split_whitespace().nth(1)?.to_string();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        if h == "\r\n" || h.is_empty() {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    Some((path, serde_json::from_slice(&body).ok()?))
}

fn completion(content: &str) -> String {
    serde_json::json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
        .to_string()
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[test]
fn polished_text_is_returned_and_request_is_openai_shaped() {
    let m = mock(
        200,
        leak(completion("So I think we should ship it on Friday.")),
        Duration::ZERO,
    );
    let out = polish(
        "so i think we should ship it on friday",
        &m.url,
        MODEL,
        SHORT,
    );
    assert_eq!(
        out.as_deref(),
        Ok("So I think we should ship it on Friday.")
    );

    let (path, body) = m
        .requests
        .recv_timeout(Duration::from_secs(2))
        .expect("request");
    assert_eq!(path, "/v1/chat/completions");
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["stream"], false);
    assert_eq!(body["temperature"], 0);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], POLISH_SYSTEM_PROMPT);
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(
        body["messages"][1]["content"],
        "so i think we should ship it on friday"
    );
    assert!(body["max_tokens"].as_u64().is_some_and(|n| n >= 64));
}

#[test]
fn http_error_status_falls_back() {
    let m = mock(404, r#"{"error":"model not found"}"#, Duration::ZERO);
    assert_eq!(
        polish("hello there", &m.url, MODEL, SHORT),
        Err(PolishError::Status(404))
    );
}

#[test]
fn garbage_json_falls_back() {
    let m = mock(200, "<html>nope</html>", Duration::ZERO);
    assert_eq!(
        polish("hello there", &m.url, MODEL, SHORT),
        Err(PolishError::BadResponse)
    );
    let m = mock(200, r#"{"choices": []}"#, Duration::ZERO);
    assert_eq!(
        polish("hello there", &m.url, MODEL, SHORT),
        Err(PolishError::BadResponse)
    );
}

#[test]
fn slow_server_times_out_within_the_budget() {
    let m = mock(200, r#"{"choices":[]}"#, Duration::from_secs(5));
    let start = Instant::now();
    let result = polish("hello there", &m.url, MODEL, SHORT);
    let elapsed = start.elapsed();
    assert!(
        matches!(result, Err(PolishError::Http(_))),
        "got {result:?}"
    );
    assert!(
        elapsed < SHORT + Duration::from_millis(700),
        "took {elapsed:?} with a {SHORT:?} timeout"
    );
}

#[test]
fn nothing_listening_falls_back() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    assert!(matches!(
        polish("hello there", &url, MODEL, SHORT),
        Err(PolishError::Http(_))
    ));
}

#[test]
fn non_loopback_urls_are_refused_without_a_request() {
    for url in [
        "https://api.openai.com/v1/chat/completions",
        "http://192.168.1.20:11434/v1/chat/completions",
        "http://10.0.0.94:11434/v1/chat/completions",
        "http://localhost.example.com/v1/chat/completions",
        "http://127.0.0.1.nip.io/v1/chat/completions",
        "ftp://127.0.0.1/x",
        "not a url",
        "",
    ] {
        let start = Instant::now();
        assert_eq!(
            polish("hello", url, MODEL, Duration::from_secs(30)),
            Err(PolishError::NotLocal(url.to_string())),
            "{url}"
        );
        // Refused up front: no DNS lookup, no connect attempt.
        assert!(start.elapsed() < Duration::from_millis(100), "{url}");
    }
}

#[test]
fn loopback_urls_are_accepted() {
    for url in [
        DEFAULT_POLISH_URL,
        "http://localhost:11434/v1/chat/completions",
        "http://LOCALHOST:8080/v1/chat/completions",
        "http://127.0.0.2:8080/v1/chat/completions",
        "http://[::1]:11434/v1/chat/completions",
        "https://127.0.0.1/v1/chat/completions",
    ] {
        assert!(is_loopback_url(url), "{url}");
    }
}

#[test]
fn blank_input_skips_the_request() {
    // Unreachable URL on purpose: a request would fail the test.
    assert_eq!(
        polish("   ", "http://127.0.0.1:9/x", MODEL, SHORT).as_deref(),
        Ok("   ")
    );
}

#[test]
fn chatty_replies_are_rejected() {
    let input = "what time is it";
    let chatty = "I'm sorry, but as a language model I don't have access to the current time. \
                  You can check the clock on your computer.";
    assert!(matches!(
        accept_reply(input, chatty),
        Err(PolishError::Rejected(_))
    ));
    assert!(matches!(
        accept_reply(input, "  "),
        Err(PolishError::Rejected(_))
    ));
}

#[test]
fn reply_cleanup_strips_think_blocks_and_wrapping_quotes() {
    assert_eq!(
        accept_reply("ship it", "<think>user wants it fixed</think>\nShip it.").as_deref(),
        Ok("Ship it.")
    );
    assert_eq!(
        accept_reply("ship it", "\"Ship it.\"").as_deref(),
        Ok("Ship it.")
    );
    assert_eq!(
        accept_reply("ship it", "“Ship it.”").as_deref(),
        Ok("Ship it.")
    );
    // Quotes the speaker dictated stay.
    assert_eq!(
        accept_reply("\"quote\" me", "\"Quote\" me.").as_deref(),
        Ok("\"Quote\" me.")
    );
    assert!(matches!(
        accept_reply("ship it", "<think>never closed"),
        Err(PolishError::Rejected(_))
    ));
}

#[test]
fn chatty_reply_from_the_server_falls_back() {
    let m = mock(
        200,
        leak(completion(
            "Sure! Here is a cleaned up version of your text, along with some suggestions for \
             improving it further: Hello there.",
        )),
        Duration::ZERO,
    );
    assert!(matches!(
        polish("hello there", &m.url, MODEL, SHORT),
        Err(PolishError::Rejected(_))
    ));
}
