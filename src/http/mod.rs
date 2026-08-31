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

use chrono::{DateTime, Utc};
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
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(10);
const DEFAULT_READ_ATTEMPTS: u32 = 3;
const RETRY_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
/// Warn once a provider reports fewer than this many rate-limit tokens left.
const RATE_LIMIT_WARN_THRESHOLD: i64 = 3;

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

    fn operation_timeout(operation: &str, method: Method, url: Url, timeout: Duration) -> Self {
        Self {
            kind: HttpErrorKind::Timeout,
            operation: operation.to_string(),
            method: Some(method),
            url: Some(url),
            status: None,
            message: format!("operation exceeded its {} ms budget", timeout.as_millis()),
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
type ChunkConsumerResult = Result<(), Box<HttpError>>;

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

/// Retry policy for transient failures: HTTP 429, 5xx, and connection errors.
///
/// By default only idempotent methods (GET/HEAD) are retried; a mutating
/// request must opt in through [`RetryPolicy::including_non_idempotent`].
/// Requests whose body cannot be cloned (streaming bodies) are never retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetryPolicy {
    max_attempts: u32,
    base_delay: Duration,
    max_delay: Duration,
    retry_non_idempotent: bool,
}

impl RetryPolicy {
    /// Retry transient failures up to `max_attempts` total attempts (the
    /// first request counts as attempt one).
    pub(crate) fn transient(max_attempts: u32) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            base_delay: RETRY_BASE_DELAY,
            max_delay: RETRY_MAX_DELAY,
            retry_non_idempotent: false,
        }
    }

    /// Also retry non-idempotent methods (POST/PUT/...). The caller must
    /// guarantee the request is safe to repeat.
    #[allow(dead_code)]
    pub(crate) fn including_non_idempotent(mut self) -> Self {
        self.retry_non_idempotent = true;
        self
    }

    fn applies_to(&self, method: &Method) -> bool {
        self.retry_non_idempotent || matches!(*method, Method::GET | Method::HEAD)
    }

    /// Delay before the retry that follows failed attempt `attempt`
    /// (1-based): exponential backoff with jitter, where a server-provided
    /// `Retry-After` takes precedence. Every path is capped by `max_delay`.
    /// `jitter` must be in `[0, 1)` and scales up to half an extra step.
    fn delay_before_retry(
        &self,
        attempt: u32,
        retry_after: Option<Duration>,
        jitter: f64,
    ) -> Duration {
        if let Some(retry_after) = retry_after {
            return retry_after;
        }
        let exponent = attempt.saturating_sub(1).min(16);
        let base = self
            .base_delay
            .saturating_mul(1_u32 << exponent)
            .min(self.max_delay);
        let jittered = base + base.mul_f64(jitter.clamp(0.0, 1.0) * 0.5);
        jittered.min(self.max_delay)
    }
}

fn should_retry_response(status: StatusCode, _headers: &HeaderMap) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn is_transient_transport_error(error: &HttpError) -> bool {
    matches!(error.kind, HttpErrorKind::Connect | HttpErrorKind::Timeout)
}

fn retry_after_hint(headers: &HeaderMap) -> Option<Duration> {
    retry_after_hint_at(headers, Utc::now())
}

fn retry_after_hint_at(headers: &HeaderMap, now: DateTime<Utc>) -> Option<Duration> {
    if let Some(value) = diagnostic_header_value(headers, "retry-after") {
        let value = value.trim();
        if let Ok(seconds) = value.parse::<u64>() {
            return Some(Duration::from_secs(seconds));
        }
        if let Ok(deadline) = DateTime::parse_from_rfc2822(value) {
            return deadline
                .with_timezone(&Utc)
                .signed_duration_since(now)
                .to_std()
                .ok();
        }
    }
    // GitHub commonly returns a Unix reset instant instead of Retry-After.
    diagnostic_header_value(headers, "x-ratelimit-reset")?
        .trim()
        .parse::<i64>()
        .ok()
        .and_then(|deadline| u64::try_from(deadline.saturating_sub(now.timestamp())).ok())
        .map(Duration::from_secs)
}

fn rate_limit_remaining(headers: &HeaderMap) -> Option<i64> {
    ["x-ratelimit-remaining", "ratelimit-remaining"]
        .into_iter()
        .find_map(|name| diagnostic_header_value(headers, name))
        .and_then(|value| value.trim().parse::<i64>().ok())
}

#[derive(Debug)]
struct BodyProgressTracker {
    timeout: Duration,
    last_progress_at: Instant,
}

impl BodyProgressTracker {
    fn new(timeout: Duration, now: Instant) -> Self {
        Self {
            timeout,
            last_progress_at: now,
        }
    }

    fn remaining(&self, now: Instant) -> Result<Option<Duration>, ()> {
        if self.timeout.is_zero() {
            return Ok(None);
        }
        self.timeout
            .checked_sub(now.saturating_duration_since(self.last_progress_at))
            .filter(|remaining| !remaining.is_zero())
            .map(Some)
            .ok_or(())
    }

    fn observe(&mut self, now: Instant) {
        self.last_progress_at = now;
    }

    fn idle_elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.last_progress_at)
    }
}

/// Per-request behavior. Silent feedback is the safe default for background jobs.
#[derive(Debug, Clone)]
pub(crate) struct RequestOptions {
    pub operation: String,
    pub feedback: ErrorFeedback,
    pub reject_http_errors: bool,
    pub timeout: Duration,
    pub read_idle_timeout: Duration,
    pub body_diagnostic_context: Option<String>,
    retry: Option<RetryPolicy>,
    retry_disabled: bool,
}

impl RequestOptions {
    pub(crate) fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            feedback: ErrorFeedback::Silent,
            reject_http_errors: true,
            timeout: DEFAULT_TIMEOUT,
            read_idle_timeout: DEFAULT_READ_IDLE_TIMEOUT,
            body_diagnostic_context: None,
            retry: None,
            retry_disabled: false,
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

    /// Set the maximum interval without body progress under the selected
    /// request policy. Zero disables idle detection independently from the
    /// total request timeout.
    #[cfg_attr(not(feature = "ai-code-review"), allow(dead_code))]
    pub(crate) fn read_idle_timeout(mut self, timeout: Duration) -> Self {
        self.read_idle_timeout = timeout;
        self
    }

    /// Emit bounded body-read lifecycle records without logging body content.
    #[cfg_attr(not(feature = "ai-code-review"), allow(dead_code))]
    pub(crate) fn body_diagnostics(mut self, context: impl Into<String>) -> Self {
        self.body_diagnostic_context = Some(context.into());
        self
    }

    /// Retry transient failures (HTTP 429/5xx and connection errors) up to
    /// `max_attempts` total attempts with exponential backoff and jitter.
    /// Only GET/HEAD requests are retried under this shorthand; requests
    /// without this option keep the existing single-attempt behavior.
    pub(crate) fn retry_transient(mut self, max_attempts: u32) -> Self {
        self.retry = Some(RetryPolicy::transient(max_attempts));
        self.retry_disabled = false;
        self
    }

    /// Disable the bounded GET/HEAD retry default when the caller owns retry timing.
    #[allow(dead_code)]
    pub(crate) fn no_retry(mut self) -> Self {
        self.retry = None;
        self.retry_disabled = true;
        self
    }

    /// Attach a fully customized retry policy.
    #[allow(dead_code)]
    pub(crate) fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = Some(policy);
        self.retry_disabled = false;
        self
    }
}

fn retry_policy_for(method: &Method, options: &RequestOptions) -> Option<RetryPolicy> {
    if options.retry_disabled {
        return None;
    }
    options
        .retry
        .or_else(|| {
            matches!(*method, Method::GET | Method::HEAD)
                .then(|| RetryPolicy::transient(DEFAULT_READ_ATTEMPTS))
        })
        .filter(|policy| policy.applies_to(method))
}

fn remaining_operation_budget(deadline: Option<Instant>) -> Option<Duration> {
    deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()))
}

fn request_attempt_timeout(
    has_later_attempt: bool,
    remaining: Option<Duration>,
) -> Option<Duration> {
    match (has_later_attempt, remaining) {
        (true, Some(remaining)) => Some(remaining.min(RETRY_ATTEMPT_TIMEOUT)),
        (_, remaining) => remaining,
    }
}

fn retry_delay_fits_budget(delay: Duration, deadline: Option<Instant>) -> bool {
    remaining_operation_budget(deadline).is_none_or(|remaining| delay < remaining)
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
    // Keep the concrete shared error type across the interceptor contract.
    #[allow(clippy::result_large_err)]
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
    result.push('…');
    result
}

// Interceptors intentionally share the public HTTP error contract.
#[allow(clippy::result_large_err)]
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

#[cfg(test)]
fn request_total_timeout(timeout: Duration) -> Option<Duration> {
    (!timeout.is_zero()).then_some(timeout)
}

fn diagnostic_header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
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

    async fn chunk_raw_with_timeout(
        &mut self,
        wait_timeout: Duration,
    ) -> HttpResult<Option<bytes::Bytes>> {
        let read = await_with_read_idle_timeout(wait_timeout, self.inner.chunk())
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

    async fn chunk_raw(&mut self) -> HttpResult<Option<bytes::Bytes>> {
        self.chunk_raw_with_timeout(self.options.read_idle_timeout)
            .await
    }

    pub(crate) async fn chunk(&mut self) -> HttpResult<Option<bytes::Bytes>> {
        self.chunk_raw()
            .await
            .map_err(|error| handle_error(error, self.options.feedback))
    }

    async fn for_each_chunk_raw(
        &mut self,
        mut consume: impl FnMut(&[u8]) -> ChunkConsumerResult,
    ) -> HttpResult<()> {
        let diagnostic_context = self.options.body_diagnostic_context.clone();
        let operation = self.options.operation.clone();
        let started = Instant::now();
        if let Some(context) = diagnostic_context.as_deref() {
            log::debug!(
                "[HTTP][Body] event=read_started operation={} context={}",
                operation,
                context
            );
        }
        let mut total_bytes = 0usize;
        let mut chunk_count = 0usize;
        let mut last_chunk_at = started;
        let mut progress = BodyProgressTracker::new(self.options.read_idle_timeout, started);
        loop {
            let wait_timeout = match progress.remaining(Instant::now()) {
                Ok(Some(timeout)) => timeout,
                Ok(None) => Duration::ZERO,
                Err(()) => {
                    let now = Instant::now();
                    if let Some(context) = diagnostic_context.as_deref() {
                        log::warn!(
                            "[HTTP][Body] event=read_idle_timeout operation={} context={} chunks={} bytes={} body_progress_idle_ms={} last_chunk_age_ms={} elapsed_ms={}",
                            operation,
                            context,
                            chunk_count,
                            total_bytes,
                            progress.idle_elapsed(now).as_millis(),
                            now.saturating_duration_since(last_chunk_at).as_millis(),
                            started.elapsed().as_millis()
                        );
                    }
                    return Err(HttpError::response_read_idle_timeout(
                        &self.options.operation,
                        self.method.clone(),
                        self.url.clone(),
                        self.options.read_idle_timeout,
                    ));
                }
            };
            let chunk = match self.chunk_raw_with_timeout(wait_timeout).await {
                Ok(chunk) => chunk,
                Err(error) => {
                    if error.kind == HttpErrorKind::Timeout
                        && let Some(context) = diagnostic_context.as_deref()
                    {
                        let now = Instant::now();
                        log::warn!(
                            "[HTTP][Body] event=read_idle_timeout operation={} context={} chunks={} bytes={} body_progress_idle_ms={} last_chunk_age_ms={} elapsed_ms={}",
                            operation,
                            context,
                            chunk_count,
                            total_bytes,
                            progress.idle_elapsed(now).as_millis(),
                            now.saturating_duration_since(last_chunk_at).as_millis(),
                            started.elapsed().as_millis()
                        );
                    }
                    return Err(error);
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            let now = Instant::now();
            chunk_count = chunk_count.saturating_add(1);
            let chunk_bytes = chunk.len();
            let whitespace_only = !chunk.iter().any(|byte| !byte.is_ascii_whitespace());
            let interarrival_ms = now.saturating_duration_since(last_chunk_at).as_millis();
            last_chunk_at = now;
            progress.observe(now);
            total_bytes = total_bytes.saturating_add(chunk_bytes);
            if let Err(error) = consume(&chunk) {
                return Err(*error);
            }
            if let Some(context) = diagnostic_context.as_deref()
                && (chunk_count <= 8 || chunk_count.is_power_of_two())
            {
                let idle_remaining_ms = progress
                    .remaining(now)
                    .ok()
                    .flatten()
                    .map(|remaining| remaining.as_millis());
                log::debug!(
                    "[HTTP][Body] event=read_progress operation={} context={} chunk={} chunk_bytes={} total_bytes={} interarrival_ms={} whitespace_only={} counts_as_progress={} idle_remaining_ms={:?} elapsed_ms={}",
                    operation,
                    context,
                    chunk_count,
                    chunk_bytes,
                    total_bytes,
                    interarrival_ms,
                    whitespace_only,
                    true,
                    idle_remaining_ms,
                    started.elapsed().as_millis()
                );
            }
        }
        if let Some(context) = diagnostic_context.as_deref() {
            log::debug!(
                "[HTTP][Body] event=read_completed operation={} context={} chunks={} bytes={} elapsed_ms={}",
                operation,
                context,
                chunk_count,
                total_bytes,
                started.elapsed().as_millis()
            );
        }
        Ok(())
    }

    /// Consume the response body incrementally while retaining shared idle
    /// timeout, diagnostics, and error-interceptor behavior.
    #[cfg_attr(not(feature = "ai-code-review"), allow(dead_code))]
    pub(crate) async fn for_each_chunk(
        mut self,
        consume: impl FnMut(&[u8]) -> ChunkConsumerResult,
    ) -> HttpResult<()> {
        let feedback = self.options.feedback;
        self.for_each_chunk_raw(consume)
            .await
            .map_err(|error| handle_error(error, feedback))
    }

    async fn bytes_raw(mut self) -> HttpResult<Vec<u8>> {
        let mut body = Vec::new();
        self.for_each_chunk_raw(|chunk| {
            body.extend_from_slice(chunk);
            Ok(())
        })
        .await?;
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

/// Passive rate-limit observability: surface (near-)exhaustion in the log so
/// bursts can be diagnosed. Global throttling is deliberately out of scope.
fn observe_rate_limit_headers(operation: &str, url: &Url, status: StatusCode, headers: &HeaderMap) {
    if is_silent_log_operation(operation) {
        return;
    }
    let remaining = rate_limit_remaining(headers);
    let nearly_exhausted = remaining.is_some_and(|value| value < RATE_LIMIT_WARN_THRESHOLD);
    if status == StatusCode::TOO_MANY_REQUESTS || nearly_exhausted {
        log::warn!(
            "[HTTP][RateLimit] operation={} url={} status={} remaining={:?} retry_after={:?}",
            operation,
            redacted_url(url),
            status.as_u16(),
            remaining,
            diagnostic_header_value(headers, "retry-after")
        );
    }
}

/// One network attempt: request interceptors, dispatch, response interceptors.
/// Errors are returned raw; the retry loop in [`send`] decides when to run
/// error interceptors and UI feedback so retried attempts stay quiet.
async fn send_attempt(
    mut request: reqwest::Request,
    options: &RequestOptions,
) -> HttpResult<HttpResponse> {
    let method = request.method().clone();
    let url = request.url().clone();
    run_request_interceptors(&method, &url, request.headers_mut(), options)?;
    let request_body_bytes = request
        .body()
        .and_then(|body| body.as_bytes())
        .map(|body| body.len());
    if let Some(context) = options.body_diagnostic_context.as_deref() {
        log::debug!(
            "[HTTP][Transfer] event=request_dispatched operation={} context={} method={} request_body_bytes={:?} content_type={:?}",
            options.operation,
            context,
            method,
            request_body_bytes,
            diagnostic_header_value(request.headers(), "content-type")
        );
    }
    let started = Instant::now();
    let response = HTTP
        .asynchronous
        .execute(request)
        .await
        .map_err(|error| HttpError::from_reqwest(error, &options.operation))?;
    if let Some(context) = options.body_diagnostic_context.as_deref() {
        log::debug!(
            "[HTTP][Transfer] event=response_headers_received operation={} context={} status={} request_body_bytes={:?} response_http_version={:?} content_type={:?} content_length={:?} transfer_encoding={:?} content_encoding={:?} elapsed_ms={}",
            options.operation,
            context,
            response.status().as_u16(),
            request_body_bytes,
            response.version(),
            diagnostic_header_value(response.headers(), "content-type"),
            diagnostic_header_value(response.headers(), "content-length"),
            diagnostic_header_value(response.headers(), "transfer-encoding"),
            diagnostic_header_value(response.headers(), "content-encoding"),
            started.elapsed().as_millis()
        );
    }
    run_response_interceptors(
        &method,
        &url,
        response.status(),
        response.headers(),
        started.elapsed(),
        options,
    );
    observe_rate_limit_headers(
        &options.operation,
        &url,
        response.status(),
        response.headers(),
    );
    Ok(HttpResponse {
        inner: response,
        method,
        url,
        options: options.clone(),
    })
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
    let operation_deadline = (!options.timeout.is_zero()).then(|| Instant::now() + options.timeout);
    let retry = retry_policy_for(request.method(), &options);
    let max_attempts = retry.map(|policy| policy.max_attempts).unwrap_or(1);
    let mut attempt = 1_u32;
    loop {
        let remaining = remaining_operation_budget(operation_deadline);
        if remaining.is_some_and(|remaining| remaining.is_zero()) {
            return Err(handle_error(
                HttpError::operation_timeout(
                    &options.operation,
                    request.method().clone(),
                    request.url().clone(),
                    options.timeout,
                ),
                options.feedback,
            ));
        }
        // A streaming request body cannot be cloned; without a clone this
        // attempt is final regardless of the configured policy.
        let retry_request = (attempt < max_attempts)
            .then(|| request.try_clone())
            .flatten();
        *request.timeout_mut() = request_attempt_timeout(retry_request.is_some(), remaining);
        match send_attempt(request, &options).await {
            Ok(response) => {
                let status = response.status();
                let exhausted_primary_quota = status == StatusCode::FORBIDDEN
                    && rate_limit_remaining(response.headers()) == Some(0);
                if (should_retry_response(status, response.headers()) || exhausted_primary_quota)
                    && let Some(policy) = retry
                    && let Some(next_request) = retry_request
                {
                    let retry_after = retry_after_hint(response.headers());
                    if exhausted_primary_quota && retry_after.is_none() {
                        let method = response.method.clone();
                        let url = response.url.clone();
                        return Err(handle_error(
                            HttpError::status(
                                &options.operation,
                                method,
                                url,
                                status,
                                String::new(),
                            ),
                            options.feedback,
                        ));
                    }
                    let delay =
                        policy.delay_before_retry(attempt, retry_after, rand::random::<f64>());
                    if !retry_delay_fits_budget(delay, operation_deadline) {
                        let method = response.method.clone();
                        let url = response.url.clone();
                        return Err(handle_error(
                            HttpError::status(
                                &options.operation,
                                method,
                                url,
                                status,
                                String::new(),
                            ),
                            options.feedback,
                        ));
                    }
                    log::warn!(
                        "[HTTP][Retry] operation={} status={} attempt={}/{} delay_ms={}",
                        options.operation,
                        status.as_u16(),
                        attempt,
                        max_attempts,
                        delay.as_millis()
                    );
                    drop(response);
                    tokio::time::sleep(delay).await;
                    request = next_request;
                    attempt += 1;
                    continue;
                }
                if options.reject_http_errors && !status.is_success() {
                    let method = response.method.clone();
                    let url = response.url.clone();
                    let body = response
                        .text_raw()
                        .await
                        .map_err(|error| handle_error(error, options.feedback))?;
                    return Err(handle_error(
                        HttpError::status(
                            &options.operation,
                            method,
                            url,
                            status,
                            truncate_body(body),
                        ),
                        options.feedback,
                    ));
                }
                return Ok(response);
            }
            Err(error) => {
                if is_transient_transport_error(&error)
                    && let Some(policy) = retry
                    && let Some(next_request) = retry_request
                {
                    let delay = policy.delay_before_retry(attempt, None, rand::random::<f64>());
                    if !retry_delay_fits_budget(delay, operation_deadline) {
                        return Err(handle_error(error, options.feedback));
                    }
                    log::warn!(
                        "[HTTP][Retry] operation={} error_kind={:?} attempt={}/{} delay_ms={}",
                        options.operation,
                        error.kind,
                        attempt,
                        max_attempts,
                        delay.as_millis()
                    );
                    tokio::time::sleep(delay).await;
                    request = next_request;
                    attempt += 1;
                    continue;
                }
                return Err(handle_error(error, options.feedback));
            }
        }
    }
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
        assert_eq!(options.body_diagnostic_context, None);
        assert_eq!(options.retry, None);
        assert!(!options.retry_disabled);
    }

    #[test]
    fn read_methods_retry_by_default_and_can_explicitly_opt_out() {
        let defaults = RequestOptions::new("Load list");
        assert_eq!(
            retry_policy_for(&Method::GET, &defaults),
            Some(RetryPolicy::transient(DEFAULT_READ_ATTEMPTS))
        );
        assert_eq!(
            retry_policy_for(&Method::HEAD, &defaults),
            Some(RetryPolicy::transient(DEFAULT_READ_ATTEMPTS))
        );
        assert_eq!(retry_policy_for(&Method::POST, &defaults), None);

        let disabled = RequestOptions::new("Long poll").no_retry();
        assert_eq!(retry_policy_for(&Method::GET, &disabled), None);
    }

    #[test]
    fn retry_policy_defaults_to_idempotent_methods_only() {
        let policy = RetryPolicy::transient(3);
        assert!(policy.applies_to(&Method::GET));
        assert!(policy.applies_to(&Method::HEAD));
        assert!(!policy.applies_to(&Method::POST));
        assert!(!policy.applies_to(&Method::PUT));
        assert!(!policy.applies_to(&Method::DELETE));

        let opted_in = RetryPolicy::transient(3).including_non_idempotent();
        assert!(opted_in.applies_to(&Method::POST));
    }

    #[test]
    fn retry_transient_normalizes_attempt_counts() {
        assert_eq!(RetryPolicy::transient(0).max_attempts, 1);
        assert_eq!(RetryPolicy::transient(3).max_attempts, 3);
        let options = RequestOptions::new("Load list").retry_transient(3);
        assert_eq!(options.retry, Some(RetryPolicy::transient(3)));
    }

    #[test]
    fn backoff_grows_exponentially_and_is_capped() {
        let policy = RetryPolicy::transient(5);
        assert_eq!(
            policy.delay_before_retry(1, None, 0.0),
            Duration::from_millis(500)
        );
        assert_eq!(
            policy.delay_before_retry(2, None, 0.0),
            Duration::from_millis(1_000)
        );
        assert_eq!(
            policy.delay_before_retry(3, None, 0.0),
            Duration::from_millis(2_000)
        );
        // Deterministic cap regardless of the attempt counter.
        assert_eq!(policy.delay_before_retry(30, None, 0.0), RETRY_MAX_DELAY);
    }

    #[test]
    fn backoff_jitter_adds_at_most_half_a_step_within_the_cap() {
        let policy = RetryPolicy::transient(5);
        assert_eq!(
            policy.delay_before_retry(1, None, 1.0),
            Duration::from_millis(750)
        );
        // Out-of-range jitter inputs are clamped instead of panicking.
        assert_eq!(
            policy.delay_before_retry(1, None, -3.0),
            Duration::from_millis(500)
        );
        assert!(policy.delay_before_retry(30, None, 1.0) <= RETRY_MAX_DELAY);
    }

    #[test]
    fn retry_after_header_overrides_backoff_without_shortening_server_delay() {
        let policy = RetryPolicy::transient(3);
        assert_eq!(
            policy.delay_before_retry(1, Some(Duration::from_secs(2)), 1.0),
            Duration::from_secs(2)
        );
        assert_eq!(
            policy.delay_before_retry(1, Some(Duration::from_secs(600)), 0.0),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn retry_after_hint_parses_seconds_dates_and_provider_reset_instants() {
        let now = DateTime::parse_from_rfc3339("2026-10-21T07:28:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("7"));
        assert_eq!(
            retry_after_hint_at(&headers, now),
            Some(Duration::from_secs(7))
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            "retry-after",
            HeaderValue::from_static("Wed, 21 Oct 2026 07:28:09 GMT"),
        );
        assert_eq!(
            retry_after_hint_at(&headers, now),
            Some(Duration::from_secs(9))
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-reset", HeaderValue::from_static("1792567691"));
        assert_eq!(
            retry_after_hint_at(&headers, now),
            Some(Duration::from_secs(11))
        );

        assert_eq!(retry_after_hint_at(&HeaderMap::new(), now), None);
    }

    #[test]
    fn only_rate_limit_and_server_errors_are_retryable_statuses() {
        let empty = HeaderMap::new();
        assert!(should_retry_response(StatusCode::TOO_MANY_REQUESTS, &empty));
        assert!(should_retry_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &empty
        ));
        assert!(should_retry_response(StatusCode::BAD_GATEWAY, &empty));
        assert!(!should_retry_response(StatusCode::FORBIDDEN, &empty));
        let mut exhausted = HeaderMap::new();
        exhausted.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        assert!(!should_retry_response(StatusCode::FORBIDDEN, &exhausted));
        assert!(!should_retry_response(StatusCode::OK, &empty));
        assert!(!should_retry_response(StatusCode::NOT_FOUND, &empty));
        assert!(!should_retry_response(StatusCode::UNAUTHORIZED, &empty));
    }

    #[test]
    fn retry_delays_must_fit_the_shared_operation_budget() {
        assert!(retry_delay_fits_budget(
            Duration::from_millis(10),
            Some(Instant::now() + Duration::from_secs(1))
        ));
        assert!(!retry_delay_fits_budget(
            Duration::from_secs(2),
            Some(Instant::now() + Duration::from_millis(10))
        ));
    }

    #[test]
    fn attempt_timeout_caps_only_attempts_that_can_still_retry() {
        let remaining = Duration::from_secs(90);
        assert_eq!(
            request_attempt_timeout(true, Some(remaining)),
            Some(RETRY_ATTEMPT_TIMEOUT)
        );
        assert_eq!(
            request_attempt_timeout(false, Some(remaining)),
            Some(remaining)
        );
        assert_eq!(request_attempt_timeout(true, None), None);
    }

    #[test]
    fn connection_and_timeout_failures_are_transient_transport_errors() {
        let mut error = HttpError::status(
            "Load projects",
            Method::GET,
            Url::parse("https://example.com/projects").unwrap(),
            StatusCode::BAD_GATEWAY,
            String::new(),
        );
        // Status retries are decided from the live response, not the error.
        assert!(!is_transient_transport_error(&error));
        error.kind = HttpErrorKind::Connect;
        assert!(is_transient_transport_error(&error));
        error.kind = HttpErrorKind::Timeout;
        assert!(is_transient_transport_error(&error));
        error.kind = HttpErrorKind::Decode;
        assert!(!is_transient_transport_error(&error));
    }

    #[test]
    fn request_body_diagnostics_store_only_the_explicit_context() {
        let options = RequestOptions::new("AI code review").body_diagnostics("task=review-1");
        assert_eq!(
            options.body_diagnostic_context.as_deref(),
            Some("task=review-1")
        );
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
