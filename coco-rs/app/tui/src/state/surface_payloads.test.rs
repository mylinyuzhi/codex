use super::PlanApprovalPromptState;

#[test]
fn plan_approval_toggles_between_approve_and_deny() {
    let mut o = PlanApprovalPromptState::new(
        "req-1".into(),
        "alice".into(),
        None,
        "# Plan\n- step 1\n- step 2".into(),
    );
    assert!(o.is_approve_focused(), "initial focus should be Approve");
    o.toggle_focus();
    assert!(!o.is_approve_focused());
    o.toggle_focus();
    assert!(o.is_approve_focused());
}

#[test]
fn plan_approval_prompt_gets_awaiting_input_priority() {
    let prompt = crate::state::PanePromptState::PlanApproval(PlanApprovalPromptState::new(
        "req".into(),
        "alice".into(),
        None,
        "".into(),
    ));
    // Priority 2 — same as Question / Elicitation / McpServerApproval.
    // Plan approval blocks the teammate, so it can't be out-prioritized
    // by user-triggered pickers (priority 7+).
    assert_eq!(prompt.priority(), 2);
}

#[test]
fn plan_approval_preserves_from_field_for_response_routing() {
    // The teammate agent name carried in `from` must survive so the
    // UserCommand::PlanApprovalResponse handler in tui_runner knows
    // which inbox to write the response to.
    let o = PlanApprovalPromptState::new(
        "req-42".into(),
        "teammate-delta".into(),
        Some("/plans/delta.md".into()),
        "plan".into(),
    );
    assert_eq!(o.from, "teammate-delta");
    assert_eq!(o.request_id, "req-42");
    assert_eq!(o.plan_file_path.as_deref(), Some("/plans/delta.md"));
}

// === AskUserQuestion footer feedback synthesizers ===
// Pin the exact TS prose at
// `claude-code/src/components/permissions/AskUserQuestionPermissionRequest/AskUserQuestionPermissionRequest.tsx:300-378`.
// Drift = transcript-interchangeability break.

mod question_feedback {
    use super::super::OTHER_OPTION_DISPLAY;
    use super::super::OTHER_OPTION_LABEL;
    use super::super::QuestionFocus;
    use super::super::QuestionItem;
    use super::super::QuestionOption;
    use super::super::QuestionPromptState;

    fn opt(label: &str) -> QuestionOption {
        QuestionOption {
            label: label.into(),
            description: String::new(),
            preview: None,
        }
    }

    fn q(text: &str, selected: i32, options: Vec<QuestionOption>) -> QuestionItem {
        QuestionItem {
            header: "h".into(),
            question: text.into(),
            options,
            multi_select: false,
            selected,
            checked: Vec::new(),
            notes: String::new(),
            editing_notes: false,
        }
    }

    fn state(questions: Vec<QuestionItem>, plan_mode: bool) -> QuestionPromptState {
        QuestionPromptState {
            request_id: "rid".into(),
            original_input: serde_json::json!({}),
            questions,
            focus: QuestionFocus::Question(0),
            is_in_plan_mode: plan_mode,
        }
    }

    #[test]
    fn chat_about_this_matches_ts_with_partial_answers() {
        let o = state(
            vec![
                q("Which library?", 0, vec![opt("Tokio"), opt("Async-std")]),
                q(
                    "Custom name?",
                    1,
                    vec![opt("Default"), opt(OTHER_OPTION_LABEL)],
                ),
            ],
            false,
        );

        let actual = o.chat_about_this_feedback();
        let expected = "\
The user wants to clarify these questions.\n    \
This means they may have additional information, context or questions for you.\n    \
Take their response into account and then reformulate the questions if appropriate.\n    \
Start by asking them what they would like to clarify.\n\n    \
Questions asked:\n\
- \"Which library?\"\n  Answer: Tokio\n\
- \"Custom name?\"\n  (No answer provided)";

        pretty_assertions::assert_eq!(actual, expected);
    }

    #[test]
    fn skip_interview_matches_ts() {
        let o = state(
            vec![q("Approach?", 0, vec![opt("Refactor"), opt("Rewrite")])],
            true,
        );

        let actual = o.skip_interview_feedback();
        let expected = "\
The user has indicated they have provided enough answers for the plan interview.\n\
Stop asking clarifying questions and proceed to finish the plan with the information you have.\n\n\
Questions asked and answers provided:\n\
- \"Approach?\"\n  Answer: Refactor";

        pretty_assertions::assert_eq!(actual, expected);
    }

    #[test]
    fn other_option_with_notes_uses_typed_text_as_answer() {
        let mut o = state(
            vec![q(
                "Pick:",
                1, // focus on the OTHER sentinel
                vec![opt("Tokio"), opt(OTHER_OPTION_LABEL)],
            )],
            false,
        );
        o.questions[0].notes = "  rayon  ".into();

        let actual = o.chat_about_this_feedback();
        assert!(
            actual.contains("Answer: rayon"),
            "Other-with-notes must trim and use typed text; got: {actual}"
        );
        assert!(
            !actual.contains(OTHER_OPTION_LABEL),
            "must NOT leak the __other__ sentinel; got: {actual}"
        );
    }

    #[test]
    fn multi_select_joins_checked_labels_with_comma_space() {
        let mut item = q(
            "Pick many:",
            0,
            vec![opt("A"), opt("B"), opt("C"), opt(OTHER_OPTION_LABEL)],
        );
        item.multi_select = true;
        item.checked = vec![0, 2];
        let o = state(vec![item], false);

        let actual = o.chat_about_this_feedback();
        assert!(actual.contains("Answer: A, C"), "got: {actual}");
    }

    #[test]
    fn no_answer_when_other_focused_with_no_notes() {
        let o = state(
            vec![q("Q?", 0, vec![opt(OTHER_OPTION_LABEL), opt("Skip")])],
            false,
        );
        let actual = o.chat_about_this_feedback();
        assert!(actual.contains("(No answer provided)"), "got: {actual}");
    }

    #[test]
    fn other_option_display_label_differs_from_sentinel() {
        // Sentinel is the data-layer marker; display is what the
        // renderer paints. peek_answer_for keys on the sentinel.
        assert_eq!(OTHER_OPTION_LABEL, "__other__");
        assert_eq!(OTHER_OPTION_DISPLAY, "Other");
    }
}

mod skills_dialog_from_wire {
    use crate::state::SkillsDialogSource;
    use crate::state::SkillsDialogState;

    fn entry(
        name: &str,
        source: coco_types::SkillsDialogSource,
        tokens: i64,
    ) -> coco_types::SkillsDialogEntry {
        coco_types::SkillsDialogEntry {
            name: name.to_string(),
            source,
            plugin_name: None,
            token_estimate: tokens,
        }
    }

    #[test]
    fn from_wire_groups_in_render_order_and_sorts_each_group() {
        // Two project + one user + one MCP, intentionally out of order
        // in the wire payload so the grouping + sort steps are visible.
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![
                entry("zeta", coco_types::SkillsDialogSource::User, 10),
                entry("alpha", coco_types::SkillsDialogSource::Project, 20),
                entry("acme:resource", coco_types::SkillsDialogSource::Mcp, 5),
                entry("beta", coco_types::SkillsDialogSource::Project, 30),
            ],
            group_subtitles: vec![
                coco_types::SkillsDialogGroupSubtitle {
                    source: coco_types::SkillsDialogSource::Project,
                    subtitle: "/proj/.claude/skills".to_string(),
                },
                coco_types::SkillsDialogGroupSubtitle {
                    source: coco_types::SkillsDialogSource::Mcp,
                    subtitle: "acme".to_string(),
                },
            ],
        };

        let state = SkillsDialogState::from_wire(payload);

        // Render order: Project before User before Mcp; Policy/Plugin
        // missing → dropped.
        let sources: Vec<_> = state.groups.iter().map(|g| g.source).collect();
        assert_eq!(
            sources,
            vec![
                SkillsDialogSource::Project,
                SkillsDialogSource::User,
                SkillsDialogSource::Mcp,
            ]
        );

        // Project group: alphabetical sort within group.
        let project = &state.groups[0];
        assert_eq!(project.subtitle, "/proj/.claude/skills");
        let project_names: Vec<_> = project.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(project_names, vec!["alpha", "beta"]);

        // User group has no subtitle in payload → empty string.
        assert_eq!(state.groups[1].subtitle, "");

        // Total across groups matches input count.
        assert_eq!(state.total(), 4);
    }

    #[test]
    fn from_wire_drops_empty_groups_silently() {
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![entry("a", coco_types::SkillsDialogSource::User, 1)],
            group_subtitles: vec![],
        };
        let state = SkillsDialogState::from_wire(payload);
        assert_eq!(state.groups.len(), 1);
        assert_eq!(state.groups[0].source, SkillsDialogSource::User);
    }
}
