//! Development-only Preview fixtures that exercise the production facade.

use std::path::PathBuf;

use super::{ArtifactBody, ArtifactHint, ArtifactPreviewHandle, ArtifactSource};
use crate::MainWindow;

const MARKDOWN_FIXTURE: &str = r#"# Artifact Preview

This document exercises **strong text**, *emphasis*, `inline code`, and an
[HTTP link](https://github.com).

## Lists and tasks

- First list item
- Nested-looking content with 中文、日本語、한국어
- [x] Completed task
- [ ] Pending task

> Preview providers prepare content independently from docked and detached hosts.

## Table

| Provider | Enabled | Capability |
| --- | --- | --- |
| Markdown | Yes | Copy and links |
| Image | Yes | Zoom |
| Plain text | Yes | Copy and search |

## Code

```rust
let source = ArtifactSource::inline_markdown(
    "fixture:preview:markdown",
    "preview-document.md",
    markdown,
);
preview.open_source(source);
```

---

Resize the docked divider or detached window to verify stable wrapping and scrolling.
"#;

const PLAIN_TEXT_FIXTURE: &str = "Usher artifact preview fixture\n\
================================\n\
2026-08-12T10:15:01+08:00 INFO  Preview worker started\n\
2026-08-12T10:15:01+08:00 INFO  Provider selected: plain-text\n\
2026-08-12T10:15:01+08:00 WARN  This long line verifies wrapping without rebuilding the source model while the docked divider or detached native window is resized.\n\
2026-08-12T10:15:02+08:00 INFO  Unicode check: 中文、日本語、한국어\n\
2026-08-12T10:15:02+08:00 INFO  Copy action should include this complete document.\n";

const JSON_FIXTURE: &str = r#"{
  "name": "Usher Preview",
  "provider": "plain-text",
  "hostModes": ["docked", "detached"],
  "capabilities": {
    "copy": true,
    "search": true,
    "zoom": false
  },
  "status": "fixture"
}
"#;

pub(super) fn setup(ui: &MainWindow, handle: ArtifactPreviewHandle) {
    ui.on_open_artifact_preview_fixture(move |fixture_id| {
        let source = match fixture_id.as_str() {
            "markdown" => markdown_fixture(),
            "image-png" => image_fixture(),
            "plain-text" => text_fixture(
                "fixture:preview:plain-text",
                "preview-output.log",
                "text/plain",
                PLAIN_TEXT_FIXTURE,
            ),
            "json-text" => text_fixture(
                "fixture:preview:json",
                "preview-metadata.json",
                "application/json",
                JSON_FIXTURE,
            ),
            "unsupported-binary" => unsupported_fixture(),
            unknown => {
                log::warn!("[ArtifactPreview::Fixture] Unknown fixture: {unknown}");
                return;
            }
        };
        handle.open_source(source);
    });
}

fn markdown_fixture() -> ArtifactSource {
    ArtifactSource::inline_markdown(
        "fixture:preview:markdown",
        "preview-document.md",
        MARKDOWN_FIXTURE,
    )
}

fn image_fixture() -> ArtifactSource {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ui/assets/illustrations/vuetify-empty-state-teamwork.png");
    ArtifactSource::file(path, Some("image/png".to_string()))
}

fn text_fixture(id: &str, title: &str, mime_type: &str, content: &str) -> ArtifactSource {
    ArtifactSource {
        id: id.to_string(),
        title: title.to_string(),
        mime_type: Some(mime_type.to_string()),
        hint: ArtifactHint::PlainText,
        body: ArtifactBody::Text(content.to_string()),
    }
}

fn unsupported_fixture() -> ArtifactSource {
    ArtifactSource {
        id: "fixture:preview:unsupported-binary".to_string(),
        title: "unsupported-payload.bin".to_string(),
        mime_type: Some("application/octet-stream".to_string()),
        hint: ArtifactHint::Auto,
        body: ArtifactBody::Bytes(vec![0xff, 0x00, 0x81, 0x7f, 0xfe, 0x10]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usher_preview::{
        CancellationToken, IMAGE_PROVIDER_ID, MARKDOWN_PROVIDER_ID, PLAIN_TEXT_PROVIDER_ID,
        PreparedContent, PreviewError, PreviewRegistry,
    };

    #[test]
    fn fixtures_select_the_expected_providers_and_error_state() {
        let registry = PreviewRegistry::with_builtins();
        let cancellation = CancellationToken::default();

        let markdown = registry
            .prepare(markdown_fixture(), &cancellation)
            .expect("Markdown fixture should parse");
        assert_eq!(markdown.provider_id, MARKDOWN_PROVIDER_ID);
        assert!(matches!(markdown.content, PreparedContent::Markdown(_)));

        let image = registry
            .prepare(image_fixture(), &cancellation)
            .expect("PNG fixture should decode");
        assert_eq!(image.provider_id, IMAGE_PROVIDER_ID);
        assert!(matches!(image.content, PreparedContent::Image(_)));

        let text = registry
            .prepare(
                text_fixture("id", "title", "application/json", JSON_FIXTURE),
                &cancellation,
            )
            .expect("JSON fixture should use plain text");
        assert_eq!(text.provider_id, PLAIN_TEXT_PROVIDER_ID);
        assert!(matches!(text.content, PreparedContent::PlainText(_)));

        let unsupported = registry.prepare(unsupported_fixture(), &cancellation);
        assert!(matches!(unsupported, Err(PreviewError::Unsupported)));
    }
}
