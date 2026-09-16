//! Repository source limits, including the Base64 JSON envelope used by providers.
use base64::Engine as _;
use serde_json::Value;

pub(super) const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_SOURCE_JSON_BYTES: usize = 12 * 1024 * 1024;

pub(super) fn decode_base64_content(payload: &Value, provider: &str) -> Result<String, String> {
    let encoded = payload
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{provider} repository file response has no content."))?;
    let max_encoded = MAX_SOURCE_BYTES.div_ceil(3) * 4;
    let mut content = String::with_capacity(encoded.len().min(max_encoded));
    for character in encoded
        .chars()
        .filter(|character| !character.is_whitespace())
    {
        if content.len() + character.len_utf8() > max_encoded {
            return Err(format!(
                "{provider} repository source exceeds {MAX_SOURCE_BYTES} byte limit"
            ));
        }
        content.push(character);
    }
    // Decode into a bounded destination; invalid or oversized data never creates
    // an unbounded decoded allocation, including padding edge cases at the limit.
    let capacity = (content.len() / 4 * 3).min(MAX_SOURCE_BYTES);
    let mut bytes = vec![0; capacity];
    let length = base64::engine::general_purpose::STANDARD
        .decode_slice(content.as_bytes(), &mut bytes)
        .map_err(|error| format!("{provider} repository file base64 decode failed or exceeds {MAX_SOURCE_BYTES} byte limit: {error}"))?;
    bytes.truncate(length);
    String::from_utf8(bytes)
        .map_err(|error| format!("{provider} repository file is not UTF-8 text: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_budget_preserves_whitespace_utf8_and_padding_at_boundary() {
        assert_eq!(
            decode_base64_content(&serde_json::json!({"content":" aGVs\nbG8= "}), "GitHub")
                .unwrap(),
            "hello"
        );
        let bytes = vec![b'a'; MAX_SOURCE_BYTES];
        let content = base64::engine::general_purpose::STANDARD.encode(&bytes);
        assert_eq!(
            decode_base64_content(&serde_json::json!({"content":content}), "Gitee")
                .unwrap()
                .len(),
            MAX_SOURCE_BYTES
        );
        let content =
            base64::engine::general_purpose::STANDARD.encode(vec![b'a'; MAX_SOURCE_BYTES + 1]);
        assert!(decode_base64_content(&serde_json::json!({"content":content}), "GitHub").is_err());
        for content in ["!bad", "/w=="] {
            assert!(
                decode_base64_content(&serde_json::json!({"content":content}), "Gitee").is_err()
            );
        }
        assert_eq!(
            decode_base64_content(&serde_json::json!({"content":""}), "GitHub").unwrap(),
            ""
        );
    }
}
