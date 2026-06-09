# Permission & Sandbox Hardening — Refactor Status & Handoff

Date started: 2026-06-09 · Branch: `feat/tui` · First commit: `1a0b37a32`

This is a **living handoff doc**: it carries the full context needed to resume
the permission/sandbox hardening refactor in a fresh session. It is a
fix-status / TODO doc, not an information owner — for crate internals see
[crate-coco-permissions.md](crate-coco-permissions.md) and
[crate-coco-sandbox.md](crate-coco-sandbox.md); historical audit rows live in
[audit-gaps.md](audit-gaps.md).

> **Resuming?** Read §1–§3 once (background + decisions + invariants), then go
> straight to **§5 TODO** (prioritized, internal-first, TUI last). Each TODO
> item has exact file/function anchors and the approach. Build/verify workflow
> is §6.

---

## 1. Background (重构背景)

A 12-dimension audit of coco-rs's `permission` + `sandbox` subsystems against
the `claude-code` TS reference (`/lyz/codespace/3rd/claude-code/src`) found one
dominant anti-pattern plus real decision bugs:

**"Ported-but-not-connected" (wired-but-dead).** Large amounts of correct logic
were ported from TS but **never invoked in production** (zero non-test callers).
Because the code *existed*, it looked done — but the behavior it was supposed to
produce never happened. The most severe instances:

- **Linux sandbox was 100% broken** — the bwrap inner stage re-execs
  `coco --apply-seccomp <mode> -- <cmd>`, but `main.rs` went straight to
  `Cli::parse()` with no arg0 intercept, so the inner process died on a clap
  "unexpected argument" error. Every sandboxed command on Linux failed.
- **The sandbox approval bridge was fully dead** — `check_path_async` /
  `check_network_async` / `set_approval_bridge` / `SdkSandboxApprovalBridge`
  had zero production callers; `SandboxApprovalRequired` was never emitted.
- **Two fail-open security holes** — a bash deny rule (`Bash(curl:*)`) was
  bypassable by compounding/wrapping/env-prefixing the command, and a dangerous
  classifier-bypass allow rule was never stripped on Auto-mode entry.

Plus fail-closed correctness bugs (central evaluator over-denying content-scoped
rules), unsafe read-only auto-approvals (`env`, `date -s`, `ps axe`, …), and
inert sandbox network filtering / violation reporting.

The work mirrors **observable TS behavior**, not a byte-for-byte port (Rust
idioms — typed enums, `Result`, snafu — are kept).

---

## 2. Audit & method

Process used (and the way to regenerate it): **audit → design → implement →
gate**, all via background workflows.

1. **Audit workflow** — 12 read-only dimension agents diffed coco vs TS, each
   adversarially verified; 14/14 verdicts confirmed. Output classified every
   gap by severity and `is_intentional_divergence`.
2. **Spec workflow** — 12 spec agents produced precise, TS-mirrored
   implementation specs (exact fns, algorithms, call-sites, tests). These specs
   are the source of truth for the remaining work.
3. **Implementation** — done **serially** (multiple groups touch the same hot
   files: `evaluate.rs`, `bash.rs`, `state.rs`, `session_runtime.rs`), verifying
   with `just quick-check` at each milestone.
4. **Gate** — `just pre-commit` (full nextest + clippy) before commit.

> **Spec files** were persisted under the session's workflow output. If gone,
> regenerate by re-running the audit + spec workflows (the audit prompt diffs
> each subsystem dimension vs TS; the spec prompt turns each gap into an exact
> change list). The gap IDs below (P1–P18, S1–S7) come from that audit.

---

## 3. Technical decisions & invariants (技术决策)

These are load-bearing — **do not "fix" or revert them** without re-reading the
rationale.

### 3.1 Intentional divergences (NOT bugs — don't flag/port)
Per the multi-provider port rules (see root `CLAUDE.md`):
- No `USER_TYPE='ant'` / GrowthBook gates (e.g. the auto-mode classifier is the
  ant-gated TS feature; coco keeps the seam but it's unreachable via the normal
  cycle — `auto_mode_available=false`).
- Anthropic cloud routes (Bedrock/Vertex/Foundry) are non-goals.
- Ultraplan paths skipped. Env vars renamed `CLAUDE_*` → `COCO_*`.
- coco's default `Tool::check_permissions` returns `Passthrough` (TS returns
  `allow`) — deliberate fail-secure choice.

### 3.2 "Force-ask that can't be overridden" = `ToolCheckResult::Ask` at step-1b
`PermissionEvaluator::evaluate_inner` consults `Tool::check_permissions` at
**step-1b** — after deny rules, but **before** allow rules (step 2), ask rules,
path-safety, and mode fallthrough. So an `Ask` returned from `check_permissions`
short-circuits with a `PermissionDecision::Ask` that **no allow rule and no
mode (incl. acceptEdits) can override** — exactly TS's "cannot be auto-allowed
by permission rules". Only `DontAsk` mode converts it to deny (TS parity). This
is how the bash path-safety gates (§4 [5]) are non-overridable.

### 3.3 Layering: `coco-permissions` now depends on `coco-shell` (Core→Exec)
Added for the bash rule decomposition (P2) — a **legal downward edge**
(Core depends on Exec; `core/tools` + `core/tool-runtime` already do). **`coco-shell`
must NOT depend on `coco-permissions`** (would cycle). Consequence: the bash
path-safety gates (P3/P4/P5/P15) live **in `coco-shell`** (pure helpers reusing
`coco-shell`'s own `check_dangerous_path`) and are orchestrated from
`core/tools/bash.rs` — NOT by having `coco-shell` call
`coco_permissions::is_dangerous_removal_path`.

### 3.4 Bash rule matching policy (P2)
`shell_rules::match_bash_rule(content, command, policy, case)`:
- `RuleMatchPolicy::DenyOrAsk` — strip ALL leading env vars (fixed point) +
  safe wrappers + redirections, then re-split per-subcommand (any segment
  match ⇒ match). A deny can't hide behind `FOO=1 …` / `timeout 5 …` / `a && …`.
- `RuleMatchPolicy::Allow` — compound guard: a prefix/wildcard allow rule never
  matches a compound command (can't widen an allow by chaining).
- `ShellCase::Insensitive` for PowerShell, `Sensitive` for Bash.

### 3.5 Auto-mode dangerous-rule strip (P1) — two mechanisms
- **(B) load-bearing security fix:** strip at **context-build** time —
  `tool_context.rs::build()` runs `strip_dangerous_allow_rules` when
  `live_mode == Auto`. Mandatory regardless of entry path.
- **(A) provenance:** Auto-entry stash in
  `apply_permission_mode_transition_to_app_state` (signature gained a
  `&PermissionRulesBySource` param — 6 callers updated) drives the
  "Exited Auto Mode" banner + restore.

### 3.6 acceptEdits no longer blanket-allows bash rm/mv/cp/sed
`ACCEPT_EDITS_COMMANDS` shrunk to `[mkdir, touch]`. Dangerous rm/sed are caught
by the force-ask gates (§3.2) which run *before* the acceptEdits check; a safe
`rm foo.txt` now defers to the rule pipeline rather than auto-allowing — more
TS-faithful (TS acceptEdits auto-accepts file-*edit* tools, not bash `rm`).

### 3.7 Sandbox bridge installed post-build via interior mutability
`SandboxState.approval_bridge: RwLock<Option<…>>`; installed AFTER
`SessionRuntime::build` via `sandbox_state().set_approval_bridge(...)` (SDK path
in `main.rs::run_sdk_mode`). This avoids churning `SessionRuntimeBuildOpts` /
`build_sandbox_state` signatures. `permission_checker()` propagates the bridge
so `check_path_async` is bridge-aware. Survives hot-reload (lives on the
persistent `Arc<SandboxState>`).

### 3.8 Sandbox is opt-in / default-off
`Feature::Sandbox` is default-off. The CRITICAL Linux break (S1) only bit users
who explicitly enabled the sandbox — but that's the security-conscious cohort
that most needs it.

---

## 4. Current status (当前状态) — committed in `1a0b37a32`

Full `just pre-commit` green (whole nextest suite + clippy). 59 files,
+2336/−313. Every item below has companion `.test.rs` coverage.

| Group | Gap | What landed | Severity |
|------|-----|-------------|----------|
| **[0]** | S1/S6 | `coco_sandbox::inner_stage::dispatch_or_continue` in `main.rs` before `Cli::parse`; `WindowsSandbox::available()=false`; Linux integ test | **CRITICAL** |
| **[4]** | P2 | `shell_rules::match_bash_rule` + `RuleMatchPolicy` + `strip_output_redirections`; coco-permissions→coco-shell dep | **HIGH (fail-open)** |
| **[7]** | P1 | `strip_dangerous_allow_rules` at `tool_context.rs` build + Auto-entry stash; `apply_permission_mode_transition_to_app_state` +rules param | **HIGH (fail-open)** |
| **[6]** | P13 | `evaluate.rs::central_rule_applies` (content denies defer to tool); `AgentTool` agentType deny enforcement | **HIGH (fail-closed)** |
| **[3]** | P6/P7/P11 | `read_only.rs` `ReadOnlyRule` table (env/date/ps/sort/pagers removed/validated) | HIGH |
| **[8]** | P9/P10 | `normalize_legacy_tool_name` in `rule_compiler.rs`; `ShellCase` PowerShell case-insensitive | HIGH |
| **[10]** | P14 | `PermissionEvaluationOptions.sandbox_auto_allow_bash` + `tool_call_preparer` wiring | MED |
| **[9]** | P12 | `filesystem.rs` `.claude/worktrees` component match, `/tmp` blanket-allow removed, `.claude/commands` | MED |
| **[1]** | S2/S3 | `SandboxState::start_network_proxy_with_bridge` (netns socat `BridgeManager`); `inner_command_prefix` in executor; `deps::socat_path`; ProxyRouted seccomp allows AF_UNIX; cross-platform `session_runtime` wiring | HIGH (fail-closed) |
| **[2]** | S4/S5/S7 | **CORE:** SDK `SdkSandboxApprovalBridge` installed on live state; `sandbox_preflight`→`check_path_async`; Linux executor records SIGSYS `seccomp_violation`; checker.rs doc fixed | partial — see §5 |
| **[5]** | P4/P5/P15 | **CORE:** force-ask gates in `bash.rs::check_permissions` (dangerous-removal, sed-danger, git-escape); `is_read_only` git-escape; `ACCEPT_EDITS_COMMANDS=[mkdir,touch]` | partial — see §5 |
| **[11]** | P8/P17/P18 | managed-rules enforcement (`typed_permission_rules` policy-only filter); `seed_session_additional_dirs` (--add-dir + settings); `is_running_as_root` (`geteuid`) | partial — see §5 |

**Note (S2 correction):** the audit's "proxy-env never injected" was imprecise —
injection already worked via `try_wrap_command_with_binds` (`snap.proxy_env`).
The real S2/S3 fix was starting the proxy on Linux + the netns bridge.

---

## 5. TODO — remaining work (prioritized)

Ordering reflects the directive: **finish internal implementation first; TUI/UI
surfaces are lowest priority.** All anchors are current as of `1a0b37a32`
(line numbers will drift — search by function name).

### P0 — Internal, completes a security/parity behavior

#### T1. Bash redirect / out-of-tree path-constraint gate ([5] P3)
The `> /etc/passwd` (write outside the working dirs) gate is the one missing
bash path-safety check. The dangerous-removal/sed/git gates already exist in
`core/tools/src/tools/bash.rs::check_permissions`.
- **Where:** add a 4th gate in `bash.rs::check_permissions` (after git-escape,
  before the acceptEdits check), and a helper in `core/tools` (NOT `coco-shell`
  — it needs `coco_permissions::filesystem::is_path_within_allowed_dirs`).
- **How:** extract output-redirection targets (reuse `coco-shell`'s redirect
  parsing / `path_validation` helpers, all pure) + process-substitution
  detection; for each non-`/dev/null` target, if `has_shell_expansion` or NOT
  `is_path_within_allowed_dirs(target, cwd, additional_dirs)` → `Ask`. cwd via
  `bash_gate_cwd(ctx)` (already added). Mirror TS `checkPathConstraints`
  (`pathValidation.ts`). Spec: group `05`, change "bash.rs gate 3".
- **Why P0:** it's the last fail-direction bash gate and is internal-only.

#### T2. Sandbox network ask-callback ([2], `check_network_async`)
TS surfaces "Allow network connection to {host}?" on a denied CONNECT
(`createSandboxAskCallback`). Today the proxy `DomainFilter` returns a static
403/SOCKS-refused — `check_network_async` is still never called.
- **Where:** `exec/sandbox/src/proxy/server.rs` (`ProxyServer::start_with_ports`
  + `run_http_proxy`/`run_socks_proxy` + `handle_http_connect`/`handle_socks5`);
  `exec/sandbox/src/proxy/mod.rs` (export `NetworkAskCallback`/`ViolationSink`);
  `exec/sandbox/src/state.rs::start_network_proxy*` builds the callback from the
  installed `approval_bridge`.
- **How:** add `pub type NetworkAskCallback = Arc<dyn Fn(String) -> Pin<Box<dyn
  Future<Output=bool>+Send>>+Send+Sync>` and thread an `Option<NetworkAskCallback>`
  through the handlers; on a denied host, `await` the callback before refusing
  (build it from `bridge.request_approval(SandboxOperation::Network, host)`).
  Spec: group `02`, changes "proxy/server.rs" + "state.rs start_network_proxy".
- **Why P0:** this is the actual TS-parity network-approval surface; works with
  the already-installed SDK bridge (no TUI needed).

### P1 — Internal, completes feedback/reporting

#### T3. Violation pipeline: monitor spawn + observer → event ([2] S5)
The Linux SIGSYS producer is wired (model sees `<sandbox_violations>`), but the
macOS `log stream` monitor isn't spawned and the observer→TUI-flash isn't
emitted. Note: the `SandboxViolationsDetected` TUI handler already exists
(consumer present, producer missing).
- **Where:** `exec/sandbox/src/state.rs::build` switch `ViolationStore::new()` →
  `with_observer()` + store the `UnboundedReceiver<i32>`; add `take_violation_observer`;
  spawn `ViolationMonitor::start(...)` (macOS) in `build_sandbox_state`; add a
  drain task emitting `ServerNotification::SandboxViolationsDetected { count }`.
  `monitor.rs` needs a sync `cancel()` for `SessionRuntime` Drop. This needs a
  `SandboxRuntimeBundle` return + the `SessionRuntime` field/Drop changes the
  [2] spec describes (the heavier wiring).
- **Spec:** group `02`, changes "state.rs fields", "monitor.rs", "build_sandbox_state".
- **Why P1:** informational (non-blocking on both sides); kernel enforcement
  already works — this restores the after-the-fact report.

#### T4. LLM permission-explainer logic ([11] P16, internal half)
The explainer (risk-level for a prompt) is implemented but never invoked.
- **Where (internal):** `common/config/src/settings/mod.rs` add
  `permission_explainer_enabled: Option<bool>` (default on) + a `RuntimeConfig`
  accessor; `app/cli/src/tui_permission_bridge.rs` add `async fn explain_risk(...)`
  that runs the explainer via the session's SideQuery handle with a bounded
  timeout, graceful-degrades to `None`.
- **Caveat (highest-uncertainty):** resolve the SideQuery-handle access on
  `SessionRuntime` first (inspect how the executor populates
  `ToolUseContext.side_query`). Spec: group `11`, changes "settings/mod.rs" +
  "tui_permission_bridge explain_risk".
- **Why P1 (not P0):** its output only matters once the risk badge renders (T6),
  but the internal infra (settings flag + `explain_risk`) should land first.

### P2 — TUI / UI surfaces (LOWEST priority)

#### T5. Interactive TUI sandbox approval bridge ([2])
A `TuiSandboxApprovalBridge` so denied sandbox ops surface in the interactive
TUI (the SDK path already works). The TUI consumer already exists
(`SandboxPermissionPromptState`, `tui_only.rs` pushes on `SandboxApprovalRequired`,
`update/interaction.rs` sends `ApprovalResponse`).
- **Where:** new `app/cli/src/sandbox_approval_bridge_tui.rs` (mirror
  `tui_permission_bridge.rs`, reuse its `PendingApprovals` round-trip, emit
  `TuiOnlyEvent::SandboxApprovalRequired`); install in `tui_runner` like the SDK
  bridge install in `main.rs`. Spec: group `02`, "sandbox_approval_bridge_tui.rs".
- **Why lowest:** UI-only; the producer-side (T2) + SDK surface deliver the
  parity behavior; this is the interactive-mode convenience.

#### T6. Risk-badge rendering ([11] P16, UI half)
- **Where:** `common/types/src/event.rs` add `risk_level: Option<RiskLevel>` to
  `TuiOnlyEvent::ApprovalRequired` (⚠ **breaks all `ApprovalRequired`
  constructors workspace-wide** — grep + add `risk_level: None` to each non-
  explainer emitter); `app/tui/src/server_notification_handler/tui_only.rs` map
  it into `PermissionPromptState`; render the badge. Spec: group `11`.
- **Why lowest:** UI-only, and the workspace-wide constructor change should land
  in a focused pass.

---

## 6. Build / verify / commit workflow

Run from `coco-rs/`.
- Iterate: `just quick-check` (fmt + seam guards + incremental clippy; ~30s–2m).
  **Do not run mid-edit spam** — run at milestones.
- Final gate (REQUIRED before commit, run ONCE): `just pre-commit` (full nextest
  + clippy). It is orders of magnitude slower; run it long-running/background and
  **check the real exit code** — a `| tail` pipe masks failures (this bit us
  once; the suite had 2 failures behind a `tail` exit 0).
- Conventional Commits; end the message with
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Never auto-commit; wait for explicit user "commit".

**Test-only gotchas seen:** making a sync fn async breaks its companion test
(`#[test]`→`#[tokio::test]` + `.await`); a naive whitespace splitter shreds
quoted sed scripts (use a quote-aware split). Both already fixed.

---

## 7. References

- TS reference source: `/lyz/codespace/3rd/claude-code/src` (per
  `reference_claude_code_ts_source_path` memory).
- Crate internals: [crate-coco-permissions.md](crate-coco-permissions.md),
  [crate-coco-sandbox.md](crate-coco-sandbox.md),
  [crate-coco-shell.md](crate-coco-shell.md),
  [bypass-permissions.md](bypass-permissions.md).
- Historical audit log: [audit-gaps.md](audit-gaps.md);
  fix-ordering: [current-gap-fix-plan.md](current-gap-fix-plan.md).
- Auto-memory: `project_permission_sandbox_dead_wiring.md` (the audit findings +
  fix status, cross-session).
