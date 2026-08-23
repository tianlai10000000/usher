//! Shared HTTP client and Axios-style interception pipeline.
//!
//! Business modules should build requests with [`get`], [`post`], or [`put`]
//! and execute them through [`send`] or [`send_json`]. This keeps timeout,
//! status handling, logging, and UI feedback policy in one place while still
//! exposing reqwest's mature request builder API.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use lazy_static::lazy_static;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use slint::{ComponentHandle, Global};

use crate::{
    CustomComponentType, MainWindow, NotificationDelivery, NotificationManager,
    NotificationOptions, ToastManager, ToastOptions,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ERROR_BODY_CHARS: usize = 2_000;

lazy_static! {
    static ref HTTP: HttpClient =
        HttpClient::new().expect("failed to initialize shared HTTP client");
    static ref UI_WEAK: std::sync::Mutex<Option<slint::Weak<MainWindow>>> =
        std::sync::Mutex::new(None);
}

/// Coarse error category that callers can match without parsing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HttpErrorKind {
    Build,
    Timeout,
    Connect,
    Request,
    Status,
    Decode,
    Interceptor,
}

/// Normalized error returned by all HTTP entry points.
#[derive(Debug, Clone)]
pub(crate) struct HttpError {
    pub kind: HttpErrorKind,
    pub operation: String,
    pub method: Option<Method>,
    pub url: Option<Url>,
    pub status: Option<StatusCode>,
    pub message: String,
    pub response_body: Option<String>,
}

impl HttpError {
    fn from_reqwest(error: reqwest::Error, operation: &str) -> Self {
        let kind = if error.is_timeout() {
            HttpErrorKind::Timeout
        } else if error.is_connect() {
            HttpErrorKind::Connect
        } else if error.is_builder() {
            HttpErrorKind::Build
        } else if error.is_decode() {
            HttpErrorKind::Decode
        } else {
            HttpErrorKind::Request
        };
        Self {
            kind,
            operation: operation.to_string(),
            method: None,
            url: error.url().cloned(),
            status: error.status(),
            message: error.to_string(),
            response_body: None,
        }
    }

    fn status(operation: &str, method: Method, url: Url, status: StatusCode, body: String) -> Self {
        Self {
            kind: HttpErrorKind::Status,
            operation: operation.to_string(),
            method: Some(method),
            url: Some(url),
            status: Some(status),
            message: format!("HTTP request returned status {}", status.as_u16()),
            response_body: (!body.is_empty()).then_some(body),
        }
    }

    fn decode(error: reqwest::Error, operation: &str) -> Self {
        Self::from_reqwest(error, operation)
    }

    fn response_read_idle_timeout(
        operation: &str,
        method: Method,
        url: Url,
        timeout: Duration,
    ) -> Self {
        Self {
            kind: HttpErrorKind::Timeout,
            operation: operation.to_string(),
            method: Some(method),
            url: Some(url),
            status: None,
            message: format!(
                "response body made no progress for {} ms",
                timeout.as_millis()
            ),
            response_body: None,
        }
    }

    fn json_decode(error: serde_json::Error, operation: &str, method: Method, url: Url) -> Self {
        Self {
            kind: HttpErrorKind::Decode,
            operation: operation.to_string(),
            method: Some(method),
            url: Some(url),
            status: None,
            message: error.to_string(),
            response_body: None,
        }
    }

    /// Stable, user-facing summary suitable for toast/notification content.
    pub(crate) fn user_message(&self) -> String {
        let action = if self.operation.trim().is_empty() {
            "Request"
        } else {
            self.operation.as_str()
        };
        match (self.kind, self.status.map(|status| status.as_u16())) {
            (HttpErrorKind::Timeout, _) => format!("{action} timed out. Please try again."),
            (HttpErrorKind::Connect, _) => {
                format!("{action} could not connect. Check your network and try again.")
            }
            (HttpErrorKind::Status, Some(401 | 403)) => {
                format!("{action} was rejected. Check your credentials or permissions.")
            }
            (HttpErrorKind::Status, Some(429)) => {
                format!("{action} was rate limited. Please try again later.")
            }
            (HttpErrorKind::Status, Some(code @ 500..=599)) => {
                format!("{action} failed because the server returned {code}.")
            }
            (HttpErrorKind::Status, Some(code)) => format!("{action} failed (HTTP {code})."),
            (HttpErrorKind::Decode, _) => {
                format!("{action} returned data in an unexpected format.")
            }
            _ => format!("{action} failed. Please try again."),
        }
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (Some(method), Some(url)) = (&self.method, &self.url) {
            write!(formatter, "{method} {} ", redacted_url(url))?;
        }
        if let Some(status) = self.status {
            write!(formatter, "{}: {}", self.operation, status)?;
        } else {
            write!(formatter, "{}: {}", self.operation, self.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for HttpError {}

pub(crate) type HttpResult<T> = Result<T, HttpError>;

/// UI side effect requested for one failed request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ErrorFeedback {
    #[default]
    Silent,
    Toast,
    Notification,
    ToastAndNotification,
}

/// Per-request behavior. Silent feedback is the safe default for background jobs.
#[derive(Debug, Clone)]
pub(crate) struct RequestOptions {
    pub operation: String,
    pub feedback: ErrorFeedback,
    pub reject_http_errors: bool,
    pub timeout: Duration,
    pub read_idle_timeout: Duration,
}

impl RequestOptions {
    pub(crate) fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            feedback: ErrorFeedback::Silent,
            reject_http_errors: true,
            timeout: DEFAULT_TIMEOUT,
            read_idle_timeout: DEFAULT_READ_IDLE_TIMEOUT,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn feedback(mut self, feedback: ErrorFeedback) -> Self {
        self.feedback = feedback;
        self
    }

    /// Set the total request timeout. Zero disables the total deadline while
    /// retaining the shared connect timeout.
    pub(crate) fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the maximum interval without a response-body chunk. Zero disables
    /// idle detection independently from the total request timeout.
    pub(crate) fn read_idle_timeout(mut self, timeout: Duration) -> Self {
        self.read_idle_timeout = timeout;
        self
    }

    pub(crate) fn allow_http_errors(mut self) -> Self {
        self.reject_http_errors = false;
        self
    }
}

/// Mutable metadata exposed before a request is sent.
pub(crate) struct RequestContext<'a> {
    pub operation: &'a str,
    pub method: &'a Method,
    pub url: &'a Url,
    pub headers: &'a mut HeaderMap,
    pub timeout: Duration,
    pub read_idle_timeout: Duration,
}

/// Read-only metadata exposed after headers are received.
pub(crate) struct ResponseContext<'a> {
    pub operation: &'a str,
    pub method: &'a Method,
    pub url: &'a Url,
    pub status: StatusCode,
    pub headers: &'a HeaderMap,
    pub elapsed: Duration,
}

/// Axios-style lifecycle hook. Implementors must remain fast and non-blocking.
pub(crate) trait HttpInterceptor: Send + Sync {
    fn on_request(&self, _request: &mut RequestContext<'_>) -> HttpResult<()> {
        Ok(())
    }

    fn on_response(&self, _response: &ResponseContext<'_>) {}

    fn on_error(&self, _error: &HttpError) {}
}

struct LoggingInterceptor;

/// Connectivity probing runs every few seconds and is expected to be silent.
/// Keep its traffic out of the terminal while retaining normal HTTP logging.
fn is_silent_log_operation(operation: &str) -> bool {
    operation == "Check network connectivity"
}

impl HttpInterceptor for LoggingInterceptor {
    fn on_request(&self, request: &mut RequestContext<'_>) -> HttpResult<()> {
        if is_silent_log_operation(request.operation) {
            return Ok(());
        }
        log::debug!(
            "[HTTP] -> {} {} headers={} timeout_ms={} read_idle_timeout_ms={} operation={}",
            request.method,
            redacted_url(request.url),
            request.headers.len(),
            request.timeout.as_millis(),
            request.read_idle_timeout.as_millis(),
            request.operation
        );
        Ok(())
    }

    fn on_response(&self, response: &ResponseContext<'_>) {
        if is_silent_log_operation(response.operation) {
            return;
        }
        log::debug!(
            "[HTTP] <- {} {} status={} headers={} elapsed_ms={} operation={}",
            response.method,
            redacted_url(response.url),
            response.status.as_u16(),
            response.headers.len(),
            response.elapsed.as_millis(),
            response.operation
        );
    }

    fn on_error(&self, error: &HttpError) {
        if is_silent_log_operation(&error.operation) {
            return;
        }
        let response_body_chars = error
            .response_body
            .as_deref()
            .map(str::chars)
            .map(Iterator::count)
            .unwrap_or(0);
        log::error!("[HTTP] {error} response_body_chars={response_body_chars}");
    }
}

fn redacted_url(url: &Url) -> String {
    let mut result = url.clone();
    let pairs = url
        .query_pairs()
        .map(|(key, value)| {
            let sensitive = matches!(
                key.to_ascii_lowercase().as_str(),
                "access_token" | "token" | "api_key" | "apikey" | "key"
            );
            (
                key.into_owned(),
                if sensitive {
                    "***".to_string()
                } else {
                    value.into_owned()
                },
            )
        })
        .collect::<Vec<_>>();
    if !pairs.is_empty() {
        result.query_pairs_mut().clear().extend_pairs(pairs);
    }
    result.to_string()
}

struct HttpClient {
    asynchronous: reqwest::Client,
    interceptors: RwLock<Vec<Arc<dyn HttpInterceptor>>>,
}

impl HttpClient {
    fn new() -> Result<Self, reqwest::Error> {
        let user_agent = format!("usher/{}", env!("CARGO_PKG_VERSION"));
        let asynchronous = reqwest::Client::builder()
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .user_agent(user_agent)
            .build()?;
        Ok(Self {
            asynchronous,
            interceptors: RwLock::new(vec![Arc::new(LoggingInterceptor)]),
        })
    }

    fn interceptors(&self) -> Vec<Arc<dyn HttpInterceptor>> {
        self.interceptors
            .read()
            .map(|items| items.clone())
            .unwrap_or_default()
    }
}

/// Register an application-wide interceptor. Registration order is execution order.
#[allow(dead_code)]
pub(crate) fn add_interceptor(interceptor: Arc<dyn HttpInterceptor>) {
    if let Ok(mut interceptors) = HTTP.interceptors.write() {
        interceptors.push(interceptor);
    }
}

pub(crate) fn get(url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
    HTTP.asynchronous.get(url)
}

pub(crate) fn post(url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
    HTTP.asynchronous.post(url)
}

pub(crate) fn put(url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
    HTTP.asynchronous.put(url)
}

fn truncate_body(body: String) -> String {
    if body.chars().count() <= MAX_ERROR_BODY_CHARS {
        return body;
    }
    let mut result = body.chars().take(MAX_ERROR_BODY_CHARS).collect::<String>();
    result.push_str("…");
    result
}

fn run_request_interceptors(
    method: &Method,
    url: &Url,
    headers: &mut HeaderMap,
    options: &RequestOptions,
) -> HttpResult<()> {
    if !headers.contains_key(USER_AGENT) {
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(concat!("usher/", env!("CARGO_PKG_VERSION"))),
        );
    }
    let mut context = RequestContext {
        operation: options.operation.as_str(),
        method,
        url,
        headers,
        timeout: options.timeout,
        read_idle_timeout: options.read_idle_timeout,
    };
    for interceptor in HTTP.interceptors() {
        if let Err(mut error) = interceptor.on_request(&mut context) {
            error.kind = HttpErrorKind::Interceptor;
            return Err(error);
        }
    }
    Ok(())
}

fn run_response_interceptors(
    method: &Method,
    url: &Url,
    status: StatusCode,
    headers: &HeaderMap,
    elapsed: Duration,
    options: &RequestOptions,
) {
    let context = ResponseContext {
        operation: options.operation.as_str(),
        method,
        url,
        status,
        headers,
        elapsed,
    };
    for interceptor in HTTP.interceptors() {
        interceptor.on_response(&context);
    }
}

fn handle_error(error: HttpError, feedback: ErrorFeedback) -> HttpError {
    for interceptor in HTTP.interceptors() {
        interceptor.on_error(&error);
    }
    present_error(&error, feedback);
    error
}

fn request_total_timeout(timeout: Duration) -> Option<Duration> {
    (!timeout.is_zero()).then_some(timeout)
}

async fn await_with_read_idle_timeout<T>(
    timeout: Duration,
    future: impl Future<Output = T>,
) -> Result<T, ()> {
    if timeout.is_zero() {
        Ok(future.await)
    } else {
        tokio::time::timeout(timeout, future).await.map_err(|_| ())
    }
}

/// Response wrapper that keeps body reads inside the shared timeout and error
/// interception pipeline.
pub(crate) struct HttpResponse {
    inner: reqwest::Response,
    method: Method,
    url: Url,
    options: RequestOptions,
}

impl HttpResponse {
    pub(crate) fn status(&self) -> StatusCode {
        self.inner.status()
    }

    pub(crate) fn headers(&self) -> &HeaderMap {
        self.inner.headers()
    }

    pub(crate) fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    async fn chunk_raw(&mut self) -> HttpResult<Option<bytes::Bytes>> {
        let read = await_with_read_idle_timeout(self.options.read_idle_timeout, self.inner.chunk())
            .await
            .map_err(|()| {
                HttpError::response_read_idle_timeout(
                    &self.options.operation,
                    self.method.clone(),
                    self.url.clone(),
                    self.options.read_idle_timeout,
                )
            })?;
        read.map_err(|error| HttpError::decode(error, &self.options.operation))
    }

    pub(crate) async fn chunk(&mut self) -> HttpResult<Option<bytes::Bytes>> {
        self.chunk_raw()
            .await
            .map_err(|error| handle_error(error, self.options.feedback))
    }

    async fn bytes_raw(mut self) -> HttpResult<Vec<u8>> {
        let mut body = Vec::new();
        while let Some(chunk) = self.chunk_raw().await? {
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    pub(crate) async fn bytes(self) -> HttpResult<Vec<u8>> {
        let feedback = self.options.feedback;
        self.bytes_raw()
            .await
            .map_err(|error| handle_error(error, feedback))
    }

    async fn text_raw(self) -> HttpResult<String> {
        self.bytes_raw()
            .await
            .map(|body| String::from_utf8_lossy(&body).into_owned())
    }

    pub(crate) async fn text(self) -> HttpResult<String> {
        let feedback = self.options.feedback;
        self.text_raw()
            .await
            .map_err(|error| handle_error(error, feedback))
    }

    pub(crate) async fn json<T: DeserializeOwned>(self) -> HttpResult<T> {
        let feedback = self.options.feedback;
        let operation = self.options.operation.clone();
        let method = self.method.clone();
        let url = self.url.clone();
        let body = self
            .bytes_raw()
            .await
            .map_err(|error| handle_error(error, feedback))?;
        serde_json::from_slice(&body).map_err(|error| {
            handle_error(
                HttpError::json_decode(error, &operation, method, url),
                feedback,
            )
        })
    }
}

pub(crate) async fn send(
    builder: reqwest::RequestBuilder,
    options: RequestOptions,
) -> HttpResult<HttpResponse> {
    let mut request = builder.build().map_err(|error| {
        handle_error(
            HttpError::from_reqwest(error, &options.operation),
            options.feedback,
        )
    })?;
    *request.timeout_mut() = request_total_timeout(options.timeout);
    let method = request.method().clone();
    let url = request.url().clone();
    run_request_interceptors(&method, &url, request.headers_mut(), &options)
        .map_err(|error| handle_error(error, options.feedback))?;
    let started = Instant::now();
    let response = HTTP.asynchronous.execute(request).await.map_err(|error| {
        handle_error(
            HttpError::from_reqwest(error, &options.operation),
            options.feedback,
        )
    })?;
    run_response_interceptors(
        &method,
        &url,
        response.status(),
        response.headers(),
        started.elapsed(),
        &options,
    );
    let response = HttpResponse {
        inner: response,
        method: method.clone(),
        url: url.clone(),
        options: options.clone(),
    };
    if options.reject_http_errors && !response.status().is_success() {
        let status = response.status();
        let body = response
            .text_raw()
            .await
            .map_err(|error| handle_error(error, options.feedback))?;
        return Err(handle_error(
            HttpError::status(&options.operation, method, url, status, truncate_body(body)),
            options.feedback,
        ));
    }
    Ok(response)
}

pub(crate) async fn send_json<T: DeserializeOwned>(
    builder: reqwest::RequestBuilder,
    options: RequestOptions,
) -> HttpResult<T> {
    let response = send(builder, options).await?;
    response.json::<T>().await
}

/// Bind centralized error feedback to the Slint managers.
pub(crate) fn setup_http(ui: &MainWindow) {
    if let Ok(mut target) = UI_WEAK.lock() {
        *target = Some(ui.as_weak());
    }
    lazy_static::initialize(&HTTP);
}

fn present_error(error: &HttpError, feedback: ErrorFeedback) {
    if feedback == ErrorFeedback::Silent {
        return;
    }
    let ui_weak = UI_WEAK.lock().ok().and_then(|target| target.clone());
    let Some(ui_weak) = ui_weak else {
        return;
    };
    let message = error.user_message();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if matches!(
            feedback,
            ErrorFeedback::Toast | ErrorFeedback::ToastAndNotification
        ) {
            ToastManager::get(&ui).invoke_error(ToastOptions {
                message: message.clone().into(),
                duration: 3_500,
                position: "top".into(),
                icon: "".into(),
                overlay: false,
                close_on_click: true,
                loading_type: "".into(),
                width: "auto".into(),
            });
        }
        if matches!(
            feedback,
            ErrorFeedback::Notification | ErrorFeedback::ToastAndNotification
        ) {
            NotificationManager::get(&ui).invoke_error(NotificationOptions {
                title: "Request failed".into(),
                message: message.into(),
                duration: 5_000,
                icon: "".into(),
                component: CustomComponentType::None,
                actions: Default::default(),
                tag: "http-error".into(),
                source: "HTTP".into(),
                category: "general".into(),
                native_delivery: NotificationDelivery::WhenUnfocused,
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_errors_have_stable_user_messages() {
        let error = HttpError::status(
            "Load projects",
            Method::GET,
            Url::parse("https://example.com/projects").unwrap(),
            StatusCode::TOO_MANY_REQUESTS,
            String::new(),
        );
        assert_eq!(
            error.user_message(),
            "Load projects was rate limited. Please try again later."
        );
    }

    #[test]
    fn request_options_are_silent_and_strict_by_default() {
        let options = RequestOptions::new("Test");
        assert_eq!(options.feedback, ErrorFeedback::Silent);
        assert!(options.reject_http_errors);
        assert_eq!(options.timeout, DEFAULT_TIMEOUT);
        assert_eq!(options.read_idle_timeout, DEFAULT_READ_IDLE_TIMEOUT);
    }

    #[test]
    fn request_timeout_supports_custom_and_unbounded_values() {
        let custom = RequestOptions::new("Probe").timeout(Duration::from_secs(3));
        assert_eq!(
            request_total_timeout(custom.timeout),
            Some(Duration::from_secs(3))
        );

        let unbounded = RequestOptions::new("Long operation").timeout(Duration::ZERO);
        assert_eq!(request_total_timeout(unbounded.timeout), None);
    }

    #[tokio::test]
    async fn read_idle_timeout_is_resettable_and_can_be_disabled() {
        let timed_out =
            await_with_read_idle_timeout(Duration::from_millis(1), std::future::pending::<()>())
                .await;
        assert!(timed_out.is_err());

        let completed = await_with_read_idle_timeout(Duration::ZERO, async { 7_u8 }).await;
        assert_eq!(completed, Ok(7));
    }

    #[test]
    fn sensitive_query_values_are_redacted() {
        let url = Url::parse("https://example.com/api?page=1&access_token=secret").unwrap();
        let redacted = redacted_url(&url);
        assert!(redacted.contains("page=1"));
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn connectivity_probe_logs_are_suppressed() {
        assert!(is_silent_log_operation("Check network connectivity"));
        assert!(!is_silent_log_operation("Load projects"));
    }

    #[test]
    fn json_decode_path_preserves_existing_reqwest_classification() {
        let error = get("://invalid")
            .build()
            .expect_err("invalid URL should fail request construction");
        assert!(error.is_builder());
        assert_eq!(
            HttpError::decode(error, "Decode completion").kind,
            HttpErrorKind::Build
        );
    }
}
