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
    use crate::state::SkillOverrideState;
    use crate::state::SkillsDialogState;

    fn entry(
        name: &str,
        source: coco_types::SkillsDialogSource,
        bytes: i64,
    ) -> coco_types::SkillsDialogEntry {
        coco_types::SkillsDialogEntry {
            name: name.to_string(),
            source,
            description: String::new(),
            plugin_name: None,
            frontmatter_bytes: bytes,
            current_local: None,
            baseline: coco_types::SkillOverrideState::On,
            lock: None,
        }
    }

    #[test]
    fn from_wire_populates_flat_rows_with_pending_initialised_from_local_or_baseline() {
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![
                entry("zeta", coco_types::SkillsDialogSource::User, 40),
                entry("alpha", coco_types::SkillsDialogSource::Project, 80),
                entry("acme:resource", coco_types::SkillsDialogSource::Mcp, 20),
            ],
            bytes_per_token: 4,
        };
        let state = SkillsDialogState::from_wire(payload);
        assert_eq!(state.total(), 3);
        // pending == baseline (`On`) when no lock + no local override.
        assert!(
            state
                .rows
                .iter()
                .all(|r| r.pending == SkillOverrideState::On)
        );
        // bytes_per_token preserved verbatim.
        assert_eq!(state.bytes_per_token, 4);
    }

    #[test]
    fn from_wire_falls_back_to_four_bytes_per_token_when_zero() {
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![entry("foo", coco_types::SkillsDialogSource::User, 0)],
            bytes_per_token: 0,
        };
        let state = SkillsDialogState::from_wire(payload);
        // PR3 fallback — 4 bytes/token English-text heuristic.
        assert_eq!(state.bytes_per_token, 4);
    }

    #[test]
    fn filtered_view_sorts_by_source_then_name_when_default() {
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![
                entry("zeta", coco_types::SkillsDialogSource::User, 1),
                entry("alpha", coco_types::SkillsDialogSource::User, 1),
                entry("foo", coco_types::SkillsDialogSource::BuiltIn, 1),
            ],
            bytes_per_token: 4,
        };
        let state = SkillsDialogState::from_wire(payload);
        let view = state.filtered_view();
        let order: Vec<&str> = view.iter().map(|i| state.rows[*i].name.as_str()).collect();
        // "built-in" < "user" lexicographically; "alpha" < "zeta".
        assert_eq!(order, vec!["foo", "alpha", "zeta"]);
    }

    #[test]
    fn filtered_view_sorts_by_token_desc_when_sort_toggled() {
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![
                entry("small", coco_types::SkillsDialogSource::User, 10),
                entry("big", coco_types::SkillsDialogSource::User, 1000),
                entry("mid", coco_types::SkillsDialogSource::Project, 100),
            ],
            bytes_per_token: 4,
        };
        let mut state = SkillsDialogState::from_wire(payload);
        state.toggle_sort();
        let view = state.filtered_view();
        let names: Vec<&str> = view.iter().map(|i| state.rows[*i].name.as_str()).collect();
        assert_eq!(names, vec!["big", "mid", "small"]);
    }

    #[test]
    fn filter_query_matches_name_description_and_source_label_lowercased() {
        let mut e1 = entry("deploy-rs", coco_types::SkillsDialogSource::User, 10);
        e1.description = "Run cargo deploy".to_string();
        let mut e2 = entry("review", coco_types::SkillsDialogSource::Project, 20);
        e2.description = "Code review helper".to_string();
        let mut e3 = entry("noise", coco_types::SkillsDialogSource::Plugin, 5);
        e3.description = "Unrelated".to_string();
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![e1, e2, e3],
            bytes_per_token: 4,
        };
        let mut state = SkillsDialogState::from_wire(payload);
        // Filter by name.
        state.filter_query = "deploy".into();
        let names_by_filter: Vec<&str> = state
            .filtered_view()
            .iter()
            .map(|i| state.rows[*i].name.as_str())
            .collect();
        assert_eq!(names_by_filter, vec!["deploy-rs"]);

        // Filter by source label.
        state.filter_query = "plugin".into();
        let by_src: Vec<&str> = state
            .filtered_view()
            .iter()
            .map(|i| state.rows[*i].name.as_str())
            .collect();
        assert_eq!(by_src, vec!["noise"]);

        // Filter by description.
        state.filter_query = "review".into();
        let by_desc: Vec<&str> = state
            .filtered_view()
            .iter()
            .map(|i| state.rows[*i].name.as_str())
            .collect();
        assert_eq!(by_desc, vec!["review"]);
    }

    #[test]
    fn cycle_focused_advances_pending_unless_locked() {
        let mut locked_row = entry("locked", coco_types::SkillsDialogSource::User, 10);
        locked_row.lock = Some(coco_types::SkillLock {
            source: coco_types::SkillLockSource::Policy,
            forced_value: coco_types::SkillOverrideState::Off,
        });
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![
                entry("free", coco_types::SkillsDialogSource::User, 10),
                locked_row,
            ],
            bytes_per_token: 4,
        };
        let mut state = SkillsDialogState::from_wire(payload);

        // Default sort: alphabetical → ["free", "locked"]. selected
        // starts at 0 (free).
        state.cycle_focused();
        let view = state.filtered_view();
        // free's pending advanced from On → NameOnly.
        let free_row = view
            .iter()
            .copied()
            .find(|i| state.rows[*i].name == "free")
            .map(|i| &state.rows[i])
            .unwrap();
        assert_eq!(free_row.pending, SkillOverrideState::NameOnly);

        // Move to the locked row and try to cycle.
        state.move_down();
        let pending_before = state
            .rows
            .iter()
            .find(|r| r.name == "locked")
            .map(|r| r.pending)
            .unwrap();
        state.cycle_focused();
        let pending_after = state
            .rows
            .iter()
            .find(|r| r.name == "locked")
            .map(|r| r.pending)
            .unwrap();
        assert_eq!(pending_before, pending_after, "lock must no-op on Space");
    }

    #[test]
    fn compute_save_diff_emits_value_when_diverged_and_null_when_returning_to_baseline() {
        let payload = coco_types::SkillsDialogPayload {
            entries: vec![entry("foo", coco_types::SkillsDialogSource::User, 1)],
            bytes_per_token: 4,
        };
        let mut state = SkillsDialogState::from_wire(payload);
        // Baseline = On, pending = Off → write "off".
        state.rows[0].pending = SkillOverrideState::Off;
        let diff = state.compute_save_diff();
        assert_eq!(diff.total_edits, 1);
        assert_eq!(diff.diff.get("foo"), Some(&Some(SkillOverrideState::Off)));

        // Now local = Off, pending revert to On (== baseline) → write null.
        state.rows[0].current_local = Some(SkillOverrideState::Off);
        state.rows[0].pending = SkillOverrideState::On;
        let diff = state.compute_save_diff();
        assert_eq!(diff.diff.get("foo"), Some(&None));
        // Effective changed (was Off, now On) → 1 edit.
        assert_eq!(diff.total_edits, 1);
    }
}
