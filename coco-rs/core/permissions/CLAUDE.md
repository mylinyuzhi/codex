# coco-permissions

Permission evaluation pipeline: auto-mode / yolo classifier (2-stage XML via LLM), denial tracking, rule compilation, shell rule matching, dangerous-pattern detection.

## TS Source
- `utils/permissions/permissions.ts`, `utils/permissions/permissionsLoader.ts` — eval pipeline
- `utils/permissions/PermissionMode.ts`, `utils/permissions/PermissionResult.ts`, `utils/permissions/PermissionRule.ts`, `utils/permissions/PermissionUpdate.ts`, `utils/permissions/PermissionUpdateSchema.ts`, `utils/permissions/PermissionPromptToolResultSchema.ts` — types
- `utils/permissions/yoloClassifier.ts`, `utils/permissions/bashClassifier.ts`, `utils/permissions/classifierShared.ts`, `utils/permissions/classifierDecision.ts` — 2-stage XML classifier
- `utils/permissions/autoModeState.ts` — auto-mode state machine
- `utils/permissions/denialTracking.ts` — denial ring buffer
- `utils/permissions/permissionRuleParser.ts`, `utils/permissions/shellRuleMatching.ts`, `utils/permissions/shadowedRuleDetection.ts` — rule compiler
- `utils/permissions/filesystem.ts`, `utils/permissions/pathValidation.ts`, `utils/permissions/dangerousPatterns.ts` — filesystem safety
- `utils/permissions/getNextPermissionMode.ts` — mode transitions
- `utils/permissions/permissionExplainer.ts` — LLM-generated explanations
- `utils/permissions/permissionSetup.ts` — initial setup / validation
- `utils/permissions/bypassPermissionsKillswitch.ts` — bypass capability + killswitch
- `utils/classifierApprovals.ts`, `utils/classifierApprovalsHook.ts`, `utils/autoModeDenials.ts` — auto-mode denial cache

## Key Types

- **Auto mode**: `AutoModeInput`, `AutoModeDecision`, `AutoModeState`, `AutoModeRules`, `ClassifyRequest`, `YoloClassifierResult`, `classify_for_auto_mode`, `classify_auto_mode_extended`, `classify_yolo_action`, `is_safe_tool`, `can_use_tool_in_auto_mode`
- **Evaluation**: `PermissionEvaluator`, `ToolCheckFn`, `ToolCheckResult`, `get_all_rules`, `get_content_rules_for_tool`, `get_tool_wide_rule`
- **Rule compiler**: `compile_rules`, `evaluate_rules_for_tool`, `parse_rule_string`, `rule_value_to_string`, `RuleMatchResult`
- **Mode transitions**: `get_next_permission_mode`, `resolve_predefined_mode`, `resolve_subagent_mode`, `transition_context_with_auto`, `apply_auto_transition_to_app_state`
- **Filesystem safety**: `PathSafetyResult`, `check_path_safety_for_auto_edit`, `contains_path_traversal`, `get_paths_for_permission_check`, `has_dangerous_tilde`, `has_shell_expansion`, `is_dangerous_file_path`, `is_dangerous_removal_path`, `is_editable_internal_path`, `is_readable_internal_path`, `path_in_working_path`
- **Stores**: `PermissionStore`, `PermissionRulesByBehavior`, `SettingsPermissionStore`
- **Updates**: `apply_permission_update`, `apply_permission_updates`
- **Setup**: `PermissionConfigError`, `PermissionModeChoice`, `get_default_rules_for_mode`, `is_dangerous_powershell_permission`, `validate_permission_configuration`
- **Explainer**: `ExplainerParams`, `build_explainer_query`, `explainer_tool_def`, `generate_permission_explanation`
- **Shadowed rules**: `DetectUnreachableRulesOptions`, `ShadowType`, `UnreachableRule`, `detect_unreachable_rules`
- **Bypass / killswitch**: `InitialPermissionMode`, `KillswitchCheck`, `check_bypass_killswitch_transition`, `compute_bypass_capability`, `resolve_initial_permission_mode`
- **Denial tracking**: `DenialTracker`
- **Dangerous rules**: `restore_dangerous_rules`, `strip_dangerous_rules`
- **Shell rules**: `ShellPermissionRule`

## Priority (more-specific wins)

```
session > command > cliArg > flagSettings > localSettings > projectSettings > userSettings > policySettings
```
Deny always wins immediately (step 1 of eval pipeline), regardless of priority.
