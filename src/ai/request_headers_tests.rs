use super::*;
use crate::ai::agent::{CompletionMessage, complete, test_connection};
use crate::http::{self, HttpErrorKind, RequestOptions};
use crate::settings::AiAgentHeader;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const SECRET: &str = "synthetic-agent-header-secret";

fn header(name: &str, value: &str) -> AiAgentHeader {
    AiAgentHeader {
        enabled: true,
        name: name.into(),
        value: value.into(),
    }
}

fn settings(headers: Vec<AiAgentHeader>) -> AiAgentSettings {
    AiAgentSettings {
        base_url: "http://127.0.0.1:1".into(),
        model: "fixture-model".into(),
        headers,
        ..Default::default()
    }
}

fn assert_private(error: &HttpError) {
    for diagnostic in [
        error.to_string(),
        format!("{error:?}"),
        format!("{error:#?}"),
        error.message.clone(),
    ] {
        assert!(
            !diagnostic.contains(SECRET),
            "header value escaped in {diagnostic}"
        );
    }
}

#[test]
fn enabled_headers_preserve_duplicates_literal_values_and_sensitive_flags() {
    let headers = build(&settings(vec![
        header(" X-Workspace ", SECRET),
        header("x-workspace", " second "),
    ]))
    .unwrap();
    let values = headers.get_all("x-workspace").iter().collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].as_bytes(), SECRET.as_bytes());
    assert_eq!(values[1].as_bytes(), b" second ");
    assert!(values.iter().all(|value| value.is_sensitive()));
    assert!(!format!("{headers:?}").contains(SECRET));
}

#[test]
fn custom_authorization_overrides_bearer_and_disabled_rows_are_ignored() {
    let mut config = settings(vec![header("Authorization", "Basic explicit")]);
    config.api_key = SECRET.into();
    config.headers.push(AiAgentHeader {
        enabled: false,
        name: "invalid\r\nname".into(),
        value: format!("{SECRET}\r\ninvalid"),
    });
    let headers = build(&config).unwrap();
    assert_eq!(headers[AUTHORIZATION], "Basic explicit");
    assert_eq!(headers.get_all(AUTHORIZATION).iter().count(), 1);
    config.headers[0].enabled = false;
    assert_eq!(
        build(&config).unwrap()[AUTHORIZATION],
        format!("Bearer {SECRET}")
    );
}

#[test]
fn header_only_authentication_is_explicit_and_empty_authentication_still_fails() {
    for name in ["X-Api-Key", "Cookie", "X-Workspace-Credential"] {
        let headers = build(&settings(vec![header(name, SECRET)])).unwrap();
        assert_eq!(headers[name], SECRET);
        assert!(!headers.contains_key(AUTHORIZATION));
    }
    assert!(build(&settings(Vec::new())).is_err());
    assert!(build(&settings(vec![header("X-Empty", "")])).is_err());
    // Saving an unfinished connection only validates the header table.
    assert!(validate(&settings(Vec::new())).is_ok());
}

#[test]
fn malformed_and_transport_control_headers_are_rejected_without_echoing_values() {
    for name in [
        "",
        "bad name",
        "x-agent\r\n",
        "Host",
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Keep-Alive",
        "Proxy-Connection",
        "TE",
        "Trailer",
        "Upgrade",
        "Proxy-Authorization",
    ] {
        let error = build(&settings(vec![header(name, SECRET)])).unwrap_err();
        assert_private(&error);
        assert_eq!(error.kind, HttpErrorKind::Request);
    }
    for value in [
        format!("{SECRET}\r\nX-Injection: yes"),
        format!("{SECRET}\0"),
    ] {
        assert_private(&build(&settings(vec![header("X-Secret", &value)])).unwrap_err());
    }
    let oversized = settings(vec![
        header("X-Secret", SECRET);
        crate::settings::MAX_AI_AGENT_HEADERS + 1
    ]);
    assert_private(&build(&oversized).unwrap_err());
}

async fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut chunk = [0; 2048];
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0 && bytes.len() + count <= 16 * 1024);
        bytes.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(bytes).unwrap()
}

async fn response(stream: &mut TcpStream, body: &str) {
    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
}

#[tokio::test]
async fn both_models_and_completions_apply_only_enabled_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = settings(vec![
        header("X-Workspace-Credential", SECRET),
        header("Content-Type", "application/vnd.fixture+json"),
    ]);
    config.base_url = format!("http://{}", listener.local_addr().unwrap());
    config.headers.push(AiAgentHeader {
        enabled: false,
        name: "X-Disabled".into(),
        value: "must-not-send".into(),
    });
    let server = tokio::spawn(async move {
        for (path, body) in [
            ("/models", r#"{"data":[{"id":"fixture-model"}]}"#),
            (
                "/chat/completions",
                r#"{"choices":[{"message":{"content":"ok"}}]}"#,
            ),
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await.to_ascii_lowercase();
            assert!(request.lines().next().unwrap().contains(path));
            assert!(request.contains(&format!("x-workspace-credential: {SECRET}\r\n")));
            assert!(!request.contains("x-disabled:"));
            assert!(!request.contains("authorization:"));
            assert!(request.contains("content-type: application/vnd.fixture+json\r\n"));
            response(&mut stream, body).await;
        }
    });
    assert!(test_connection(&config).await.unwrap().model_listed);
    assert_eq!(
        complete(
            &config,
            &[CompletionMessage::user("fixture")],
            "Header completion fixture"
        )
        .await
        .unwrap(),
        "ok"
    );
    server.await.unwrap();
}

#[cfg(feature = "ai-code-review")]
#[tokio::test]
async fn streaming_completions_apply_custom_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = settings(vec![header("X-Workspace-Credential", SECRET)]);
    config.base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(read_request(&mut stream).await.contains(SECRET));
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    });
    assert_eq!(
        crate::ai::agent::complete_review(
            &config,
            &[CompletionMessage::user("fixture")],
            "Streaming header fixture",
            "header-fixture"
        )
        .await
        .unwrap(),
        "ok"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn invalid_headers_fail_before_contacting_the_configured_origin() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = settings(vec![header(
        "X-Secret",
        &format!("{SECRET}\r\nInjected: yes"),
    )]);
    config.base_url = format!("http://{}", listener.local_addr().unwrap());
    assert_private(&test_connection(&config).await.unwrap_err());
    assert_private(
        &complete(
            &config,
            &[CompletionMessage::user("fixture")],
            "Invalid header fixture",
        )
        .await
        .unwrap_err(),
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

async fn redirect_source(destination: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source = format!("http://{}", listener.local_addr().unwrap());
    let destination = destination.to_string();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        assert!(request.contains(SECRET));
        stream.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    });
    source
}

async fn redirect_destination() -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = format!("http://{}", listener.local_addr().unwrap());
    let observed = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        response(&mut stream, r#"{"data":[{"id":"fixture-model"}]}"#).await;
        request
    });
    (destination, observed)
}

#[tokio::test]
async fn sensitive_custom_headers_prevent_the_unmarked_cross_origin_leak() {
    // Before/control: an arbitrary unmarked header bypasses credential-name
    // recognition and is forwarded by the anonymous redirect client.
    let (destination, observed) = redirect_destination().await;
    let source = redirect_source(&destination).await;
    http::send(
        http::get(source).header("X-Workspace-Credential", SECRET),
        RequestOptions::new("Unmarked loopback header fixture").no_retry(),
    )
    .await
    .unwrap();
    assert!(observed.await.unwrap().contains(SECRET));

    // After: the exact same custom name goes through the production Agent path.
    let (destination, mut observed) = redirect_destination().await;
    let mut config = settings(vec![header("X-Workspace-Credential", SECRET)]);
    config.base_url = redirect_source(&destination).await;
    let error = test_connection(&config).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Request);
    assert_eq!(error.message, "error following redirect");
    assert_private(&error);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut observed)
            .await
            .is_err()
    );
    observed.abort();
    let _ = observed.await;
}

#[tokio::test]
async fn same_origin_redirects_preserve_custom_header_authentication() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = settings(vec![header("X-Workspace-Credential", SECRET)]);
    config.base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for redirected in [false, true] {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(read_request(&mut stream).await.contains(SECRET));
            if redirected {
                response(&mut stream, r#"{"data":[{"id":"fixture-model"}]}"#).await;
            } else {
                stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            }
        }
    });
    assert!(test_connection(&config).await.unwrap().model_listed);
    server.await.unwrap();
}
