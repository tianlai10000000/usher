use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) async fn fixture(status: &str, chunks: Vec<Vec<u8>>, content_type: &str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
    );
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request).await;
        if stream.write_all(header.as_bytes()).await.is_err() {
            return;
        }
        for chunk in chunks {
            if stream
                .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                .await
                .is_err()
                || stream.write_all(&chunk).await.is_err()
                || stream.write_all(b"\r\n").await.is_err()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let _ = stream.write_all(b"0\r\n\r\n").await;
    });
    url
}

async fn response(limit: usize) -> HttpResponse {
    let url = fixture(
        "200 OK",
        vec![b"123".to_vec(), b"456".to_vec()],
        "text/plain",
    )
    .await;
    send(
        get(url),
        RequestOptions::new("Budget readers")
            .no_retry()
            .response_byte_limit(limit),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn budget_exact_zero_and_all_readers_enforce_actual_chunk_bytes() {
    assert_eq!(response(6).await.bytes().await.unwrap(), b"123456");
    for reader in 0..5 {
        let mut response = response(5).await;
        assert_eq!(response.content_length(), None);
        let error = match reader {
            0 => response.bytes().await.unwrap_err(),
            1 => response.text().await.unwrap_err(),
            2 => response.json::<serde_json::Value>().await.unwrap_err(),
            3 => {
                let mut delivered = 0;
                let error = response
                    .for_each_chunk(|chunk| {
                        delivered += chunk.len();
                        Ok(())
                    })
                    .await
                    .unwrap_err();
                assert!(delivered <= 5);
                error
            }
            _ => loop {
                match response.chunk().await {
                    Ok(Some(_)) => continue,
                    Ok(None) => panic!("oversized stream succeeded"),
                    Err(error) => break error,
                }
            },
        };
        assert_eq!(error.kind, HttpErrorKind::ResponseTooLarge);
        assert!(error.response_body.is_none());
        assert!(!is_transient_transport_error(&error));
    }
    assert_eq!(
        response(0).await.bytes().await.unwrap_err().kind,
        HttpErrorKind::ResponseTooLarge
    );
    let url = fixture("200 OK", vec![], "text/plain").await;
    assert!(
        send(
            get(url),
            RequestOptions::new("Empty budget").response_byte_limit(0)
        )
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap()
        .is_empty()
    );
}

struct ErrorCount(AtomicUsize);
impl HttpInterceptor for ErrorCount {
    fn on_error(&self, error: &HttpError) {
        if error.operation == "Sticky budget error" {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[tokio::test]
async fn budget_failure_is_sticky_across_readers_and_reports_once() {
    let count = Arc::new(ErrorCount(AtomicUsize::new(0)));
    HTTP.interceptors.write().unwrap().push(count.clone());
    let url = fixture("200 OK", vec![b"secret-body".to_vec()], "text/plain").await;
    let mut response = send(
        get(url),
        RequestOptions::new("Sticky budget error").response_byte_limit(2),
    )
    .await
    .unwrap();
    for _ in 0..2 {
        assert_eq!(
            response.chunk().await.unwrap_err().kind,
            HttpErrorKind::ResponseTooLarge
        );
    }
    assert_eq!(
        response.bytes().await.unwrap_err().kind,
        HttpErrorKind::ResponseTooLarge
    );
    assert_eq!(count.0.load(Ordering::Relaxed), 1);
    let target: Arc<dyn HttpInterceptor> = count;
    HTTP.interceptors
        .write()
        .unwrap()
        .retain(|entry| !Arc::ptr_eq(entry, &target));
}

#[tokio::test]
async fn budget_tightening_counts_prior_chunks_and_cannot_relax_a_limit() {
    let mut response = response(6).await;
    assert!(response.chunk().await.unwrap().is_some());
    let response = response.response_byte_limit(2).response_byte_limit(100);
    assert_eq!(
        response.bytes().await.unwrap_err().kind,
        HttpErrorKind::ResponseTooLarge
    );
}

#[tokio::test]
async fn budget_defaults_leave_streaming_unbounded_and_status_excerpt_is_bounded() {
    assert_eq!(RequestOptions::new("default").response_byte_limit, None);
    let url = fixture("200 OK", vec![vec![b'x'; 20_000]], "text/event-stream").await;
    let mut total = 0;
    send(get(url), RequestOptions::new("Streaming default"))
        .await
        .unwrap()
        .for_each_chunk(|chunk| {
            total += chunk.len();
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(total, 20_000);
    let url = fixture("400 Bad Request", vec![vec![b'x'; 20_000]], "text/plain").await;
    let error = match send(get(url), RequestOptions::new("Bounded excerpt")).await {
        Ok(_) => panic!("error status accepted"),
        Err(error) => error,
    };
    assert_eq!(error.kind, HttpErrorKind::Status);
    assert!(error.response_body.unwrap().chars().count() <= MAX_ERROR_BODY_CHARS + 1);
}

#[tokio::test]
async fn budget_does_not_disable_read_idle_timeout() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    let error = send(
        get(url),
        RequestOptions::new("Budget idle")
            .no_retry()
            .response_byte_limit(10)
            .read_idle_timeout(Duration::from_millis(30)),
    )
    .await
    .unwrap()
    .bytes()
    .await
    .unwrap_err();
    server.abort();
    let _ = server.await;
    assert_eq!(error.kind, HttpErrorKind::Timeout);
}
