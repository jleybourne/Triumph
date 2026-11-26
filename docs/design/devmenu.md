# Dev menu: build-ready design (supersedes design\debug-menu.md)

Tags: [V] verified in code by the earlier study or research, [I] inference, [U] unverified, must be checked at the checkpoint named.
Rules: the title is never named; no game paths; Part B has no offsets in the repo. All of it is off by default.

## 0. Switches (all default off)

| env | effect |
|---|---|
| VICTORY_DEVMENU=1 | Part A: overlay, F10 menu, input gate. Needs the diagnostics build for viewers and skip picker. |
| VICTORY_DEVMENU_WORLD=1 | Part B enabled at all. Requires DEVMENU=1. |
| VICTORY_DEVMENU_DISCOVER=1 | B1 finder and B2 logger only (read-only finder; logger overwrites slots). No calls into the title. |
| VICTORY_DEVMENU_CALLS=1 | Allows the request queue to execute (B3). Without it the world panels are view-only. |
| VICTORY_DEVMENU_OVERLAY=fpsbox | Force the fallback renderer. |
| VICTORY_FPS_COUNTER=0 | Keeps its current meaning for the compact line. |

Part B refuses to arm when an online mode is detected [U: how to detect; fall back to a manual "offline only" confirmation in the panel]. Using any world tool stamps the `usual frame` log line "world tools used" so measurements are not mistaken for baselines.

## Part A: layer tools

### A1. Input isolation (first)
- Files: new `static MENU_OPEN: AtomicBool` plus `static MENU_DEBOUNCE: AtomicBool` in victory-core (hle must not depend on gpu) [V: dependency direction per earlier study]. `victory-hle/src/modules/gameinput.rs`: in `Source::keys()` and `Source::pad()` (and the stand-in pad path, and the VICTORY_KEYS merge) return empty/neutral when `MENU_OPEN || MENU_DEBOUNCE`. Neutral means a zeroed connected pad, not disconnected.
- Toggle: F10 [V: F11 is `gpucapture.rs`; F10 not in `FALLBACK_BUTTONS`]. The menu polls `GetAsyncKeyState` itself, once per present, edge-detected, only when `focused()`. Do not touch the window procedure.
- Debounce: on open send `rumble(slot,0,0)`; on close keep MENU_DEBOUNCE set until every key and pad button is physically up for 3 consecutive polls, then clear. Prevents the closing Enter becoming a title press.
- Menu keys: arrows, Enter, Backspace, PgUp/PgDn, F9 capture; pad dpad/A/B. Text entry (names, hashes) is by key-edge polling into a small line editor, since messages are ignored.
- Tests: `Source` is a trait; unit test with the gate set asserts empty keys and neutral pad. Effort 1-2 days. Risk: another reader of raw input outside gameinput.rs [U: grep for other GetAsyncKeyState/XInput callers at the checkpoint].

### A2. In-house overlay renderer (`victory-gpu/src/overlay.rs`, about 400 lines)
- Pieces: (1) glyph atlas, a 128x? R8 texture generated at startup from the existing 5x7 font scaled 2x (full ASCII), uploaded once; (2) rect quads and glyph quads share one vertex format (pos, uv, rgba); (3) one dynamic vertex buffer, upload heap, N slots by swap-chain frame, slot reused only when its fence <= FINISHED (same rule as `p.ahead`); (4) tiny VS/PS compiled offline to bytecode blobs checked in (no runtime compiler dependency); (5) own root signature: one SRV table for the atlas and 4 root constants (1/width, 1/height); (6) own small SRV heap; (7) one PSO per back-buffer format with alpha blend, built lazily; (8) viewport/scissor, no depth.
- Call site: `host.rs` `Host::present_now` [V ~8866], immediately after the back-buffer `CopyResource` and where `fpsbox::draw` is called now: transition back buffer to render target, `OMSetRenderTargets` on an RTV in our own heap, draw, transition back to the state the closing barriers expect. Drawn into the back buffer only, so `capture::read_back` (reads `texture`) stays clean [V].
- Heaps: the list must set the overlay's descriptor heap, which can displace the title's heaps; present_now is at the end of the list so only restore if later present code needs them [U: check what follows in present_now].
- Fallback: `fpsbox::draw` stays; chosen when PSO or root signature creation fails, or VICTORY_DEVMENU_OVERLAY=fpsbox. Its per-text cache (2048) is fine for the compact line only.
- Snapshot: submit thread fills a `Snapshot` struct (fps, median ms of 60, draws via `draw::drawn()`, dropped via `draw::take_dropped()` kept as an own copy, `queue::presented()`, framelog lines); the overlay reads a copy, takes no title lock [V: counters exist, names per study].
- Budget under 0.3 ms per frame, measured with `framelog::T::PresentShowUs` [V name]. Effort 4-5 days. Risks: format variance in swap-chain (HDR/float) [U], barrier state assumptions [U], ring reuse.

### A3. Panels
Panel framework: a `Panel` trait (title, key handler, draw into a quad list), a left tab column, PgUp/PgDn to switch. Stats is the compact view plus the framelog account.

1. Stats: fps · ms · draws · dropped, plus preparing, copies, copy-back, GPU busy, list calls, wait in present. Banner "switches changed live: not a baseline". Dropped split into real drops versus "skipped by menu". 1 day.
2. Live switches: a `Switch` type (`AtomicU8`: off/on/unset-use-env, Relaxed load) with a `switch!(NAME)` registry macro enumerated by the menu, in victory-core. Classes, shown as a tag per row:
   - live: `FPS_COUNTER`, capture cadence, shot trigger, `SKIP_PIPELINES`, `SKIP_TARGET_TRANSITIONS`, `SKIP_VERTEX_REBIND`, `DEDUPE_BARRIERS`: applied immediately.
   - latch: `LATE_WAITS`, `LIGHT_REFRESH`, `MERGE_GAP`, `MERGE_RUNS`: pending value moved to active in `present_now`.
   - drain: `ASYNC_SUBMIT`, `QUEUE2`, `INDIRECT_*`, `STAGING_POOL`, `COPY_PLAIN`, `BACK_OWN_VIEW`, `TEXTURE_BACK`, `FILLS_PLAIN`, `CARD_*`: request, apply after `settle_now(WHY_SYNC)` at the next present; marked experimental (a known all-on set crashes at start).
   - startup-only: `INPLACE*`, `XMEM_COMMIT`, `PRECOMPILED`, `D3D12_DEBUG`, translation flags: read-only, "restart required".
   Convert about 25 switches, not all 839 uses. Persist chosen set to the work folder and print as one env string; never recommend (the baseline rule belongs to the user). Effort 3-4 days. Risk: a switch classified live that is actually cached per pipeline [U: audit each at conversion].
3. Shader-skip picker: `pipeline.rs` `skipped_shader` [V ~532] becomes a shared list plus epoch; each `Pipeline` caches verdict and epoch. Per-frame hash to count map filled where `draw.rs` resolves the pipeline (diagnostics only). UI: list, toggle, solo, clear. Warning about stale targets. Effort 2-3 days.
4. Capture: F9 calls `shot::request()` (same flag as the `take` file) plus plain frame PNG via `capture::read_back` [V ~130 ms for that frame]. Folder under the work dir captures, never Temp. Toast when `shot` reports done. 1 day.
5. Viewers: v1 target list (format, size, last writing pass) from shot naming, snapshot copy read back and shown scaled, labelled "snapshot, not live" (~2 s). v2 live: own SRV heap and fullscreen triangle PSO at frame end inside the present list, with transition and restore. Only after v1. v1 3-4 days, v2 5-7 days. Risk: enhanced-barrier state tracking and overwritten placed memory [U].

## Part B: world tools

Gate: VICTORY_DEVMENU_WORLD=1 and a known/unlocked build (B5). Nothing here ships offsets.

### B1. Discovery mode (first; read-only)
- Where: new `victory-hle/src/devmenu/discover.rs` (or victory-gpu if hle cannot see the pe info; [U] which crate holds the mapped image handle) using `victory-pe` `ImageInfo` [V: sections, .pdata presence].
- Trigger: worker thread started when the title idles at its menu (use the existing present count stable plus no loading flag; [U] define "idle", simplest is a menu key "Run finder" in the panel, manual).
- Algorithm: iterate .rdata in 8-byte steps; keep (u32 or u64 hash, pointer) pairs where the pointer lies in .text and equals a RUNTIME_FUNCTION BeginAddress from .pdata (binary search). Require runs: a table is a run of at least 64 consecutive qualifying pairs with strictly distinct hashes; the count should be in the thousands [I]. Pair layout (hash first or pointer first, 4 or 8 byte hash, stride) is unknown: try the few layouts and score each [U].
- Confirm: hash a known native name taken from the title's own data with candidate hash functions (try the common string hashes; [U]) and see it appear as a key. Read-only, section-bounded, chunked reads with `VirtualQuery` so a decommitted page cannot fault.
- Output: a candidate table report (RVA of table, count, stride, hash function match) written to the data dir. No pointer is called. Effort 3-7 days. Risks: stripped, unusual table form (hash-sorted binary tree, or per-slot indirection) [I]; mitigation: report .rdata string dump and pointer-run statistics for manual analysis.

### B2. Native-call logger (builds the catalogue)
- Method: overwrite a table slot (not inline patch: no length decoder exists [V]) with the address of a loader-emitted stub (`victory-loader/src/thunks.rs` facility [V exists]) that saves registers, records (hash, first N argument registers, thread id, frame) in a lock-free ring, and tail-jumps to the original. Slot lives in .rdata: change page protection around the write, as the loader does for the IAT [V].
- Slots are logged only when selected in a panel (or all for a bounded time); one slot at a time recommended. Arguments are raw register values, never dereferenced; the formatter classifies by guess (float bits, small ints, pointer-looking) and marks as guesses.
- Output: catalogue file per build: hash, rva, call count, arg histograms, optional name from a name list. Used by the human to label natives (set position, set time, etc.) in the sidecar (B6). Flag VICTORY_DEVMENU_DISCOVER=1. Effort 1 week. Risks: calling convention translation of the console ABI [U], stub stack alignment, shadow space, re-entrancy, the table being copied elsewhere at startup so overwriting has no effect [U]; the thread of each call is recorded to settle the thread question for B3.

### B3. Request queue and game-thread entry
- Queue: MPSC bounded ring in victory-core (`WorldRequest` enum: Teleport, SetTime, SetWeather, Toggle, Spawn), produced by menu code (submit thread), drained only by the game thread.
- Entry: an HLE function the title calls every frame on its game thread. Candidate: the gameinput read (polled every frame [I]); alternatives: a frame-pacing, timing or present-side HLE call. Confirm which: with the B2-style logging (or a counter bumped inside each candidate HLE entry plus `GetCurrentThreadId`), record per entry the calling thread and calls per frame; choose the entry that is always the same thread, once per frame, and the same thread as the logged native calls from B2. Checkpoint C4. [U: all of this]. Do not drain from `present_now` (submit thread [V]).
- Drain rules: only when state flags allow (not loading, not in cutscene, not paused; [U] how to read), at most one request per frame, with a cooldown; multi-frame requests (streaming wait) are small state machines advanced one step per drain.

### B4. Crash containment
- Pointer validation: every pointer from the title or sidecar passes `valid_read(ptr,len)` (VirtualQuery: committed, readable, not guard, not NOACCESS) and, for call targets, executable and inside .text and a .pdata function start. Writes only via a title function, never raw.
- In-world-call guard: thread-local `IN_WORLD_CALL` plus a saved recovery context (`setjmp`-like capture in a small asm/stub trampoline `guarded_call(fn, args)`). A vectored handler (`AddVectoredExceptionHandler`, first handler) checks the flag: if set and the exception is an access violation or illegal instruction, it logs through `crash.rs`, sets the global `WORLD_DISABLED`, clears the flag, restores RSP/RIP to the recovery point returning an error. Faults outside a call pass on. [V: crash.rs logs first-chance faults; watch.rs handles write faults only.] [U: recovery is best effort; a fault midway may leave the title's locks or state corrupt; document as "disable and expect instability, restart advised"]. Stack overflow, other threads' faults, and faults in memory the callee corrupts are not covered.
- Also: per-call watchdog (call count and time) and a one-shot confirm ("probe call") before first use of each sidecar entry. Effort 1 week. Risk: unwinding past the title's frames and translated code [U].

### B5/B6. Per-build sidecar tables
- Key: BLAKE3 build id [V: `identify` prints "build unknown (<hex>)"; KNOWN_BUILDS empty]. File `<data dir>\devmenu\<hex>.toml` outside the repo. Contents: schema version, PE timestamp and CodeView GUID/age as cross-check [V parsed], table RVA/stride/hash function, per-native entries (name, hash, rva, signature description, byte-pattern with mask, check bytes), per-feature argument templates, bookmarks, spawn name lists.
- Load: for each entry, verify the pattern at the RVA; on mismatch rescan the image section for the pattern; on failure mark the feature disabled (never guess). Unknown build (no file) means discovery mode only. The sidecar is written by the finder/logger panels and hand-edited. Never committed; `.gitignore` nothing needed since it lives in the data dir. Effort 3 days. Risk: patterns brittle across patches; accepted.

### B7. Features, in order
1. Teleport (high value). Needs natives: get player entity [U], set position [U], request streaming/collision around a point [U]. Sequence as a state machine: request collision at target, wait until reported ready or timeout, set position, optionally zero velocity. Bookmarks: save current position (name, xyz, heading) to the sidecar; "test-area" bookmarks are user-created named positions with a category tag, listed in a panel; a coordinate editor for manual values. Effort 1-2 weeks after B1-B4. Risks: falling through world when streaming is not ready; interior versus exterior cells.
2. Time and weather. Setters [U] and whatever re-applies them each frame [U] (may need to call each frame while held: "freeze" toggle through a persistent request). Name lists from the title's own data via the hash function. 1 week. Risk: overridden next tick.
3. Feature toggles (invincible, etc.): each is a native or a persistent per-frame request; one-by-one, each needing a catalogue entry. About 2-3 days each. Risk: save state affected; warning banner, never persisted.
4. Spawning (last). Model streaming API (request, poll resident, create, release) and name hash [U]; state machine over frames; handle cleanup on failure; cap live spawns. 2-3 weeks. Risks: highest: unresident models, stale handles, draw count changes (stamp line).

## Build order with checkpoints
1. A1 input isolation. C1: unit test passes; in a run, F10 opens, title ignores keys, closing leaves no phantom press.
2. A2 overlay and stats. C2: draws correct in all swap-chain formats, under 0.3 ms, captures stay clean, fpsbox fallback works.
3. A3.4 capture key, then A3.2 switch registry (live, then latch, then read-only rest). C3: one switch toggled live changes the metric; no crashes with latch/drain.
4. A3.3 skip picker, then viewers v1 (v2 optional).
5. B1 finder (idle menu). C4: a table candidate with a verified hash match, or stop and reassess Part B (the go/no-go point).
6. B2 logger on one harmless slot. C5: logged calls show sensible arguments, threads recorded.
7. B3 entry choice plus empty queue drain (no native calls) and B4 guard with a deliberate fault test on a safe dummy target. C6: fault is contained, Part B disabled, title survives.
8. B6 sidecar load and verify. C7: unknown build gives discovery only; pattern mismatch disables.
9. B7: teleport, time and weather, toggles, spawning, one at a time, each with its own checkpoint run on diagnostics build, single instance, nothing compiling.

## Effort summary
Part A about 3-4 weeks for v1 of everything (viewer v2 extra). Part B about 6-8 weeks to something usable on one build, high variance, plus a recurring cost per title update. Main unknowns: table form and hash function (B1), game-thread entry (B3), calling-convention translation (B2).
