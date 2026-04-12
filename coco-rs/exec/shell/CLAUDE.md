# coco-shell

Shell execution, bash security (23 check IDs), destructive warnings, sandbox decisions, mode validation.

HYBRID: cocode-rs `utils/shell-parser` (24 analyzers) as base + TS enhancements.

## TS Source
- `src/utils/bash/` (12K LOC -- parser, validators)
- `src/utils/Shell.ts` (shell execution, CWD tracking)
- `src/utils/shell/` (shell utilities)
- `src/tools/BashTool/` (bashPermissions.ts, bashSecurity.ts, readOnlyValidation.ts, commandSemantics.ts, destructiveCommandWarning.ts, shouldUseSandbox.ts, modeValidation.ts, sedEditParser.ts)
- `src/utils/powershell/` (PowerShell parsing)

## Key Types
ShellExecutor, CommandResult, SafetyResult, SecurityCheckId (23 IDs)
