# coco-output-styles

Output style catalog, dir/plugin loading, and active-style resolution.
TS-mirror for `outputStyles/`, `constants/outputStyles.ts`, and
`utils/plugins/loadPluginOutputStyles.ts`.

## TS Source

- `constants/outputStyles.ts` — `OutputStyleConfig`, `OUTPUT_STYLE_CONFIG`
  (built-ins: `default` / `Explanatory` / `Learning`),
  `getAllOutputStyles`, `getOutputStyleConfig`, `hasCustomOutputStyle`.
- `outputStyles/loadOutputStylesDir.ts` — project + user
  `.claude/output-styles/*.md` markdown discovery, frontmatter parsing
  (`name`, `description`, `keep-coding-instructions`).
- `utils/plugins/loadPluginOutputStyles.ts` — plugin output-styles
  loader; namespace prefix `pluginName:baseName`; parses
  `force-for-plugin`.
- `constants/prompts.ts` — system-prompt injection
  (`# Output Style: <name>\n<prompt>` block + intro-toggle +
  `keepCodingInstructions` gate).
- `utils/messages.ts:3797` + `utils/attachments.ts:1597` — per-turn
  reminder template + main-thread, non-default gate (handled by
  `core/system-reminder/generators/output_style.rs`, not here).
- `commands/output-style/output-style.tsx` — deprecated CLI stub
  (handled by `commands/`).

## Key Types

| Type | Purpose |
|------|---------|
| `OutputStyleConfig` | TS `OutputStyleConfig` mirror — `name`, `description`, `prompt`, `source`, `keep_coding_instructions`, `force_for_plugin` |
| `OutputStyleSource` | `BuiltIn` / `Plugin` / `UserSettings` / `ProjectSettings` / `PolicySettings`. `priority()` drives override ordering |
| `Aggregated` | Name → config catalog produced by `aggregate(..)`. `default` sentinel intentionally absent — TS represents it as `null` |
| `OutputStyleManager` | Resolved catalog + active config. Built once at session bootstrap, cheap to clone, threaded into prompt builder + SDK init + reminder pipeline |
| `OutputStyleManagerBuilder` | Fluent builder used by the CLI — accepts settings name, user dir, project dirs, managed dir, plugin sources |
| `PluginOutputStyleSource` | Minimal data per plugin (`plugin_name`, `default_dir`, `extra_paths`) so this crate doesn't depend on plugin lifecycle types |
| `ForceForPluginVerdict` | `None` or `Selected { winner, competing }`. Lets the manager log multi-force conflicts |

## Module Layout

```
src/
├── lib.rs            re-exports + crate-local Result
├── error.rs          OutputStylesError (thiserror, Tier 2 boundary)
├── catalog.rs        OutputStyleConfig + OutputStyleSource
├── builtin.rs        DEFAULT/EXPLANATORY/LEARNING constants + builtin_styles()
├── dir_loader.rs     load_dir_styles + shared frontmatter extraction
├── plugin_loader.rs  load_plugin_output_styles + PluginOutputStyleSource
├── resolver.rs       aggregate() + resolve_active_style() + ForceForPluginVerdict
└── manager.rs        OutputStyleManager + builder
```

## Key Invariants

- **Built-ins ship verbatim.** The Explanatory and Learning prompt
  bodies are reproduced from TS character-for-character (Unicode `★`
  and `●` substituted for `figures.star` / `figures.bullet`). Built-in
  tests assert presence of every TS section header so a regression
  rewrites the body intentionally, not by accident.
- **`default` is `None`, not an entry.** TS stores
  `OUTPUT_STYLE_CONFIG[default] = null`; we make `default` absent from
  `Aggregated.by_name` and bake the `None` mapping into
  `Aggregated::get`. Callers that need the literal sentinel string
  (SDK init, picker default option) read `DEFAULT_OUTPUT_STYLE_NAME`.
- **TS aggregation order**: `built-in < plugin < user < project < managed`.
  Implemented numerically by `OutputStyleSource::priority()` with
  `>=` overwrite so a later layer with the same priority replaces an
  earlier one of the same priority.
- **Plugin styles can't carry `keep-coding-instructions`.** TS plugin
  loader doesn't read that field; we explicitly clear it after
  parsing. Plugin authors who need that escape hatch should ship a
  dir-style instead.
- **Force-for-plugin tie-break is alphabetical.** Multiple plugins all
  setting `force-for-plugin: true` is a misconfiguration; we log the
  full list and pick the alphabetically-first deterministically so
  bootstraps are reproducible across machines.

## What this crate does NOT own

- **Per-turn `<system-reminder>` injection** — that's
  `core/system-reminder/generators/output_style.rs`. It reads the
  active style name from `OutputStyleSnapshot` (set on
  `SessionBootstrap` by the CLI) and renders the TS template
  (`{name} output style is active. Remember to...`).
- **System prompt assembly** — the CLI passes the active
  `OutputStyleConfig` into `coco_context::build_system_prompt`, which
  injects `# Output Style: <name>\n<prompt>` after the identity
  block. This crate has no direct dependency on `coco_context` to
  preserve the `core/` → `root/` direction.
- **Settings reading.** Callers pass the resolved
  `Settings.output_style` value as a string. This crate doesn't
  depend on `coco_config`.
- **`/output-style` slash command.** The deprecated stub lives in
  `commands/src/implementations.rs` (`output_style_handler`).
