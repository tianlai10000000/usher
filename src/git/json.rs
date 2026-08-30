//! Shared decoding helpers for provider JSON and pagination metadata.

use reqwest::header::HeaderMap;
use serde_json::Value;

pub(super) fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u32>().ok())
}

pub(super) fn value_to_id(value: Option<&Value>, provider: &str) -> Result<String, String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(Value::Number(value)) => Ok(value.to_string()),
        _ => Err(format!("{provider} project details did not include an id.")),
    }
}

pub(super) fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .map(|value| value.to_string())
        })
}

pub(super) fn value_u32(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| {
            value
                .get(key)
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u32>().ok())
        })
}

pub(super) fn nested_string(value: &Value, parent: &str, key: &str) -> Option<String> {
    value.get(parent)?.get(key)?.as_str().map(str::to_string)
}

pub(super) fn fallback_total(page: u32, per_page: u32, item_count: usize) -> u32 {
    page.saturating_sub(1)
        .saturating_mul(per_page)
        .saturating_add(u32::try_from(item_count).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderValue};
    use serde_json::json;

    use super::{fallback_total, header_u32, nested_string, string_field, value_to_id, value_u32};

    #[test]
    fn numeric_helpers_accept_provider_number_encodings() {
        let value = json!({
            "number": 42,
            "numeric_string": "43",
            "negative": -1,
            "too_large": u64::from(u32::MAX) + 1,
        });

        assert_eq!(value_u32(&value, "number"), Some(42));
        assert_eq!(value_u32(&value, "numeric_string"), Some(43));
        assert_eq!(value_u32(&value, "negative"), None);
        assert_eq!(value_u32(&value, "too_large"), None);
        assert_eq!(string_field(&value, "number"), Some("42".to_string()));
    }

    #[test]
    fn shared_fields_and_ids_reject_missing_or_empty_values() {
        let value = json!({
            "id": 7,
            "empty": "  ",
            "user": { "login": "octocat" },
        });

        assert_eq!(value_to_id(value.get("id"), "Git"), Ok("7".to_string()));
        assert!(value_to_id(value.get("empty"), "Git").is_err());
        assert!(value_to_id(value.get("missing"), "Git").is_err());
        assert_eq!(
            nested_string(&value, "user", "login"),
            Some("octocat".to_string())
        );
    }

    #[test]
    fn pagination_helpers_parse_headers_and_saturate_totals() {
        let mut headers = HeaderMap::new();
        headers.insert("x-total", HeaderValue::from_static("123"));

        assert_eq!(header_u32(&headers, "x-total"), Some(123));
        assert_eq!(header_u32(&headers, "missing"), None);
        assert_eq!(fallback_total(3, 20, 4), 44);
        assert_eq!(fallback_total(u32::MAX, u32::MAX, usize::MAX), u32::MAX);
    }
}
