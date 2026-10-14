use super::*;

const PRIVATE_URL: &str = "https://private-user:private-password@example.com:8443/api/projects?access_token=private-token&signature=private-signature&arbitrary=private-value#private-fragment";

fn assert_no_private_data(diagnostic: &str) {
    for value in [
        "private-user",
        "private-password",
        "private-token",
        "private-signature",
        "private-value",
        "private-fragment",
        "private-response",
    ] {
        assert!(
            !diagnostic.contains(value),
            "diagnostic disclosed {value}: {diagnostic}"
        );
    }
}

#[test]
fn url_diagnostics_remove_credentials_all_queries_and_fragments() {
    let url = Url::parse(PRIVATE_URL).unwrap();
    let diagnostic = redacted_url(&url);
    assert_no_private_data(&diagnostic);
    assert_eq!(diagnostic, "https://example.com:8443/api/projects");
    // Sanitizing a diagnostic must not change the request actually sent.
    assert_eq!(url.as_str(), PRIVATE_URL);
}

#[test]
fn ordinary_endpoint_diagnostics_keep_scheme_host_port_and_path() {
    for endpoint in [
        "https://example.com/api/projects",
        "http://localhost:8080/api/projects/42",
        "https://[::1]:8443/api/projects",
    ] {
        assert_eq!(redacted_url(&Url::parse(endpoint).unwrap()), endpoint);
    }
}

#[test]
fn unsupported_url_diagnostics_fail_closed() {
    for endpoint in [
        "data:text/plain,private-response",
        "file:///private-response",
        "mailto:private-user@example.com",
    ] {
        let diagnostic = redacted_url(&Url::parse(endpoint).unwrap());
        assert_no_private_data(&diagnostic);
        assert_eq!(diagnostic, "<redacted-url>");
    }
}

#[test]
fn status_error_display_preserves_context_without_private_data() {
    let error = HttpError::status(
        "Load projects",
        Method::GET,
        Url::parse(PRIVATE_URL).unwrap(),
        StatusCode::UNAUTHORIZED,
        "private-response".into(),
    );
    let diagnostic = error.to_string();
    assert_no_private_data(&diagnostic);
    assert!(diagnostic.contains("GET https://example.com:8443/api/projects"));
    assert!(diagnostic.contains("Load projects: 401"));
    assert_eq!(error.response_body.as_deref(), Some("private-response"));
}

#[test]
fn error_debug_preserves_metadata_without_dumping_url_or_response() {
    let mut error = HttpError::status(
        "Load projects",
        Method::GET,
        Url::parse(PRIVATE_URL).unwrap(),
        StatusCode::UNAUTHORIZED,
        "private-response".into(),
    );
    // Interceptors can supply their own error prose, which is not a safe
    // source for the general-purpose Debug representation either.
    error.message = "private-response".into();
    for diagnostic in [format!("{error:?}"), format!("{error:#?}")] {
        assert_no_private_data(&diagnostic);
        assert!(diagnostic.contains("Load projects"));
        assert!(diagnostic.contains("Status"));
        assert!(diagnostic.contains("401"));
    }
}

#[test]
fn reqwest_error_message_does_not_reintroduce_the_original_url() {
    let source = get("://invalid")
        .build()
        .expect_err("invalid URL should fail request construction")
        .with_url(Url::parse(PRIVATE_URL).unwrap());
    assert!(source.to_string().contains("private-token"));
    let error = HttpError::from_reqwest(source, "Build request");
    assert_eq!(error.kind, HttpErrorKind::Build);
    assert_eq!(error.url.as_ref().map(Url::as_str), Some(PRIVATE_URL));
    assert_eq!(error.message, "builder error");
    assert_no_private_data(&error.message);
    assert_no_private_data(&error.to_string());
    assert_no_private_data(&format!("{error:?}"));
}

#[test]
fn json_decode_diagnostics_keep_location_without_echoing_payload() {
    let source = serde_json::from_str::<u64>("\"private-response\"").unwrap_err();
    assert!(source.to_string().contains("private-response"));
    let line = source.line();
    let column = source.column();
    let error = HttpError::json_decode(
        source,
        "Decode projects",
        Method::GET,
        Url::parse("https://example.com/api/projects").unwrap(),
    );
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert!(error.message.contains("Data"));
    assert!(
        error
            .message
            .contains(&format!("line {line} column {column}"))
    );
    assert_no_private_data(&error.message);
    assert_no_private_data(&error.to_string());
    assert_no_private_data(&format!("{error:?}"));
}
