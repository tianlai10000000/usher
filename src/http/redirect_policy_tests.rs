use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0 && bytes.len() + count <= 8192);
        bytes.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(bytes).unwrap()
}

async fn redirect_fixture(destination: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    url
}

async fn destination_fixture() -> (String, tokio::task::JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let observed = tokio::spawn(async move {
        // Remain listening throughout send(). An early fixture deadline could
        // make a leaking client fail to connect and falsely pass the test.
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
        true
    });
    (url, observed)
}

#[tokio::test]
async fn credential_redirect_never_contacts_a_different_origin() {
    for credential in ["PRIVATE-TOKEN", "Authorization", "query"] {
        let (destination, mut observed) = destination_fixture().await;
        let source = redirect_fixture(destination).await;
        let request = if credential == "query" {
            get(format!("{source}?access_token=fixture-token"))
        } else {
            get(source).header(credential, "fixture-token")
        };
        let result = send(
            request,
            RequestOptions::new("Credential redirect fixture")
                .timeout(Duration::from_secs(2))
                .no_retry(),
        )
        .await;
        let destination_contacted =
            match tokio::time::timeout(Duration::from_millis(100), &mut observed).await {
                Ok(result) => result.unwrap(),
                Err(_) => {
                    observed.abort();
                    let _ = observed.await;
                    false
                }
            };
        assert!(!destination_contacted, "credential source: {credential}");
        let error = result
            .err()
            .expect("cross-origin redirects fail explicitly");
        assert_eq!(error.kind, HttpErrorKind::Request);
        // A connection failure or request deadline must not masquerade as a
        // redirect-policy rejection. This is reqwest's URL-free error kind.
        assert_eq!(error.message, "error following redirect");
    }
}

#[tokio::test]
async fn anonymous_download_preserves_cross_origin_redirects() {
    let (destination, observed) = destination_fixture().await;
    let source = redirect_fixture(destination).await;
    let response = send(
        get(source),
        RequestOptions::new("Anonymous redirect fixture")
            .timeout(Duration::from_secs(2))
            .no_retry(),
    )
    .await
    .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    assert!(observed.await.unwrap());
}

#[tokio::test]
async fn credential_redirect_preserves_same_origin_authentication() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for response in [
            "HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await.to_ascii_lowercase();
            assert!(request.contains("private-token: fixture-token\r\n"));
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let response = send(
        get(url).header("PRIVATE-TOKEN", "fixture-token"),
        RequestOptions::new("Same-origin credential redirect fixture")
            .timeout(Duration::from_secs(2))
            .no_retry(),
    )
    .await
    .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    server.await.unwrap();
}

#[tokio::test]
async fn credential_redirect_loops_retain_reqwest_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for _ in 0..11 {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
    });
    let result = send(
        get(url).header("PRIVATE-TOKEN", "fixture-token"),
        RequestOptions::new("Credential redirect loop fixture")
            .timeout(Duration::from_secs(2))
            .no_retry(),
    )
    .await;
    assert!(matches!(
        result,
        Err(HttpError {
            kind: HttpErrorKind::Request,
            ..
        })
    ));
    server.await.unwrap();
}

#[test]
fn credential_client_selection_covers_headers_and_known_query_credentials() {
    for name in ["PRIVATE-TOKEN", "Authorization", "Cookie", "X-API-Key"] {
        let request = get("https://example.invalid")
            .header(name, "fixture")
            .build()
            .unwrap();
        assert!(request_has_credentials(&request));
    }
    let mut sensitive = HeaderValue::from_static("fixture");
    sensitive.set_sensitive(true);
    let request = get("https://example.invalid")
        .header("custom-credential", sensitive)
        .build()
        .unwrap();
    assert!(request_has_credentials(&request));
    for key in ["access_token", "TOKEN", "api_key", "apikey", "key"] {
        let request = get(format!("https://example.invalid?{key}=fixture"))
            .build()
            .unwrap();
        assert!(request_has_credentials(&request));
    }
    let request = get("https://example.invalid?size=400").build().unwrap();
    assert!(!request_has_credentials(&request));
}
