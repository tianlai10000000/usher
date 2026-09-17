use super::log_layout::LogLayout;
use std::hash::{DefaultHasher, Hasher};

/// Captured on both sides before/after the representation change. The fixture
/// hashes line boundaries as well as bytes, so equal concatenation is insufficient.
#[test]
fn retention_layout_fingerprint() {
    let mut layout = LogLayout::new();
    let cases = [
        String::new(),
        "\n\r\n\n".into(),
        "Warning: \u{6d88}\u{606f}\n\nnext\r\n".into(),
        "[stdout] x".repeat(9000),
        "\u{0001}".repeat(100_000),
        "\u{6d88}\u{606f} /src/very-long-path/ {\"key\":42} ".repeat(3000),
    ];
    let mut results = Vec::new();
    for (index, text) in cases.iter().enumerate() {
        for width in [1.0, 160.0, 640.0, 800.0] {
            let lines = layout.wrap(text, width);
            let mut hash = DefaultHasher::new();
            for line in &lines {
                hash.write_usize(line.len());
                hash.write(line.as_bytes());
            }
            assert_eq!(lines.concat(), text.replace(['\r', '\n'], ""));
            results.push(serde_json::json!({
                "case": index, "width": width, "lines": lines.len(), "hash": hash.finish()
            }));
        }
    }
    if let Ok(path) = std::env::var("USHER_JOB_LAYOUT_CAPTURE") {
        std::fs::write(path, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }
}
