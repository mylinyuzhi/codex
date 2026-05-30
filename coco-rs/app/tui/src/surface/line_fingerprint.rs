use ratatui::layout::Alignment;
use ratatui::style::Style;
use ratatui::text::Line;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedLineFingerprint {
    style: Style,
    alignment: Option<Alignment>,
    spans: Vec<RenderedSpanFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedSpanFingerprint {
    content: String,
    style: Style,
}

pub(crate) fn fingerprint_lines(lines: &[Line<'_>]) -> Vec<RenderedLineFingerprint> {
    lines.iter().map(fingerprint_line).collect()
}

fn fingerprint_line(line: &Line<'_>) -> RenderedLineFingerprint {
    RenderedLineFingerprint {
        style: line.style,
        alignment: line.alignment,
        spans: line
            .spans
            .iter()
            .map(|span| RenderedSpanFingerprint {
                content: span.content.to_string(),
                style: span.style,
            })
            .collect(),
    }
}
