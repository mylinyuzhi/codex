# TUI Single Render-Path Convergence

Status: refactor design. Disregards backward compatibility by intent.

This document specifies how to collapse `coco-tui`'s native-history rendering
onto a **single render path**, eliminating the provisional-commit /
finalize-reconcile machinery introduced by `#160 perf(tui): streamline native
history rendering` (commit `95cea8827`).

It is scoped to the streaming → scrollback seam only. It does **not** redefine
terminal mechanics (`native-scrollback-architecture.md`), the transcript-cell
contract (`codex-rs-tui-comparison.md` §1–2, §4), or the console product shape
(`agent-console-design.md`). Those remain the owners of their contracts; this
doc references them by name.

## 1. Why this exists

`native-scrollback-architecture.md` **Core Decision #4** is explicit:

> Only finalized history enters native scrollback. Streaming text, running
> tools, … remain in the interactive viewport. When a stream/tool/message
> finalizes, it is emitted as history rows **exactly once**.

`#160` deviated from this to optimize long-answer streaming ("preserve native
streaming output as the viewport fills"). It now writes **streaming** assistant
markdown into the terminal scrollback *before* finalize, then reconciles those
provisional rows against the finalized render. That deviation is the source of
the rendering fragility analyzed in the investigation that prompted this doc.

What the deviation cost us — two coupled defects, both in
`app/tui/src/surface/`:

1. **Two independent render paths that must produce byte-identical lines.**
   - Streaming-stable lines: `StreamRenderController` →
     `render_markdown_region(…, FinalizedStable)`
     (`streaming/render_controller.rs:114`).
   - Finalized lines: a *different* path,
     `render_finalized_history_lines` → `ChatWidget::new(…)`
     `.native_history_append_compatible()` (`surface/history_lines.rs:280`).
   The `native_history_append_compatible()` flag exists **only** to force these
   two paths to agree. Nothing at runtime verifies they still agree.

2. **Count-based reconciliation across those paths.**
   At finalize, `consolidated_final_tail_lines` appends
   `render_finalized_history_lines(whole).skip(provisional.line_count)`
   (`surface/history_driver.rs:861`). The guard checks only `render_key`
   equality and `source.starts_with` (`:834`, `:839`) — it **never** verifies
   that the first `provisional.line_count` finalized lines are the rows actually
   committed. The ledger stores a *count*, not the rows, so it *cannot*.

The skip arithmetic is currently correct (the streaming-stable render is a
sliced whole-prefix render and the markdown renderer is prefix-stable at commit
boundaries — pinned by `tui-markdown/src/lib.test.rs:312`). But correctness
rests on the hand-maintained equality of two render paths plus a guard that does
not check what matters. If the two paths ever drift (a gutter, a model-label
line, a blank-line normalization on one side only), the result is silent
duplicated or dropped lines, caught by no guard — only by sampled tests.

**This is the class of defect to remove, not patch.** A "smarter skip" still
leaves two render paths and a reconciliation step. The fix is to delete the
second path and the reconciliation.

## 2. Target model (codex-rs)

`codex-rs/tui` renders an in-flight answer from **one** rendered-line vector and
tracks what has reached scrollback with **one watermark**, never a cross-path
count match.

Reference (`codex-rs/tui/src/streaming/controller.rs`):

- one `rendered_lines: Vec<Line>` produced from the accumulated markdown source
  through a single render call;
- `enqueued_stable_len` / `emitted_stable_len` counters with the invariant
  `emitted_stable_len <= enqueued_stable_len <= rendered_lines.len()` (`:31`);
- `current_tail_lines()` returns `rendered_lines[enqueued_stable_len..]` — the
  mutable viewport tail, explicitly derived from the *enqueued* watermark to
  prevent already-shown lines reappearing (`:204`);
- finalize renders the remaining source and consolidates into a single
  **source-backed** `AgentMarkdownCell`; the cell re-renders from raw markdown on
  resize (`streaming/controller.rs:231`, `app/agent_message_consolidation.rs`).

The committed scrollback content is **a prefix slice of the same vector**.
Reconciliation is a watermark comparison on one vector, not a count match across
two renders. There is no provisional ledger, no `skip(count)`, no compatibility
flag, no finalize guard.

## 3. Target model (coco)

Adopt the codex mechanism while keeping coco's TEA boundary, `SurfaceTerminal`,
and source-of-truth rule (`SessionState.messages` is canonical;
`native-scrollback-architecture.md` Decision #3). Concretely, restore Core
Decision #4:

**Streaming assistant text lives in the interactive viewport. The assistant
message enters native scrollback exactly once, at finalize, through the same
render function used for replay.**

### 3.1 The one render function (the crux)

Extract a single function that both the live stream and the transcript cell call:

```text
render_assistant_markdown(source: &str, opts: AssistantMarkdownOpts, width) -> Vec<Line<'static>>
```

- Live streaming: the stream controller renders `accumulated_source` through it
  each frame; the **stable prefix** (`stable_prefix_end`) is the committable
  region, the suffix is the mutable viewport tail.
- Finalize / replay: the `AssistantText` transcript cell renders the *same*
  source through the *same* function at the same width.

Because finalize and streaming are literally the same code over the same source,
the finalized lines are identical to the streamed lines by construction. The
"two paths must agree" coupling disappears — there is one path.

### 3.2 Streaming watermark (replaces the ledger)

`SurfaceStreamDriver` owns one `rendered_lines` and one `emitted_stable_len`
watermark (mirroring codex). Per frame:

- recompute `rendered_lines = render_assistant_markdown(source, …, width)`;
- `stable_line = line index of stable_prefix_end(source)`;
- **viewport tail** = `rendered_lines[stable_line..]` (+ cursor / thinking);
- **(optional) progressive commit**, see §3.3.

Finalize: emit `rendered_lines[emitted_stable_len..]` once, then drop stream
state. Same vector, same render → the suffix is exact. No `skip(count)`, no
guard, no `ProvisionalFinalizationGuard`.

### 3.3 Two valid commit policies — pick one

**Policy A — insert-once-at-finalize (recommended; matches Core Decision #4).**
Streaming stays entirely in the interactive viewport (`emitted_stable_len == 0`
until finalize). On finalize, the whole message is emitted once. Simplest;
restores the documented contract verbatim; zero scrollback writes mid-stream.
Cost: during a very long answer, streamed lines scroll *within* the bounded
interactive viewport, not into native scrollback, until the message completes.

**Policy B — progressive single-watermark commit (only if mid-stream
scrollback is a hard requirement).** Keep `#160`'s UX goal (stable lines reach
native scrollback as the viewport fills) but via one vector + one watermark:
emit `rendered_lines[emitted_stable_len..stable_line]` to scrollback during
streaming and advance the watermark; committed is always a prefix slice of
`rendered_lines`. Finalize emits the suffix. Still single-path — no second
render, no ledger, no reconciliation. Resize mid-stream is handled by §3.4.

Policy A is the default. Choosing B is a documented deviation from Core Decision
#4 and must be justified by a concrete long-stream requirement; even then it
must remain single-path.

### 3.4 Resize, `/clear`, rewind = source replay (unchanged contract)

Width change, `/clear`, truncate/rewind, and session switch rebuild from message
source via the existing replay path (`replay_all_capped` → `replay_rows` →
`clear_owned_scrollback` + re-insert). This already satisfies Decision #3/#6 and
is kept. With one render function, replay and finalize emit identical rows for
the same cell — the property the dual path could only approximate.

## 4. Invariants this establishes

1. **One render function** for assistant markdown across live, finalize, and
   replay. No `native_history_append_compatible` shim.
2. **One source of truth per cell** = its markdown source (Decision #3). Scrollback
   rows are a disposable projection.
3. **Committed = prefix slice of the current render**, tracked by one watermark.
   Never a count match across two renders.
4. **Finalize emits a suffix of the same vector**, exactly once (Decision #4).
5. **No silent divergence path**: there is nothing to drift, so no guard is
   needed; a width/source change re-renders the one vector and (if needed)
   triggers source replay.

## 5. Component-level change set

Paths under `coco-rs/app/tui/src/`. "Delete" assumes no back-compat (per request).

| Symbol / file | Action | Notes |
|---|---|---|
| `surface/stream.rs` `ProvisionalStreamLedger` / `CommittedStablePrefix` | **Delete** | Replaced by `rendered_lines` + `emitted_stable_len` watermark. |
| `surface/stream.rs` `PreparedProvisionalAppend`, `pending_prefix`, `mark_stable_appended`/`forget_stable_appended` dance | **Delete / collapse** | Watermark advance is a single field write. |
| `surface/history_driver.rs` `emit_provisional_stream` | **Delete** | Policy A: gone. Policy B: replaced by a watermark-slice emit. |
| `surface/history_driver.rs` `consolidated_final_tail_lines`, `ProvisionalFinalizationGuard`, `finalized_render_key`, `skip(provisional.line_count)` | **Delete** | The entire count-based reconciliation. |
| `surface/history_driver.rs` `HistoryTailCache` / `fill_tail_gap` / `tail_reveal_rows` | **Re-evaluate** | Viewport-reveal gap fill is independent of the bug; keep only if still needed by scrolling. Out of primary scope. |
| `streaming/render_controller.rs` `StreamRenderMode { FinalizedStable, StreamingMutableTail }`, dual `markdown_options` branch | **Collapse** | One mode. The mutable-tail vs stable split becomes a *line-index* boundary on one render, not two render modes. |
| `streaming/render_controller.rs` `StreamRenderKey` | **Simplify/keep** | Still useful to detect width/theme/syntax change → re-render + replay. No longer the finalize guard. |
| `surface/history_lines.rs` `render_finalized_history_lines` assistant arm via `ChatWidget` | **Reroute** | Assistant text cell renders via the shared `render_assistant_markdown`. |
| `widgets/chat/*` `ChatWidget::native_history_append_compatible()` | **Delete** | No second path to be compatible with. |
| `surface/controller.rs` provisional re-emit branches (`:288`–`:377`) | **Delete / simplify** | Frame flow becomes: replay-if-needed → emit finalized suffix(es) → render viewport tail. |
| `surface/history_driver.rs` `HistoryEmissionTracker` (cells, by message id) | **Keep** | Per-cell emit-once watermark for non-streaming cells (tools, user, system). Unchanged. |
| `surface/history_driver.rs` `replay_all_capped` / `replay_rows` | **Keep, simplify** | Source replay for resize/`/clear`. |
| `tui-ui/src/engine/terminal.rs` `insert_history_rows`, `clear_owned_scrollback` | **Keep** | Terminal primitives are correct. |

New / changed:

| Symbol | Action |
|---|---|
| `render_assistant_markdown(source, opts, width)` | **Add** — the single shared render fn; used by the stream controller and the `AssistantText` cell. |
| `SurfaceStreamDriver` | **Rewrite** around `rendered_lines` + `emitted_stable_len`; emits a suffix at finalize. |

This aligns with the target vocabulary in `codex-rs-tui-comparison.md` Reuse
table (`MarkdownStableTail`, `HistoryEmissionController`); name the rewritten
types to match those targets rather than inventing new ones.

## 6. Migration sequence (each step compiles + `just quick-check` green)

1. **Extract `render_assistant_markdown`.** Pull the assistant-markdown render
   out of `ChatWidget` and `render_markdown_region` into one function. Make both
   current call sites delegate to it. No behavior change yet; existing tests stay
   green. This is the load-bearing step — it proves the two renders were
   unifiable.
2. **Reroute the finalized `AssistantText` cell** through it; delete
   `native_history_append_compatible`. Run the replay/finalize snapshot + VT100
   suites.
3. **Introduce the watermark** in `SurfaceStreamDriver` (`rendered_lines` +
   `emitted_stable_len`); compute the viewport tail as a line-index slice.
   Keep emit-at-finalize only (Policy A). Streaming still renders each frame.
4. **Delete the provisional path**: `emit_provisional_stream`,
   `consolidated_final_tail_lines`, `ProvisionalFinalizationGuard`,
   `ProvisionalStreamLedger`/`CommittedStablePrefix`, the controller re-emit
   branches, and the `StreamRenderMode` second branch. Finalize now emits
   `rendered_lines[emitted_stable_len..]`.
5. **Re-evaluate `HistoryTailCache`/`fill_tail_gap`.** If viewport reveal still
   needs it, keep as an isolated cache with no role in finalize. Otherwise delete.
6. **(Optional) Policy B.** Only if a long-stream requirement survives review:
   add watermark-slice emit during streaming + a resize → replay reset. Land
   behind its own tests.
7. **Run `just pre-commit` once** at the end.

Steps 1–4 are the refactor proper. 5–6 are cleanup/optional.

## 7. Test plan

Keep and re-point the invariant tests; add drift coverage that the old design
lacked.

- **Reuse:** `tui-markdown/src/lib.test.rs:312` (prefix stability),
  `surface/stream.test.rs:74` (block-boundary append),
  `surface/history_driver.test.rs:194` (real driver, no duplication).
- **Add — single-path identity:** for a corpus of assistant messages
  (paragraphs, bold "heading" lines, lists, fenced code, pipe tables), assert
  `render_assistant_markdown(whole)` equals the concatenation of the streamed
  stable slices + the finalize suffix, line-for-line. This is the property the
  dual path could only sample.
- **Add — finalize-after-stream, no dup/drop:** drive the rewritten
  `SurfaceStreamDriver` through chunked deltas crossing block boundaries, then
  finalize; assert each source line appears exactly once in scrollback (VT100
  backend), for both Policy A and (if built) Policy B.
- **Add — resize mid-stream:** width change during streaming → source replay →
  assert scrollback matches a fresh whole-render at the new width.
- **Negative:** intentionally perturb the cell render vs the stream render in a
  test double and assert the unified function makes them identical (i.e. the
  divergence is unrepresentable), replacing the old guard-based defense.

Follow the testing split in `codex-rs-tui-comparison.md` §P0: ratatui
`TestBackend` for buffer snapshots, byte-capturing VT100 backend for
terminal-control assertions.

## 8. codex-rs reference map

| Concern | codex-rs source | coco target |
|---|---|---|
| Single rendered-line vector + watermark | `streaming/controller.rs:31,58,204` | `SurfaceStreamDriver` rewrite |
| Source-backed finalized cell | `history_cell/*` `AgentMarkdownCell`; `app/agent_message_consolidation.rs` | `AssistantText` `TranscriptCell` + `render_assistant_markdown` |
| Newline-gated markdown accumulation | `markdown_stream.rs` `MarkdownStreamCollector` | stream controller source accumulation + `stable_prefix_end` |
| Emit-once history insertion | `insert_history.rs` | `insert_history_rows` (kept) |
| Resize = re-render from source | `streaming/controller.rs:231`, `app/resize_reflow.rs` | `replay_all_capped` (kept) |

Reuse policy and attribution requirements are owned by
`codex-rs-tui-comparison.md` §Reuse Policy — port behind coco-owned types, do not
depend on `codex-*` crates.

## 9. Risks and rollback

- **Risk: a long in-flight answer under Policy A is bounded by the interactive
  viewport** until finalize. This is the documented Core Decision #4 behavior; if
  product testing shows it regresses perceived responsiveness, adopt Policy B
  (still single-path). Decide before step 3.
- **Risk: `render_assistant_markdown` extraction surfaces a real rendering
  difference** between the two current paths (gutter/marker/indent). That is the
  latent bug; resolve it in step 1 by choosing the correct single rendering and
  updating snapshots — do not re-introduce a compatibility flag.
- **Rollback:** steps are independent commits; reverting step 4 restores the
  provisional path. Because we disregard back-compat, prefer fixing forward.
- **Confirm-first:** before investing, reproduce the original duplication on HEAD
  with `COCO_LOG=tui::surface=debug,coco=debug` and confirm whether it still
  occurs; the `tui::surface::append/replay` debug lines are otherwise filtered by
  the default `coco=debug,info` subscriber.

## 10. Out of scope

- Tool/exec/hook activity presentation (`TurnActivityView`,
  `codex-rs-tui-comparison.md` §7).
- Pager/diff overlays, picker scaffolding, bottom-pane stack.
- Terminal primitive behavior (owned by `native-scrollback-architecture.md`).
- `HistoryEmissionTracker` cell-keying for non-streaming cells (already correct).
