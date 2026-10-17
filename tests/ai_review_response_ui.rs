#![cfg(feature = "ai-code-review")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;
use usher_lib::PrReviewUiExt;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Global, LogicalPosition, LogicalSize, Model, ModelRc, VecModel};
use usher_lib::{
    AgentResponseBlock as MainAgentResponseBlock,
    AgentResponseBlockKind as MainAgentResponseBlockKind, AiReviewConversationItem,
    AiReviewFindingItem, AiReviewResponseVisualRow, AppState,
    ArtifactPreviewKind as AppArtifactPreviewKind, MainWindow, Route, WorkflowPrConversationItem,
    install_transient_input_bridge,
};

slint::slint! {
    #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
    #[library_path(material) = "material-1.18.1/material.slint"]
    import {
        AgentResponseBlockKind,
        AgentResponseBlock,
        GitHubAgentResponse,
    }
        from "ui/components/agent_response/github_agent_response.slint";
    import { GitHubPassiveScrollView, GitHubScrollView }
        from "ui/components/github_scroll_view.slint";
    import { GitHubAiReviewUserMessage }
        from "ui/components/ai_review/github_ai_review_components.slint";
    import {
        ArtifactPreviewBlock,
        ArtifactPreviewKind,
        GitHubArtifactPreviewSurface,
    } from "ui/components/artifact_preview/github_artifact_preview.slint";

    export component AgentResponseHarness inherits Window {
        in property <length> viewport-width: 420px;
        width: root.viewport-width;
        height: 120px;

        in property <[AgentResponseBlock]> blocks: [];
        in property <string> preview-title: "";
        in property <bool> preview-available: false;
        in property <string> body-font-family: "sans-serif";
        out property <length> response-height: response-view.height;
        callback open-full-response();
        callback open-response-link(string);
        response-view := GitHubAgentResponse {
            width: parent.width;
            blocks: root.blocks;
            preview-title: root.preview-title;
            preview-available: root.preview-available;
            body-font-family: root.body-font-family;
            open-full-response => { root.open-full-response(); }
            open-link(url) => { root.open-response-link(url); }
        }
    }

    export component ResponseScrollHarness inherits Window {
        width: 420px;
        height: 240px;
        callback wheel-consumed(length);

        GitHubScrollView {
            width: parent.width;
            height: parent.height;
            user-wheel-direction(movement) => { root.wheel-consumed(movement); }

            Rectangle {
                width: parent.width;
                height: 2400px;
            }
        }
    }

    export component PassiveScrollHarness inherits Window {
        width: 320px;
        height: 180px;
        out property <length> scroll-y <=> passive-scroll.content-y;
        out property <bool> selection-begun: false;
        out property <int> selection-moves: 0;
        out property <length> selection-x: selection-drag.absolute-position.x;
        out property <length> selection-y: selection-drag.absolute-position.y;
        out property <length> selection-width: selection-drag.width;
        out property <length> selection-height: selection-drag.height;

        passive-scroll := GitHubPassiveScrollView {
            width: parent.width;
            height: parent.height;
            content-width: self.visible-width;
            content-height: 1200px;

            selection-drag := TouchArea {
                x: 0px;
                y: 0px;
                width: passive-scroll.visible-width;
                height: 160px;
                pointer-event(event) => {
                    if event.kind == PointerEventKind.down {
                        root.selection-begun = true;
                    }
                }
                moved => { root.selection-moves += 1; }
            }
        }
    }

    export component ArtifactPreviewLayoutHarness inherits Window {
        width: 323px;
        height: 300px;

        in property <[ArtifactPreviewBlock]> blocks: [];

        GitHubArtifactPreviewSurface {
            width: parent.width;
            height: parent.height;
            preview-kind: ArtifactPreviewKind.Markdown;
            markdown-blocks: root.blocks;
            virtualized-markdown: false;
        }
    }

    export component AiReviewUserMessageHarness inherits Window {
        width: 320px;
        height: 180px;

        in property <string> content: "";
        out property <length> raw-text-preferred-height: raw-text-measure.preferred-height;

        raw-text-measure := Text {
            visible: false;
            text: root.content;
            font-size: 14px;
        }

        GitHubAiReviewUserMessage {
            width: parent.width;
            content: root.content;
        }
    }
}

fn click(window: &slint::Window, position: LogicalPosition) {
    window.dispatch_event(WindowEvent::PointerMoved { position });
    window.dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.5,
        "expected {expected}px, got {actual}px"
    );
}

fn response_harness() -> AgentResponseHarness {
    let response = AgentResponseHarness::new().expect("agent response harness");
    response.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(420.0, 120.0),
    });
    response
}

fn artifact_preview_layout_harness(width: f32) -> ArtifactPreviewLayoutHarness {
    let harness = ArtifactPreviewLayoutHarness::new().expect("artifact Preview harness");
    harness.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(width, 300.0),
    });
    harness
}

fn configure_pr_review_window(width: f32) -> MainWindow {
    let window = MainWindow::new().expect("main window");
    slint::select_bundled_translation("en").expect("English test translations");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(width, 860.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_show_breadcrumb(false);
    window.set_current_route(Route::PrReview);
    window.set_ai_review_panel_visible(true);
    window.set_ai_review_current_task_id("review-task-1".into());
    window.set_ai_review_status("cancelled".into());
    window
}

fn one_message_conversation() -> ModelRc<AiReviewConversationItem> {
    formatted_conversation(1)
}

fn formatted_conversation(block_count: usize) -> ModelRc<AiReviewConversationItem> {
    formatted_assistant_conversation(1, block_count)
}

fn formatted_assistant_conversation(
    message_count: usize,
    blocks_per_message: usize,
) -> ModelRc<AiReviewConversationItem> {
    let messages = (0..message_count)
        .map(|message_index| {
            let blocks = (0..blocks_per_message)
                .map(|block_index| {
                    let text = format!(
                        "Review {message_index} paragraph {block_index}: conversation and scroll state must survive Preview host changes."
                    );
                    MainAgentResponseBlock {
                        kind: MainAgentResponseBlockKind::Paragraph,
                        styled: slint::StyledText::from_plain_text(&text),
                        plain_text: text.into(),
                        list_marker: "".into(),
                        list_depth: 0,
                        code_language: "".into(),
                        span_count: 1,
                        line_count: 1,
                    }
                })
                .collect::<Vec<_>>();
            AiReviewConversationItem {
                agent_label: "".into(),
                agent_name: "".into(),
                agent_provider: "".into(),
                agent_model: "".into(),
                id: format!("message-{}", message_index + 1).into(),
                role: "assistant".into(),
                content: format!(
                    "## Review result {}\n\nConversation must survive Preview host changes.",
                    message_index + 1
                )
                .into(),
                elapsed: "1.2s".into(),
                formatted: true,
                blocks: ModelRc::new(Rc::new(VecModel::from(blocks))),
                preview_title: format!("Review result {}", message_index + 1).into(),
                preview_available: true,
                truncated: false,
                projected_bytes: (blocks_per_message * 96) as i32,
                repository_action_count: 1,
            }
        })
        .collect::<Vec<_>>();
    ModelRc::new(Rc::new(VecModel::from(messages)))
}

fn response_visual_rows(
    conversation: &ModelRc<AiReviewConversationItem>,
) -> ModelRc<AiReviewResponseVisualRow> {
    let empty_finding = AiReviewFindingItem::default();
    let empty_message = AiReviewConversationItem::default();
    let mut rows = Vec::with_capacity(conversation.row_count() + 3);
    rows.push(AiReviewResponseVisualRow {
        kind: "review-state".into(),
        finding: empty_finding.clone(),
        message: empty_message.clone(),
    });
    rows.push(AiReviewResponseVisualRow {
        kind: "separator".into(),
        finding: empty_finding.clone(),
        message: empty_message.clone(),
    });
    rows.extend(
        conversation
            .iter()
            .map(|message| AiReviewResponseVisualRow {
                kind: "message".into(),
                finding: empty_finding.clone(),
                message,
            }),
    );
    rows.push(AiReviewResponseVisualRow {
        kind: "chat-status".into(),
        finding: empty_finding,
        message: empty_message,
    });
    ModelRc::new(Rc::new(VecModel::from(rows)))
}

#[test]
fn user_message_bubble_is_content_sized_right_aligned_and_wraps_when_bounded() {
    i_slint_backend_testing::init_no_event_loop();
    let harness = AiReviewUserMessageHarness::new().expect("AI Review user message harness");
    harness.set_content("继续任务".into());
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let bubble =
        ElementHandle::find_by_element_id(&harness, "GitHubAiReviewUserMessage::user-bubble")
            .next()
            .expect("production user message bubble");
    assert!(bubble.size().width < 120.0);
    assert_close(bubble.absolute_position().x + bubble.size().width, 320.0);

    let long_message =
        "请基于当前 pull request 上下文检查 Cargo.toml 的修改，并说明兼容性、内存和 UI 风险。";
    harness.set_content(long_message.into());
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let bubble =
        ElementHandle::find_by_element_id(&harness, "GitHubAiReviewUserMessage::user-bubble")
            .next()
            .expect("updated production user message bubble");
    assert_close(bubble.size().width, 320.0 * 0.88);
    assert!(bubble.size().height > 40.0);
    assert_close(bubble.absolute_position().x + bubble.size().width, 320.0);
    assert!(
        ElementHandle::find_by_accessible_label(&harness, long_message)
            .next()
            .is_some()
    );
}

#[test]
fn user_message_reserves_descender_glyph_safety_below_preferred_height() {
    i_slint_backend_testing::init_no_event_loop();
    let harness = AiReviewUserMessageHarness::new().expect("AI Review user message harness");
    harness.set_content("cargo gypq".into());
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let text = ElementHandle::find_by_accessible_label(&harness, "cargo gypq")
        .next()
        .expect("production user message text");
    assert!(
        text.size().height >= harness.get_raw_text_preferred_height() + 3.0,
        "descenders need glyph safety below Slint's preferred text height"
    );
}

#[test]
fn narrow_cjk_quote_matches_the_production_styled_text_geometry() {
    i_slint_backend_testing::init_no_event_loop();
    let quote_text = "注意：注册操作必须在调用其他企业微信功能（如 share()、isInstalled() 或\nstartup()）之前完成，否则可能导致功能调用失败。";
    let styled = slint::StyledText::from_markdown(
        "注意：注册操作必须在调用其他企业微信功能（如 `share()`、`isInstalled()` 或\n`startup()`）之前完成，否则可能导致功能调用失败。",
    )
    .expect("production quote styled text");
    let spans = ["share()", "isInstalled()", "startup()"]
        .into_iter()
        .map(|code| {
            let start = quote_text.find(code).expect("inline code") as i32;
            ArtifactPreviewInlineSpan {
                kind: ArtifactPreviewInlineKind::Code,
                start,
                end: start + code.len() as i32,
            }
        })
        .collect::<Vec<_>>();
    let harness = artifact_preview_layout_harness(323.0);
    harness.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        ArtifactPreviewBlock {
            kind: ArtifactPreviewBlockKind::Quote,
            styled,
            list_marker: "".into(),
            list_depth: 0,
            code_lines: ModelRc::default(),
            code_language: "".into(),
            plain_text: quote_text.into(),
            layout_spans: ModelRc::new(Rc::new(VecModel::from(spans))),
        },
    ]))));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let block = ElementHandle::find_by_element_id(
        &harness,
        "GitHubArtifactPreviewMarkdownBlock::block-text",
    )
    .next()
    .expect("production quote StyledText");
    let quote_bar = ElementHandle::find_by_element_id(
        &harness,
        "GitHubArtifactPreviewMarkdownBlock::quote-bar",
    )
    .next()
    .expect("production quote rule");

    assert_close(
        block.absolute_position().x - quote_bar.absolute_position().x,
        20.0,
    );
    assert_close(block.size().width, 283.0);
    assert_close(quote_bar.absolute_position().y, block.absolute_position().y);
    assert_close(quote_bar.size().width, 4.0);
    assert_close(quote_bar.size().height, block.size().height - 4.0);
    assert!(
        (90.0..=92.0).contains(&block.size().height),
        "wrapped Preview prose must keep the Codex 22px line rhythm, got {}px",
        block.size().height
    );
}

#[test]
fn mixed_script_preview_heading_reserves_the_production_glyph_height() {
    i_slint_backend_testing::init_no_event_loop();
    let text = "从 Cargo.lock 变化推测 / Mixed glyph baseline";
    let harness = artifact_preview_layout_harness(520.0);
    harness.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        ArtifactPreviewBlock {
            kind: ArtifactPreviewBlockKind::Heading2,
            styled: slint::StyledText::from_markdown(&format!("**{text}**"))
                .expect("production heading styled text"),
            list_marker: "".into(),
            list_depth: 0,
            code_lines: ModelRc::default(),
            code_language: "".into(),
            plain_text: text.into(),
            layout_spans: ModelRc::default(),
        },
    ]))));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let heading = ElementHandle::find_by_element_id(
        &harness,
        "GitHubArtifactPreviewMarkdownBlock::block-text",
    )
    .next()
    .expect("production heading StyledText");

    assert_close(heading.size().height, 44.0);
}

#[test]
fn preview_blocks_match_the_ai_review_paragraph_and_list_rhythm() {
    i_slint_backend_testing::init_no_event_loop();
    let harness = artifact_preview_layout_harness(520.0);
    let block = |kind, text: &'static str| ArtifactPreviewBlock {
        kind,
        styled: slint::StyledText::from_plain_text(text),
        list_marker: if kind == ArtifactPreviewBlockKind::ListItem {
            "•".into()
        } else {
            "".into()
        },
        list_depth: 0,
        code_lines: ModelRc::default(),
        code_language: "".into(),
        plain_text: text.into(),
        layout_spans: ModelRc::default(),
    };
    harness.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        block(ArtifactPreviewBlockKind::Paragraph, "First paragraph"),
        block(ArtifactPreviewBlockKind::Paragraph, "Second paragraph"),
        block(ArtifactPreviewBlockKind::ListItem, "• First item"),
        block(ArtifactPreviewBlockKind::ListItem, "• Second item"),
        block(ArtifactPreviewBlockKind::Paragraph, "Closing paragraph"),
    ]))));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let bodies = ElementHandle::find_by_element_id(
        &harness,
        "GitHubArtifactPreviewMarkdownBlock::block-text",
    )
    .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 5);
    let visible_gap = |before: &ElementHandle, after: &ElementHandle| {
        after.absolute_position().y - before.absolute_position().y - before.size().height
    };
    assert_close(visible_gap(&bodies[0], &bodies[1]), 20.0);
    assert_close(visible_gap(&bodies[1], &bodies[2]), 10.0);
    assert_close(visible_gap(&bodies[2], &bodies[3]), 0.0);
    assert_close(visible_gap(&bodies[3], &bodies[4]), 10.0);
}

#[test]
fn preview_title_link_supports_the_accessible_default_action() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    response.set_preview_title("Review result".into());
    response.set_preview_available(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(response.get_response_height() >= 28.0);

    let activated = Rc::new(Cell::new(false));
    let observed = activated.clone();
    response.on_open_full_response(move || observed.set(true));

    let title = ElementHandle::find_by_accessible_label(&response, "Review result")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .expect("production response title link");
    assert!(
        title.size().width < 200.0,
        "short title must keep intrinsic width"
    );
    title.invoke_accessible_default_action();

    assert!(activated.get());
}

#[test]
fn response_title_pointer_action_forwards_the_full_artifact_request() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    response.set_preview_title("Review result".into());
    response.set_preview_available(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(response.get_response_height() >= 28.0);

    let opened = Rc::new(Cell::new(false));
    let observed = opened.clone();
    response.on_open_full_response(move || observed.set(true));

    let title = ElementHandle::find_by_accessible_label(&response, "Review result")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .expect("production response title link");
    title.mock_single_click(PointerEventButton::Left);

    assert!(opened.get());
}

#[test]
fn leading_markdown_heading_is_the_only_full_preview_action() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    response.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        AgentResponseBlock {
            kind: AgentResponseBlockKind::Heading1,
            styled: slint::StyledText::from_plain_text("Review result"),
            plain_text: "Review result".into(),
            list_marker: "".into(),
            list_depth: 0,
            code_language: "".into(),
            span_count: 1,
            line_count: 1,
        },
        AgentResponseBlock {
            kind: AgentResponseBlockKind::Paragraph,
            styled: slint::StyledText::from_plain_text("Structured details."),
            plain_text: "Structured details.".into(),
            list_marker: "".into(),
            list_depth: 0,
            code_language: "".into(),
            span_count: 1,
            line_count: 1,
        },
    ]))));
    response.set_preview_title("Fallback title".into());
    response.set_preview_available(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let activated = Rc::new(Cell::new(false));
    let observed = activated.clone();
    response.on_open_full_response(move || observed.set(true));

    let heading = ElementHandle::find_by_accessible_label(&response, "Review result")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .expect("leading heading preview action");
    assert!(
        ElementHandle::find_by_accessible_label(&response, "Fallback title")
            .next()
            .is_none(),
        "leading headings must suppress the duplicate fallback action"
    );
    heading.invoke_accessible_default_action();

    assert!(activated.get());
}

#[test]
fn mixed_script_heading_and_multiline_code_reserve_safe_heights() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    response.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        AgentResponseBlock {
            kind: AgentResponseBlockKind::Heading2,
            styled: slint::StyledText::from_plain_text("从 Cargo.lock 变化推测"),
            plain_text: "从 Cargo.lock 变化推测".into(),
            list_marker: "".into(),
            list_depth: 0,
            code_language: "".into(),
            span_count: 1,
            line_count: 1,
        },
        AgentResponseBlock {
            kind: AgentResponseBlockKind::CodeBlock,
            styled: slint::StyledText::from_plain_text("cargo check\ncargo test"),
            plain_text: "cargo check\ncargo test".into(),
            list_marker: "".into(),
            list_depth: 0,
            code_language: "sh".into(),
            span_count: 0,
            line_count: 2,
        },
    ]))));
    response.set_preview_available(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let heading =
        ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::heading-text")
            .next()
            .expect("production mixed-script heading");
    let code = ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::code-text")
        .next()
        .expect("production multiline code");

    assert!(heading.size().height >= 25.0);
    assert!(code.size().height >= 36.0);
    assert!(response.get_response_height() >= heading.size().height + code.size().height + 40.0);
}

#[test]
fn list_marker_uses_a_stable_gutter_and_hanging_body_inset() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    response.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(220.0, 120.0),
    });
    response.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        AgentResponseBlock {
            kind: AgentResponseBlockKind::ListItem,
            styled: slint::StyledText::from_plain_text(
                "A long list item keeps every wrapped continuation aligned with the body.",
            ),
            plain_text: "A long list item keeps every wrapped continuation aligned with the body."
                .into(),
            list_marker: "•".into(),
            list_depth: 0,
            code_language: "".into(),
            span_count: 1,
            line_count: 1,
        },
        AgentResponseBlock {
            kind: AgentResponseBlockKind::ListItem,
            styled: slint::StyledText::from_plain_text("Nested body"),
            plain_text: "Nested body".into(),
            list_marker: "2.".into(),
            list_depth: 1,
            code_language: "".into(),
            span_count: 1,
            line_count: 1,
        },
    ]))));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let markers =
        ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::list-marker-text")
            .collect::<Vec<_>>();
    let bodies =
        ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::body-text")
            .collect::<Vec<_>>();
    assert_eq!(markers.len(), 2);
    assert_eq!(bodies.len(), 2);
    assert!(markers[0].absolute_position().x < bodies[0].absolute_position().x);
    assert_close(bodies[0].absolute_position().x, 24.0);
    assert_close(bodies[1].absolute_position().x, 48.0);
    assert!(bodies[0].size().height > 22.0);
}

#[test]
fn response_quote_rule_aligns_with_the_text_line_box() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    response.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        AgentResponseBlock {
            kind: AgentResponseBlockKind::Quote,
            styled: slint::StyledText::from_plain_text(
                "Full Markdown remains available through Preview.",
            ),
            plain_text: "Full Markdown remains available through Preview.".into(),
            list_marker: "".into(),
            list_depth: 0,
            code_language: "".into(),
            span_count: 1,
            line_count: 1,
        },
    ]))));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let quote_bar =
        ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::quote-bar")
            .next()
            .expect("response quote rule");
    let body = ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::body-text")
        .next()
        .expect("response quote text");

    assert_close(
        body.absolute_position().x - quote_bar.absolute_position().x,
        20.0,
    );
    assert_close(quote_bar.absolute_position().y, body.absolute_position().y);
    assert_close(quote_bar.size().width, 4.0);
    assert_close(
        quote_bar.size().height,
        (body.size().height - 4.0).max(22.0),
    );
}

#[test]
fn response_blocks_match_codex_paragraph_and_list_rhythm() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    let block = |kind, text: &'static str, marker: &'static str| AgentResponseBlock {
        kind,
        styled: slint::StyledText::from_plain_text(text),
        plain_text: text.into(),
        list_marker: marker.into(),
        list_depth: 0,
        code_language: "".into(),
        span_count: 1,
        line_count: 1,
    };
    response.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        block(AgentResponseBlockKind::Paragraph, "First paragraph", ""),
        block(AgentResponseBlockKind::Paragraph, "Second paragraph", ""),
        block(AgentResponseBlockKind::ListItem, "First item", "•"),
        block(AgentResponseBlockKind::ListItem, "Second item", "•"),
        block(AgentResponseBlockKind::Paragraph, "Closing paragraph", ""),
    ]))));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let bodies =
        ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::body-text")
            .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 5);
    let visible_gap = |before: &ElementHandle, after: &ElementHandle| {
        after.absolute_position().y - before.absolute_position().y - before.size().height
    };
    assert_close(visible_gap(&bodies[0], &bodies[1]), 20.0);
    assert_close(visible_gap(&bodies[1], &bodies[2]), 10.0);
    assert_close(visible_gap(&bodies[2], &bodies[3]), 0.0);
    assert_close(visible_gap(&bodies[3], &bodies[4]), 10.0);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_mixed_response_uses_the_codex_system_font_fallback() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    response.set_viewport_width(220.0);
    response.set_body_font_family(".AppleSystemUIFont".into());
    let text =
        "中文 AI review uses the macOS system font fallback，需要在较窄的消息面板中正常换行。";
    let blocks = ModelRc::new(Rc::new(VecModel::from(vec![AgentResponseBlock {
        kind: AgentResponseBlockKind::Paragraph,
        styled: slint::StyledText::from_plain_text(text),
        plain_text: text.into(),
        list_marker: "".into(),
        list_depth: 0,
        code_language: "".into(),
        span_count: 1,
        line_count: 1,
    }])));
    response.set_blocks(blocks);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let body = ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::body-text")
        .next()
        .expect("wrapped response body");
    assert_close(body.size().width, 220.0);
    assert!(
        (66.0..=72.0).contains(&body.size().height),
        "three wrapped mixed English/CJK lines must keep Preview-style leading, got {}px",
        body.size().height
    );
}

#[test]
fn long_response_title_elides_within_the_available_action_width() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    let title_text = "Review result ".repeat(20);
    response.set_preview_title(title_text.clone().into());
    response.set_preview_available(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let activated = Rc::new(Cell::new(false));
    let observed = activated.clone();
    response.on_open_full_response(move || observed.set(true));

    let title = ElementHandle::find_by_accessible_label(&response, &title_text)
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .expect("bounded production response title link");
    let position = title.absolute_position();
    let size = title.size();
    assert!(position.x >= 24.0);
    assert!(position.x + size.width <= 420.0);
    assert_close(size.width, 396.0);

    let label = ElementHandle::find_by_element_id(&response, "GitHubLink::link-label")
        .next()
        .expect("bounded title label");
    assert_close(label.size().width, size.width);

    title.invoke_accessible_default_action();
    assert!(activated.get());
}

#[test]
fn repository_markdown_link_pointer_action_forwards_the_opaque_target() {
    i_slint_backend_testing::init_no_event_loop();
    let response = response_harness();
    let target = "usher-repo-file:opaque-token";
    let styled = slint::StyledText::from_markdown(&format!("[AGENTS.md]({target})"))
        .expect("inline repository link");
    response.set_blocks(ModelRc::new(Rc::new(VecModel::from(vec![
        AgentResponseBlock {
            kind: AgentResponseBlockKind::Paragraph,
            styled,
            plain_text: "AGENTS.md".into(),
            list_marker: "".into(),
            list_depth: 0,
            code_language: "".into(),
            span_count: 1,
            line_count: 1,
        },
    ]))));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(response.get_response_height() > 0.0);

    let opened = Rc::new(RefCell::new(String::new()));
    let observed = opened.clone();
    response.on_open_response_link(move |url| {
        *observed.borrow_mut() = url.to_string();
    });

    let body = ElementHandle::find_by_element_id(&response, "GitHubAgentResponseBlock::body-text")
        .next()
        .expect("production StyledText body");
    let body_position = body.absolute_position();
    let body_size = body.size();
    // The fixture paragraph consists only of the link, so a leading-glyph hit
    // exercises StyledText's real link-clicked path deterministically.
    click(
        response.window(),
        LogicalPosition::new(
            body_position.x + 24.0,
            body_position.y + body_size.height / 2.0,
        ),
    );

    assert_eq!(opened.borrow().as_str(), target);
}

#[test]
fn ai_review_content_click_dismisses_the_adjacent_diff_settings_popover() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(1300.0);
    install_transient_input_bridge(&window).expect("transient input bridge");
    window
        .pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_tab(2);
    let target = "https://example.com/review-details";
    window.set_ai_review_conversation(ModelRc::new(Rc::new(VecModel::from(vec![
        AiReviewConversationItem {
            agent_label: "".into(),
            agent_name: "".into(),
            agent_provider: "".into(),
            agent_model: "".into(),
            id: "message-with-link".into(),
            role: "assistant".into(),
            content: "Review details".into(),
            elapsed: "1.0s".into(),
            formatted: true,
            blocks: ModelRc::new(Rc::new(VecModel::from(vec![MainAgentResponseBlock {
                kind: MainAgentResponseBlockKind::Paragraph,
                styled: slint::StyledText::from_markdown(&format!("[Review details]({target})"))
                    .expect("formatted response link"),
                plain_text: "Review details".into(),
                list_marker: "".into(),
                list_depth: 0,
                code_language: "".into(),
                span_count: 1,
                line_count: 1,
            }]))),
            preview_title: "".into(),
            preview_available: false,
            truncated: false,
            projected_bytes: 32,
            repository_action_count: 0,
        },
    ]))));
    let opened = Rc::new(RefCell::new(String::new()));
    let observed = opened.clone();
    window.on_ai_review_open_response_link(move |url| {
        *observed.borrow_mut() = url.to_string();
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let settings = ElementHandle::find_by_accessible_label(&window, "Open Diff view settings")
        .next()
        .expect("Files changed diff settings button");
    let settings_position = settings.absolute_position();
    let settings_size = settings.size();
    click(
        window.window(),
        LogicalPosition::new(
            settings_position.x + settings_size.width / 2.0,
            settings_position.y + settings_size.height / 2.0,
        ),
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        ElementHandle::find_by_element_id(
            &window,
            "GitHubDiffSettingsPopover::popover-pointer-shield",
        )
        .next()
        .is_some(),
        "diff settings popover should be visible before the AI Review click"
    );

    let body = ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text")
        .next()
        .expect("formatted AI Review StyledText body");
    let body_position = body.absolute_position();
    let body_size = body.size();
    click(
        window.window(),
        LogicalPosition::new(
            body_position.x + 24.0,
            body_position.y + body_size.height / 2.0,
        ),
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    assert_eq!(
        opened.borrow().as_str(),
        target,
        "link action must still run"
    );
    assert!(
        ElementHandle::find_by_element_id(
            &window,
            "GitHubDiffSettingsPopover::popover-pointer-shield",
        )
        .next()
        .is_none(),
        "AI Review interaction must dismiss a popover owned by the adjacent PR workspace"
    );

    click(
        window.window(),
        LogicalPosition::new(
            settings_position.x + settings_size.width / 2.0,
            settings_position.y + settings_size.height / 2.0,
        ),
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    let ask_input = ElementHandle::find_by_element_id(&window, "GitHubAiReviewPanel::ask-input")
        .next()
        .expect("AI Review Ask input");
    let input_position = ask_input.absolute_position();
    let input_size = ask_input.size();
    click(
        window.window(),
        LogicalPosition::new(
            input_position.x + input_size.width / 2.0,
            input_position.y + input_size.height / 2.0,
        ),
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        ElementHandle::find_by_element_id(
            &window,
            "GitHubDiffSettingsPopover::popover-pointer-shield",
        )
        .next()
        .is_none(),
        "focusing a native input must dismiss an adjacent transient popover"
    );

    window.set_ai_review_panel_visible(false);
    window
        .pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_tab(0);
    window
        .pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_can_comment(true);
    window
        .pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_conversation_content_height(48);
    window
        .pr_review()
        .global::<usher_lib::PrReviewBridge>()
        .set_workflow_pr_review_conversation_items(ModelRc::new(Rc::new(VecModel::from(vec![
            WorkflowPrConversationItem {
                id: "review-summary".into(),
                kind: "review_summary".into(),
                author: "Reviewer".into(),
                avatar: slint::Image::default(),
                avatar_label: "R".into(),
                author_association: "MEMBER".into(),
                body: "requested changes".into(),
                message_body: "".into(),
                message_body_height: 0,
                message_expanded: false,
                created_at: "now".into(),
                created_at_tooltip: "now".into(),
                state: "".into(),
                review_id: "review-1".into(),
                can_dismiss: true,
                connect_before: false,
                connect_after: false,
                row_height: 48,
            },
        ]))));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    ElementHandle::find_by_accessible_label(&window, "Review actions")
        .next()
        .expect("review action trigger")
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubActionMenu::menu-surface")
            .next()
            .is_some(),
        "review action menu should be open before focusing the comment input"
    );

    let comment_input = ElementHandle::find_by_element_id(
        &window,
        "GitHubPullRequestReviewPage::conversation-comment-editor",
    )
    .next()
    .expect("Conversation comment input");
    comment_input.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubActionMenu::menu-surface")
            .next()
            .is_none(),
        "focusing the Conversation comment input must dismiss the review action menu"
    );
}

#[test]
fn ai_review_composer_divider_tracks_editor_width_and_clears_rounded_corners() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(800.0);
    window.show().unwrap();
    for width in [800.0, 1300.0] {
        window.window().dispatch_event(WindowEvent::Resized {
            size: LogicalSize::new(width, 860.0),
        });
        for locale in ["en", "zh-CN"] {
            slint::select_bundled_translation(locale).unwrap();
            for dark in [false, true] {
                AppState::get(&window).set_effective_dark_mode(dark);
                window.window().dispatch_event(WindowEvent::PointerMoved {
                    position: LogicalPosition::new(1.0, 1.0),
                });
                i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
                let handle = ElementHandle::find_by_element_id(
                    &window,
                    "GitHubAiReviewPanel::composer-resize-handle",
                )
                .next()
                .expect("AI Review composer resize handle");
                let editor =
                    ElementHandle::find_by_element_id(&window, "GitHubAiReviewPanel::composer-box")
                        .next()
                        .expect("AI Review composer editor");
                let line = handle
                    .query_descendants()
                    .match_id("GitHubResizeHandle::divider-line")
                    .find_first()
                    .expect("composer painted divider");
                let editor_left = editor.absolute_position().x;
                let editor_right = editor_left + editor.size().width;
                assert_close(handle.absolute_position().x, editor_left);
                assert_close(handle.size().width, editor.size().width);
                assert_close(handle.size().height, 9.0);
                assert_close(line.size().height, 1.0);
                assert_close(line.absolute_position().x, editor_left + 6.0);
                assert_close(
                    line.absolute_position().x + line.size().width,
                    editor_right - 6.0,
                );
                let position = LogicalPosition::new(
                    editor_left + editor.size().width / 2.0,
                    handle.absolute_position().y + handle.size().height / 2.0,
                );
                window
                    .window()
                    .dispatch_event(WindowEvent::PointerMoved { position });
                i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
                assert_close(line.size().height, 2.0);
                assert_close(line.absolute_position().x, editor_left + 6.0);
                assert_close(
                    line.absolute_position().x + line.size().width,
                    editor_right - 6.0,
                );
                assert_close(handle.size().width, editor.size().width);
                assert_close(handle.size().height, 9.0);
            }
        }
    }
}

#[test]
fn ai_review_composer_resizes_upward_with_tabs_and_scrolls_long_input() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(1300.0);
    window.set_ai_review_message_input(
        (0..80)
            .map(|index| format!("Ask line {index}"))
            .collect::<Vec<_>>()
            .join("\n")
            .into(),
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let composer = ElementHandle::find_by_element_id(&window, "GitHubAiReviewPanel::composer")
        .next()
        .expect("AI Review composer");
    let resize_handle =
        ElementHandle::find_by_element_id(&window, "GitHubAiReviewPanel::composer-resize-handle")
            .next()
            .expect("AI Review composer resize handle");
    let composer_box =
        ElementHandle::find_by_element_id(&window, "GitHubAiReviewPanel::composer-box")
            .next()
            .expect("AI Review composer editor");
    let ask_tab = ElementHandle::find_by_accessible_label(&window, "Ask")
        .next()
        .expect("Ask composer mode");
    let initial_height = composer.size().height;
    let initial_bottom = composer.absolute_position().y + initial_height;
    let initial_tab_y = ask_tab.absolute_position().y;
    let handle_position = resize_handle.absolute_position();
    let handle_size = resize_handle.size();
    let editor_top = composer_box.absolute_position().y;
    assert!(
        handle_position.y <= editor_top
            && handle_position.y + handle_size.height >= editor_top + 4.0,
        "resize handle must straddle the editor top border: handle_y={}, handle_height={}, editor_top={editor_top}",
        handle_position.y,
        handle_size.height,
    );
    assert!(
        handle_position.y > composer.absolute_position().y + 30.0,
        "resize handle must not float above the composer tabs"
    );
    let start = LogicalPosition::new(
        handle_position.x + handle_size.width / 2.0,
        handle_position.y + handle_size.height / 2.0,
    );
    let target = LogicalPosition::new(start.x, start.y - 1000.0);
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position: start });
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: start,
        button: PointerEventButton::Left,
    });
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position: target });
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: target,
            button: PointerEventButton::Left,
        });
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let resized_height = composer.size().height;
    assert!(
        resized_height > initial_height + 100.0,
        "upward drag must grow the composer: initial={initial_height}, resized={resized_height}"
    );
    assert!(
        resized_height <= 360.0,
        "composer height must remain bounded: {resized_height}"
    );
    assert_close(
        composer.absolute_position().y + resized_height,
        initial_bottom,
    );
    assert!(
        ask_tab.absolute_position().y < initial_tab_y - 100.0,
        "composer mode tabs must stay attached to the editor while it grows"
    );

    let ask_scroll = ElementHandle::find_by_element_id(&window, "GitHubAiReviewPanel::ask-scroll")
        .next()
        .expect("Ask input scroll viewport");
    let ask_input = ElementHandle::find_by_element_id(&window, "GitHubAiReviewPanel::ask-input")
        .next()
        .expect("Ask input");
    let initial_input_y = ask_input.absolute_position().y;
    ask_scroll.scroll(0.0, -1000.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert!(
        ask_input.absolute_position().y < initial_input_y - 20.0,
        "long Ask content must scroll inside the bounded composer"
    );
}

#[test]
fn retained_response_list_mounts_every_source_block_for_ab_baseline() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(1300.0);
    let conversation = formatted_assistant_conversation(12, 48);
    window.set_ai_review_response_virtualized(false);
    window.set_ai_review_conversation(conversation.clone());
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let source_blocks = conversation
        .iter()
        .map(|message| message.blocks.row_count())
        .sum::<usize>();
    let traversable_blocks =
        ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text").count();
    assert_eq!(conversation.row_count(), 12);
    assert_eq!(source_blocks, 576);
    assert_eq!(
        window.get_ai_review_retained_created_message_rows() as usize,
        conversation.row_count()
    );
    assert_eq!(
        window.get_ai_review_retained_created_blocks() as usize,
        source_blocks
    );
    assert!(traversable_blocks > 0);
    assert_eq!(window.get_ai_review_response_rows().row_count(), 0);
    assert_eq!(window.get_ai_review_virtual_created_rows(), 0);
    assert_eq!(window.get_ai_review_virtual_created_message_rows(), 0);
    assert_eq!(window.get_ai_review_virtual_created_blocks(), 0);
}

#[test]
fn virtualized_response_list_bounds_created_blocks_scrolls_and_preserves_source() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(1300.0);
    let conversation = formatted_assistant_conversation(12, 48);
    let visual_rows = response_visual_rows(&conversation);
    window.set_ai_review_response_virtualized(true);
    window.set_ai_review_conversation(conversation.clone());
    window.set_ai_review_virtual_created_rows(0);
    window.set_ai_review_virtual_created_message_rows(0);
    window.set_ai_review_virtual_created_blocks(0);
    window.set_ai_review_response_rows(visual_rows.clone());
    window.show().expect("show virtualized response fixture");
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let source_blocks = conversation
        .iter()
        .map(|message| message.blocks.row_count())
        .sum::<usize>();
    let created_rows = window.get_ai_review_virtual_created_rows() as usize;
    let created_message_rows = window.get_ai_review_virtual_created_message_rows() as usize;
    let created_blocks = window.get_ai_review_virtual_created_blocks() as usize;
    let traversable_blocks =
        ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text").count();

    assert_eq!(window.get_ai_review_conversation(), conversation);
    assert_eq!(window.get_ai_review_response_rows(), visual_rows);
    assert_eq!(conversation.row_count(), 12);
    assert_eq!(source_blocks, 576);
    assert!(created_rows > 0 && created_rows < visual_rows.row_count());
    assert!(created_message_rows > 0 && created_message_rows < conversation.row_count());
    assert!(created_blocks > 0 && created_blocks < source_blocks);
    assert!(traversable_blocks > 0 && traversable_blocks < source_blocks);
    assert!(created_blocks >= traversable_blocks);

    let movements = Rc::new(RefCell::new(Vec::new()));
    let observed = movements.clone();
    window.on_ai_review_response_wheel_consumed(move |movement| {
        observed.borrow_mut().push(movement);
    });
    let response_list =
        ElementHandle::find_by_element_id(&window, "GitHubAiReviewPanel::virtual-response-list")
            .next()
            .expect("production virtual response ListView");
    let initial_scroll_y = window.get_ai_review_response_scroll_y();
    for delta_y in [-240.0, 240.0] {
        response_list.scroll(0.0, delta_y);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(120));
        if (window.get_ai_review_response_scroll_y() - initial_scroll_y).abs() > 0.5 {
            break;
        }
    }
    let final_scroll_y = window.get_ai_review_response_scroll_y();
    let movement = final_scroll_y - initial_scroll_y;
    assert!(
        movement.abs() > 0.5,
        "virtual response ListView did not scroll"
    );
    assert!(
        movements
            .borrow()
            .iter()
            .any(|reported| reported.abs() > 0.25 && reported.signum() == movement.signum()),
        "scroll callback did not report the viewport movement: {:?}",
        movements.borrow()
    );
    assert_eq!(window.get_ai_review_conversation(), conversation);
    assert_eq!(window.get_ai_review_response_rows(), visual_rows);
}

#[test]
fn virtualized_response_list_forwards_preview_and_markdown_link_actions() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(1300.0);
    window.set_ai_review_response_virtualized(true);

    let preview_conversation = formatted_assistant_conversation(1, 1);
    window.set_ai_review_conversation(preview_conversation.clone());
    window.set_ai_review_response_rows(response_visual_rows(&preview_conversation));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let opened_preview = Rc::new(RefCell::new(Vec::<String>::new()));
    let observed_preview = opened_preview.clone();
    window.on_ai_review_open_response_artifact(move |id, title, markdown| {
        *observed_preview.borrow_mut() =
            vec![id.to_string(), title.to_string(), markdown.to_string()];
    });
    let preview_action = ElementHandle::find_by_accessible_label(&window, "Review result 1")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .expect("virtualized response Preview action");
    preview_action.invoke_accessible_default_action();
    assert_eq!(opened_preview.borrow()[0], "message-1");
    assert_eq!(opened_preview.borrow()[1], "Review result 1");

    let target = "usher-repo-file:virtualized-token";
    let link_conversation = ModelRc::new(Rc::new(VecModel::from(vec![AiReviewConversationItem {
        agent_label: "".into(),
        agent_name: "".into(),
        agent_provider: "".into(),
        agent_model: "".into(),
        id: "link-message".into(),
        role: "assistant".into(),
        content: "AGENTS.md".into(),
        elapsed: "0.8s".into(),
        formatted: true,
        blocks: ModelRc::new(Rc::new(VecModel::from(vec![MainAgentResponseBlock {
            kind: MainAgentResponseBlockKind::Paragraph,
            styled: slint::StyledText::from_markdown(&format!("[AGENTS.md]({target})"))
                .expect("virtualized response link"),
            plain_text: "AGENTS.md".into(),
            list_marker: "".into(),
            list_depth: 0,
            code_language: "".into(),
            span_count: 1,
            line_count: 1,
        }]))),
        preview_title: "".into(),
        preview_available: false,
        truncated: false,
        projected_bytes: 32,
        repository_action_count: 1,
    }])));
    window.set_ai_review_conversation(link_conversation.clone());
    window.set_ai_review_response_rows(response_visual_rows(&link_conversation));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let opened_link = Rc::new(RefCell::new(String::new()));
    let observed_link = opened_link.clone();
    window.on_ai_review_open_response_link(move |url| {
        *observed_link.borrow_mut() = url.to_string();
    });
    let body = ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text")
        .next()
        .expect("virtualized StyledText body");
    let body_position = body.absolute_position();
    let body_size = body.size();
    click(
        window.window(),
        LogicalPosition::new(
            body_position.x + 24.0,
            body_position.y + body_size.height / 2.0,
        ),
    );
    assert_eq!(opened_link.borrow().as_str(), target);
}

#[test]
fn docked_preview_replaces_then_restores_ai_without_losing_conversation() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(1300.0);
    window.set_ai_review_docked_content_unmount(true);
    let conversation = formatted_conversation(36);
    window.set_ai_review_conversation(conversation.clone());
    window.set_ai_review_message_input("preserved question".into());
    window.set_ai_review_extra_requirements("preserved requirements".into());
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(window.get_ai_review_response_delegates_mounted());
    let review_again = ElementHandle::find_by_accessible_label(&window, "Review again")
        .next()
        .expect("Review again composer mode");
    let review_again_position = review_again.absolute_position();
    let review_again_size = review_again.size();
    click(
        window.window(),
        LogicalPosition::new(
            review_again_position.x + review_again_size.width / 2.0,
            review_again_position.y + review_again_size.height / 2.0,
        ),
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert_eq!(review_again.accessible_checked(), Some(true));
    let response_body =
        ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text")
            .next()
            .expect("formatted response delegates before opening Preview");

    for _ in 0..3 {
        window
            .window()
            .dispatch_event(WindowEvent::PointerScrolled {
                position: LogicalPosition::new(1100.0, 320.0),
                delta_x: 0.0,
                delta_y: -240.0,
            });
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(120));
    }
    let response_scroll_y = window.get_ai_review_response_scroll_y();
    assert!(
        response_scroll_y < -1.0,
        "fixture must establish a non-zero response scroll offset, got {response_scroll_y}"
    );
    let trailing_width = window.get_pr_review_trailing_surface_width();
    let main_width = window.get_pr_review_main_content_width();

    let app_state = AppState::get(&window);
    app_state.set_artifact_preview_presentation_mode(0);
    window.set_artifact_preview_title("Review result".into());
    window.set_artifact_preview_kind(AppArtifactPreviewKind::PlainText);
    window.set_artifact_preview_plain_text("full response".into());
    window.set_artifact_preview_pr_side_context(true);
    window.set_artifact_preview_visible(true);

    assert!(window.get_artifact_preview_pr_side_docked_visible());
    assert!(!window.get_ai_review_content_visible());
    assert!(!window.get_ai_review_response_delegates_mounted());
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text")
            .next()
            .is_none(),
        "candidate response delegates must leave the traversable item tree"
    );
    assert!(
        !response_body.is_valid(),
        "candidate must destroy inactive formatted-response delegates"
    );
    assert_close(
        window.get_pr_review_trailing_surface_width(),
        trailing_width,
    );
    assert_close(window.get_pr_review_main_content_width(), main_width);
    assert_eq!(window.get_ai_review_conversation(), conversation);
    assert_eq!(
        window.get_ai_review_message_input().as_str(),
        "preserved question"
    );
    assert_eq!(
        window.get_ai_review_extra_requirements().as_str(),
        "preserved requirements"
    );
    assert!(
        review_again.is_valid(),
        "Docked Preview must retain the light composer tree"
    );
    assert_eq!(review_again.accessible_checked(), Some(true));
    assert_close(window.get_ai_review_response_scroll_y(), response_scroll_y);

    let dismissed = Rc::new(Cell::new(false));
    let observed = dismissed.clone();
    let weak = window.as_weak();
    window.on_artifact_preview_dismiss(move || {
        observed.set(true);
        if let Some(window) = weak.upgrade() {
            window.set_artifact_preview_visible(false);
            window.set_artifact_preview_pr_side_context(false);
        }
    });

    // The PR-side Preview close control is a 32px action ending 8px from the
    // right window edge, centered in its 48px header.
    click(window.window(), LogicalPosition::new(1276.0, 24.0));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    assert!(dismissed.get());
    assert!(!window.get_artifact_preview_pr_side_docked_visible());
    assert!(window.get_ai_review_content_visible());
    assert!(window.get_ai_review_response_delegates_mounted());
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text")
            .next()
            .is_some(),
        "formatted-response delegates must remount after Preview closes"
    );
    assert_eq!(window.get_ai_review_conversation(), conversation);
    assert_eq!(
        window.get_ai_review_message_input().as_str(),
        "preserved question"
    );
    assert_eq!(
        window.get_ai_review_extra_requirements().as_str(),
        "preserved requirements"
    );
    assert_eq!(
        ElementHandle::find_by_accessible_label(&window, "Review again")
            .next()
            .expect("restored Review again composer mode")
            .accessible_checked(),
        Some(true)
    );
    assert_close(window.get_ai_review_response_scroll_y(), response_scroll_y);
    let restored = window
        .get_ai_review_conversation()
        .row_data(0)
        .expect("restored conversation message");
    assert_eq!(restored.id, "message-1");
}

#[test]
fn docked_preview_retain_policy_keeps_response_delegates_for_same_binary_ab() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(1300.0);
    window.set_ai_review_docked_content_unmount(false);
    let conversation = one_message_conversation();
    window.set_ai_review_conversation(conversation.clone());
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    let response_body =
        ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text")
            .next()
            .expect("formatted response delegate before retained Preview");

    AppState::get(&window).set_artifact_preview_presentation_mode(0);
    window.set_artifact_preview_pr_side_context(true);
    window.set_artifact_preview_visible(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    assert!(window.get_artifact_preview_pr_side_docked_visible());
    assert!(!window.get_ai_review_content_visible());
    assert!(window.get_ai_review_response_delegates_mounted());
    assert!(
        response_body.is_valid(),
        "retain policy must preserve the old hidden-but-mounted response tree"
    );
    assert_eq!(window.get_ai_review_conversation(), conversation);
}

#[test]
fn response_scroll_view_consumes_real_wheel_input() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ResponseScrollHarness::new().expect("response scroll harness");
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let movements = Rc::new(RefCell::new(Vec::new()));
    let observed = movements.clone();
    window.on_wheel_consumed(move |movement| {
        observed.borrow_mut().push(movement);
    });

    let expected_directions = [-1.0_f32, 1.0, -1.0];
    for (index, delta_y) in [-240.0, 240.0, -240.0].into_iter().enumerate() {
        window
            .window()
            .dispatch_event(WindowEvent::PointerScrolled {
                position: LogicalPosition::new(210.0, 120.0),
                delta_x: 0.0,
                delta_y,
            });
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(120));
        let observed = movements.borrow();
        assert_eq!(
            observed.len(),
            index + 1,
            "each accepted wheel event must report exactly one viewport movement"
        );
        assert!(
            observed[index].abs() > 0.25 && observed[index].signum() == expected_directions[index],
            "wheel movement direction changed: {observed:?}"
        );
    }
}

#[test]
fn passive_scroll_view_preserves_pointer_drag_and_wheel_scrolling() {
    i_slint_backend_testing::init_no_event_loop();
    let window = PassiveScrollHarness::new().expect("passive scroll harness");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(320.0, 180.0),
    });
    window.show().expect("show passive scroll harness");
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let origin = LogicalPosition::new(window.get_selection_x(), window.get_selection_y());
    let width = window.get_selection_width();
    let height = window.get_selection_height();
    assert!(
        width > 0.0 && height > 0.0,
        "selection drag area must be laid out"
    );
    let start = LogicalPosition::new(origin.x + width / 2.0, origin.y + height * 0.25);
    let target = LogicalPosition::new(origin.x + width / 2.0, origin.y + height * 0.8);
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position: start });
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: start,
        button: PointerEventButton::Left,
    });
    for step in 1..=12 {
        let progress = step as f32 / 12.0;
        let position = LogicalPosition::new(
            start.x + (target.x - start.x) * progress,
            start.y + (target.y - start.y) * progress,
        );
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
        window
            .window()
            .dispatch_event(WindowEvent::PointerMoved { position });
    }
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: target,
            button: PointerEventButton::Left,
        });
    assert!(
        window.get_selection_begun(),
        "selection must receive pointer down (origin={origin:?}, width={width}, height={height}, scroll={})",
        window.get_scroll_y()
    );
    assert!(
        window.get_selection_moves() > 0,
        "passive viewport must not steal pointer drags from code selection"
    );
    assert_close(window.get_scroll_y(), 0.0);

    let wheel_position = LogicalPosition::new(origin.x + width / 2.0, origin.y + height / 2.0);
    for delta_y in [-240.0, 240.0] {
        window
            .window()
            .dispatch_event(WindowEvent::PointerScrolled {
                position: wheel_position,
                delta_x: 0.0,
                delta_y,
            });
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(120));
        if window.get_scroll_y().abs() > 0.5 {
            break;
        }
    }
    assert!(
        window.get_scroll_y() < -0.5,
        "wheel input must keep scrolling the passive viewport"
    );
}

#[test]
fn detached_preview_coexists_with_ai_review_and_preserves_its_slot() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(1300.0);
    window.set_ai_review_docked_content_unmount(true);
    let conversation = one_message_conversation();
    window.set_ai_review_conversation(conversation.clone());
    let trailing_width = window.get_pr_review_trailing_surface_width();
    let main_width = window.get_pr_review_main_content_width();

    AppState::get(&window).set_artifact_preview_presentation_mode(1);
    window.set_artifact_preview_pr_side_context(true);
    window.set_artifact_preview_visible(true);

    assert!(!window.get_artifact_preview_pr_side_docked_visible());
    assert!(window.get_ai_review_content_visible());
    assert!(window.get_ai_review_response_delegates_mounted());
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubAgentResponseBlock::body-text")
            .next()
            .is_some(),
        "Detached Preview is a negative control and must not unmount AI delegates"
    );
    assert_close(
        window.get_pr_review_trailing_surface_width(),
        trailing_width,
    );
    assert_close(window.get_pr_review_main_content_width(), main_width);
    assert_eq!(window.get_ai_review_conversation(), conversation);
}

#[test]
fn pr_trailing_surface_is_bounded_at_narrow_and_responsive_at_wide_widths() {
    i_slint_backend_testing::init_no_event_loop();
    let window = configure_pr_review_window(800.0);

    assert_close(window.get_pr_review_trailing_surface_width(), 360.0);
    assert_close(window.get_pr_review_main_content_width(), 440.0);
    assert!(window.get_ai_review_content_visible());

    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1300.0, 860.0),
    });

    assert_close(window.get_pr_review_trailing_surface_width(), 442.0);
    assert_close(window.get_pr_review_main_content_width(), 858.0);
    assert!(window.get_ai_review_content_visible());
}
