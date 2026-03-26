## Command sandbox

By default, your command will be run in a sandbox. This sandbox controls which directories and network hosts commands may access or modify without an explicit override.

The sandbox has the following restrictions:
{restrictions}

{mode_block}
- For temporary files, always use the `$TMPDIR` environment variable (or `/tmp/cocode` as a fallback). TMPDIR is automatically set to the correct sandbox-writable directory in sandbox mode. Do NOT use `/tmp` directly - use `$TMPDIR` or `/tmp/cocode` instead.
