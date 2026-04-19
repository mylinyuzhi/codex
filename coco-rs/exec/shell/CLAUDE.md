# coco-shell

Shell execution with security analysis, destructive-command warnings, sandbox decisions, read-only / mode validation, heredoc + sed-in-place handling, CWD tracking. Builds on `coco-shell-parser` (tree-sitter + tokenizer) for the AST and 23 security check IDs; layers TS-ported rules on top.

## TS Source

- `utils/bash/` — parser + validators (~12K LOC)
- `utils/Shell.ts` — shell execution + CWD tracking
- `utils/shell/` — shell helper utilities
- `tools/BashTool/bashPermissions.ts`, `bashSecurity.ts`, `readOnlyValidation.ts`, `commandSemantics.ts`, `destructiveCommandWarning.ts`, `shouldUseSandbox.ts`, `modeValidation.ts`, `sedEditParser.ts`
- `utils/powershell/` — PowerShell parsing

Paths relative to `/lyz/codespace/3rd/claude-code/src/`.

## Key Types

- **Executor**: `ShellExecutor`, `ShellProgress`, `CommandResult`, `ExecOptions`
- **AST**: `BashNode`, `SimpleCommand`, `parse_command`, `extract_simple_commands`
- **Bash permissions**: `get_command_prefix`, `is_dangerous_bare_prefix`, `split_compound_command`, `strip_all_env_vars`, `strip_safe_wrappers`
- **Security**: `SecurityCheck`, `SecurityCheckId` (23 IDs), `SecuritySeverity`, `SafetyResult`, `check_security`
- **Destructive / read-only / mode**: destructive warnings, read-only rule matcher, `is_auto_allowed_in_accept_edits`, path validation
- **Heredoc**: `HeredocContent`, `extract_heredocs`
- **Sed in-place**: `SedEditInfo`, `parse_sed_edit_command`, `is_sed_in_place_edit`
- **Semantics**: `CommandResultInterpretation`
- **Sandbox decision**: routes through `coco-sandbox::SandboxSettings`
- **Snapshot**: shell environment snapshot support

## Layering

Parser + 24 security analyzers live in `coco-shell-parser` (infrastructure). This crate layers the TS-sourced rules on top (read-only validation, destructive warnings, mode validation, sed edit detection, accept-edits auto-allow, sandbox decision).
