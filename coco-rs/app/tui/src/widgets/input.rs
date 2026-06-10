//! Input composer widget.
//!
//! Owns the prompt indicator, placeholder, prefix stripping, mode title, and
//! streaming/queued-input presentation. Cursor placement reuses
//! [`InputRenderModel`] so the rendered text and cursor math stay aligned.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::state::ui::InputState;
use crate::state::ui::PromptMode;
use coco_tui_ui::style::UiStyles;

/// Pure input presentation data shared by the input widget and cursor logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputRenderModel {
    pub(crate) prompt_mode: PromptMode,
    pub(crate) prefix_consumed: usize,
    pub(crate) display_text: String,
    pub(crate) inline_hint: Option<String>,
    pub(crate) inline_ghost: Option<InlineGhostRender>,
    pub(crate) title: String,
    pub(crate) command_palette_filter: Option<String>,
    pub(crate) is_placeholder: bool,
    pub(crate) is_streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineGhostRender {
    pub(crate) byte_pos: usize,
    pub(crate) text: String,
}

impl InputRenderModel {
    pub(crate) fn build(
        input: &InputState,
        is_streaming: bool,
        prompt_suggestion: Option<&str>,
        has_editable_queue: bool,
        command_palette_filter: Option<&str>,
    ) -> Self {
        let prompt_mode = if is_streaming {
            PromptMode::Normal
        } else {
            input.prompt_mode()
        };
        let is_empty = input.is_empty();

        // Match submit-time prefix stripping: drop the leading mode char plus
        // one optional space so display text equals what the engine receives.
        let prefix_consumed = if is_empty || prompt_mode == PromptMode::Normal {
            0
        } else {
            let body = &input.text()[1..];
            1 + if body.starts_with(' ') { 1 } else { 0 }
        };

        let (display_text, inline_hint, is_placeholder, command_palette_filter) =
            if let Some(filter) = command_palette_filter {
                (format!("/{filter}"), None, false, Some(filter.to_string()))
            } else if is_empty {
                if has_editable_queue {
                    (t!("input.placeholder_queued").to_string(), None, true, None)
                } else if let Some(suggestion) = prompt_suggestion {
                    (suggestion.to_string(), None, true, None)
                } else {
                    (String::new(), None, false, None)
                }
            } else {
                (
                    input.text()[prefix_consumed..].to_string(),
                    input.inline_hint.clone(),
                    false,
                    None,
                )
            };
        let inline_ghost = if is_placeholder || command_palette_filter.is_some() {
            None
        } else {
            input.active_inline_ghost().and_then(|ghost| {
                let byte_pos = ghost.insert_position.checked_sub(prefix_consumed)?;
                (byte_pos <= display_text.len()).then(|| InlineGhostRender {
                    byte_pos,
                    text: ghost.text.clone(),
                })
            })
        };

        let title = if is_streaming {
            format!(" {} ", t!("input.title_queue"))
        } else if prompt_mode != PromptMode::Normal {
            format!(" {} ", t!(prompt_mode.title_i18n_key()))
        } else {
            String::new()
        };

        Self {
            prompt_mode,
            prefix_consumed,
            display_text,
            inline_hint,
            inline_ghost,
            title,
            command_palette_filter,
            is_placeholder,
            is_streaming,
        }
    }
}

/// Input widget with prompt-mode indicator and placeholder handling.
pub struct InputWidget<'a> {
    input: &'a InputState,
    styles: UiStyles<'a>,
    focused: bool,
    is_streaming: bool,
    prompt_suggestion: Option<&'a str>,
    has_editable_queue: bool,
    command_palette_filter: Option<&'a str>,
}

impl<'a> InputWidget<'a> {
    pub fn new(input: &'a InputState, styles: UiStyles<'a>) -> Self {
        Self {
            input,
            styles,
            focused: true,
            is_streaming: false,
            prompt_suggestion: None,
            has_editable_queue: false,
            command_palette_filter: None,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn streaming(mut self, streaming: bool) -> Self {
        self.is_streaming = streaming;
        self
    }

    pub fn prompt_suggestion(mut self, suggestion: Option<&'a str>) -> Self {
        self.prompt_suggestion = suggestion;
        self
    }

    pub fn has_editable_queue(mut self, has_queue: bool) -> Self {
        self.has_editable_queue = has_queue;
        self
    }

    pub fn command_palette_filter(mut self, filter: Option<&'a str>) -> Self {
        self.command_palette_filter = filter;
        self
    }

    fn model(&self) -> InputRenderModel {
        InputRenderModel::build(
            self.input,
            self.is_streaming,
            self.prompt_suggestion,
            self.has_editable_queue,
            self.command_palette_filter,
        )
    }
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let model = self.model();
        let border_color = if self.focused {
            self.styles.focused_border()
        } else {
            self.styles.border()
        };

        let indicator = match (model.is_streaming, model.prompt_mode) {
            (true, _) => Span::styled("~ ", Style::default().fg(self.styles.warning())),
            (false, PromptMode::Bash) => {
                Span::styled("! ", Style::default().fg(self.styles.accent())).bold()
            }
            (false, PromptMode::Normal) => {
                Span::styled("❯ ", Style::default().fg(self.styles.primary()))
            }
        };
        let text_style = if model.is_placeholder {
            Style::default().fg(self.styles.dim())
        } else {
            Style::default().fg(self.styles.text())
        };
        let mut spans = vec![indicator];
        if let Some(ghost) = model.inline_ghost.as_ref() {
            let split = ghost.byte_pos.min(model.display_text.len());
            let before = model.display_text[..split].to_string();
            let after = model.display_text[split..].to_string();
            spans.push(Span::styled(before, text_style));
            spans.push(Span::styled(
                ghost.text.clone(),
                Style::default().fg(self.styles.dim()),
            ));
            spans.push(Span::styled(after, text_style));
        } else {
            spans.push(Span::styled(model.display_text.clone(), text_style));
        }
        if let Some(hint) = model.inline_hint.as_ref() {
            spans.push(Span::styled(
                hint.clone(),
                Style::default().fg(self.styles.dim()),
            ));
        }
        let input_line = Line::from(spans);
        let input = Paragraph::new(input_line).block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .title(model.title)
                .border_style(Style::default().fg(border_color)),
        );

        input.render(area, buf);
    }
}

#[cfg(test)]
#[path = "input.test.rs"]
mod tests;
