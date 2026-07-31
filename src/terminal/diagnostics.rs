use super::render::RenderedTerminalLine;
use crate::Route;

pub(super) fn terminal_line_stats(lines: &[RenderedTerminalLine]) -> (usize, usize) {
    let mut span_count = 0usize;
    let mut span_text_bytes = 0usize;

    for line in lines {
        span_count += line.spans.len();
        for span in &line.spans {
            span_text_bytes += span.text.len();
        }
    }

    (span_count, span_text_bytes)
}

pub(super) fn sanitize_for_log(text: &str) -> String {
    text.chars()
        .flat_map(|ch| ch.escape_default())
        .collect::<String>()
}

pub(super) fn format_bytes_for_log(bytes: &[u8]) -> String {
    const MAX_PREVIEW: usize = 24;
    let mut parts = bytes
        .iter()
        .take(MAX_PREVIEW)
        .map(|byte| format!("{byte:#04x}"))
        .collect::<Vec<_>>();
    if bytes.len() > MAX_PREVIEW {
        parts.push("...".to_string());
    }
    format!("[{}]", parts.join(", "))
}

pub(super) fn route_label(route: Route) -> &'static str {
    match route {
        Route::Home => "Home",
        Route::Search => "Search",
        Route::Settings => "Settings",
        Route::Project => "Project",
        Route::Chat => "Chat",
        Route::UiComponents => "UI Components",
        Route::About => "About",
        Route::Workflow => "Workflow",
        Route::JobRun => "JobRun",
        Route::PrReview => "PrReview",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bounded_byte_previews() {
        let bytes = (0u8..30).collect::<Vec<_>>();
        let formatted = format_bytes_for_log(&bytes);
        assert!(formatted.contains("0x00"));
        assert!(formatted.ends_with("...]"));
    }

    #[test]
    fn reports_empty_render_stats_and_stable_route_labels() {
        assert_eq!(
            terminal_line_stats(&[RenderedTerminalLine::default()]),
            (0, 0)
        );
        assert_eq!(route_label(Route::PrReview), "PrReview");
        assert_eq!(sanitize_for_log("a\nb"), "a\\nb");
    }
}
