//! Development-only Preview fixtures that exercise the production facade.

use std::path::PathBuf;

use super::{ArtifactBody, ArtifactHint, ArtifactPreviewHandle, ArtifactSource};
use crate::MainWindow;

const MARKDOWN_FIXTURE: &str = r#"# Artifact Preview

This document exercises **strong text**, *emphasis*, ~~strikethrough~~,
`inline code`, and an
[HTTP link](https://github.com).

The model-style path `docs/terminal\_viewport\_iteration\_notes.md` must not
show residual escape characters in prose.

## Lists and tasks

- First list item
- Nested-looking content with 中文、日本語、한국어
- [x] Completed task
- [ ] Pending task

> 注意：注册操作必须在调用其他企业微信功能（如 `share()`、`isInstalled()` 或
> `startup()`）之前完成，否则可能导致功能调用失败。

## Review response rhythm

从 `Cargo.lock` 的变化可以推断 `Cargo.toml` 可能同步调整了依赖。这个段落故意包含
足以在窄 Preview 中换成多行的中英文内容，用来核对 14 px 字号、22 px 行盒和段落之间
的留白是否与 AI Review 消息保持一致。

1. portable-pty** 是否被正确引入**
2. `Cargo.toml` 与 `Cargo.lock` 的一致性
3. 是否意外启用了不需要的 feature

建议在合并前运行以下命令，并确认代码块的语言标签、顶部空间和底部空间都保持紧凑：

```sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
git diff --check
```

## Table

| Provider | Enabled | Capability |
| --- | --- | --- |
| Markdown | Yes | Copy and links |
| Image | Yes | Zoom |
| Plain text | Yes | Copy and search |

## Code

```rust
// Fenced code keeps its language and is highlighted before reaching Slint.
let source = ArtifactSource::inline_markdown(
    "fixture:preview:markdown",
    "preview-document.md",
    markdown,
);
preview.open_source(source);
```

```typescript
import { WeCom } from '@picc/jsbridge';

// 注册企业微信应用
await WeCom.register({
    appId: 'your-app-id',      // 企业微信开发者ID
    corpId: 'your-corp-id',    // 企业微信企业ID
    agentId: 'your-agent-id',  // 企业微信企业应用ID
});
```

#### Compact heading

H4 stays clearly subordinate to the main document sections.

##### Supporting heading

H5 remains scannable without becoming oversized.

###### Fine print heading

H6 keeps heading weight at the body scale.

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

    if let Ok(fixture_id) = std::env::var("USHER_ARTIFACT_PREVIEW_FIXTURE")
        && !fixture_id.trim().is_empty()
    {
        ui.invoke_open_artifact_preview_fixture(fixture_id.into());
    }
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
