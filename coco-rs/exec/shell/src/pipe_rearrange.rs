//! Pipe-aware command rearrangement for safe `eval` wrapping.
//!
//! TS source: `utils/bash/bashPipeCommand.ts` (`rearrangePipeCommand`).
//!
//! Why: `eval 'rg foo | wc -l' < /dev/null` looks innocent but causes `rg`
//! to **hang forever**. Without arguments, `rg` reads stdin; the shell
//! attaches `eval`'s stdin (which is `/dev/null`) to the *last* command in
//! the pipeline, not the first. `rg` (first command) inherits the parent
//! shell's stdin from the spawn — an open pipe with no writer — and blocks.
//! See TS file's docstring + anthropics/claude-code issues #9189 / #9732.
//!
//! TS solution: parse the command into shell tokens, find the first `|`,
//! insert `< /dev/null` between the first command and the pipe so the
//! redirect applies to the right process.
//!
//! Coco-rs solution: we do not link a shell-quote parser. The TS code already
//! falls back to `singleQuoteForEval(cmd) + ' < /dev/null'` for every
//! command it can't safely parse (backticks, `$()`, `$VAR`, control
//! structures, malformed quotes, …). That fallback is **always correct** —
//! placing `< /dev/null` outside the `eval` causes eval's stdin to be
//! `/dev/null`, which the first child in the pipeline inherits. The only
//! reason TS prefers the "smart" path is that it produces a slightly less
//! redundant command string; functionally both paths yield the same runtime
//! behavior. We choose the fallback as the universal answer.

use crate::shell_quoting::quote_shell_command;
use crate::shell_quoting::should_add_stdin_redirect;

/// Rewrite a command so its stdin redirect (if any) targets the first
/// process in a pipeline rather than `eval` itself.
///
/// Returns a string that can be appended directly after `eval `.
pub fn rearrange_pipe_command(command: &str) -> String {
    let add_redirect = should_add_stdin_redirect(command);
    quote_shell_command(command, add_redirect)
}

#[cfg(test)]
#[path = "pipe_rearrange.test.rs"]
mod tests;
