# Debug menu design (read-only study of D:\victory-work\wt-bisect)

Rule held throughout: the title is never named. Part B contains no offsets.

## What exists today (the seams)

- Present path: `victory-gpu/src/host.rs` `Host::present_now` (~8866). It copies the title's frame into the
  swap chain back buffer with `CopyResource`, then calls `fpsbox::draw(self, list, &back, ..)`, then the
  closing barriers, `execute`, `Present`, then `p.meter.frame(hwnd)` (the meter, ~8572, measures fps once a
  second). Everything drawn lands on the back buffer only, never the title's texture. This is the overlay seam.
- `fpsbox.rs` is a CPU-rasterised 5x7 font into an immutable upload buffer, one buffer per distinct text,
  copied with `CopyTextureRegion` (no PSO, no descriptor heaps, works in every swap-chain format). It is
  reusable as the text engine; its per-text cache (2048 entries) must become a per-frame ring for a full menu.
- Counters: `draw::drawn()` (draw.rs:540), `draw::take_dropped()` (557; resets on read, so the overlay keeps
  its own snapshot), `queue::presented()` (queue.rs:1280), `framelog` (the frame account, `usual frame` line).
  `framelog`, `capture::op`, `shot` and the skip-pipelines list are `diagnostics`-gated. The release build
  reaches 13 draws and cannot be measured, so the full overlay is a diagnostics feature first.
- Capture: `capture.rs` (VICTORY_CAPTURE, power-of-two frames, `read_back` inside the present) and `shot.rs`
  (VICTORY_SHOT folder plus a `take` file; per-frame events, stamps, snap PNGs, pictures taken in parts, read
  2 s and 8 frames later and written on a thread of their own). `shot` already has a "take" trigger.
- Input: `victory-hle/src/modules/gameinput.rs`. `host::Source::keys()` polls `GetAsyncKeyState` over
  0x08..0xFE whenever `focused()` (foreground window belongs to our process). `pad()` reads XInput slots 0-3.
  With no pad, the keyboard drives a stand-in pad (`FALLBACK_BUTTONS`: Esc, Tab, Enter, Backspace, X, Y,
  arrows, Q, E, F, H; WASD/IJKL sticks, Z/C triggers). `VICTORY_KEYS` scripted input is merged in too.
  Consequence: any key the menu polls is also delivered to the title. There is no consume mechanism today.

## Part A: layer tools

### A1. Input isolation (build first; everything else depends on it)
- Add `static MENU_OPEN: AtomicBool` in a place both crates can see (victory-core); hle must not depend on gpu.
- Toggle key: one outside every title mapping and the stand-in pad table, e.g. F10 (not in
  `FALLBACK_BUTTONS`). The menu edge-detects it with its own `GetAsyncKeyState`, not through `keys()`.
- While `MENU_OPEN`: in `gameinput.rs`, `Source::keys()` returns empty and `Source::pad()` returns a neutral
  PadState (all zero, so the title sees "released", not "disconnected"); the stand-in pad likewise. The menu
  reads the real keyboard and XInput itself, polled once per present so it follows frame cadence. Menu keys:
  arrows, Enter, Backspace, PgUp/PgDn; on a pad, dpad/A/B.
- Stale-state hazard: on open the title last saw buttons held, so it must see a release; on close, keep the
  neutral state until every physical key and button is up (menu-owned "debounce until all-up"), otherwise the
  closing Enter becomes an A press in the title.
- Rumble: send `rumble(slot, 0, 0)` on open.
- Do not hook the window procedure or use raw input: the title owns window messages and polling is the
  convention already. Gate the menu on `focused()` too, so another application does not drive it.
- Test: `Source` is a trait, so a unit test with `MENU_OPEN` set can assert empty keys and neutral pads.

### A2. Overlay and stats panel
- New `victory-gpu/src/overlay.rs` replaces `fpsbox::draw` at the same call site (keep `VICTORY_FPS_COUNTER=0`
  meaning). Two states: compact (one line, always on, as now) and full panel (menu open).
- Content, honouring the repo rule "fps · ms · draws · dropped": `fps · ms (median of the last 60) ·
  draws/frame · dropped/frame`, plus the frame account lines `framelog` already produces (preparing, copies,
  copy-back, GPU busy, list calls, wait in present). One `Snapshot` struct is filled by the submit thread at
  present time and the overlay copies it; the overlay never takes a lock a title thread holds.
- Drawing: extend the font to full ASCII and render into a per-frame upload ring sized by swap-chain slot.
  fpsbox avoids rewriting a buffer a present in flight still reads by never rewriting; the ring replaces that
  guarantee: reuse a slot only when its fence value is <= `FINISHED`, as `p.ahead` does. A copy cannot blend,
  so v1 uses opaque boxes; a small alpha-blend quad PSO (root constants, own heaps) is later polish.
- It must be recorded into the present's own list after `CopyResource`, so saved frames
  (`capture::read_back` reads `texture`, not `back`) stay clean, as today.
- Budget: the present is about 2 ms now; the overlay must stay under 0.3 ms. Measure it with
  `framelog::T::PresentShowUs`.

### A3. Runtime toggles for VICTORY_* switches
839 occurrences across 66 files; most are `OnceLock<bool>` read once. Classify, do not convert all:

| class | examples | how |
|---|---|---|
| Live-safe now (observation or per-draw choices) | `FPS_COUNTER`, capture cadence (`CAPTURE_WORLD_EVERY`), shot trigger, `SKIP_PIPELINES`, `SKIP_TARGET_TRANSITIONS`, `SKIP_VERTEX_REBIND`, `DEDUPE_BARRIERS` | Replace the `OnceLock` with a `Switch` type: an `AtomicU8` (off, on, unset-use-env) read with `Relaxed`, seeded from the env var. One `switch!(NAME)` macro registers it in a static list the menu enumerates. Hot-path cost is one relaxed load, the same as `OnceLock::get`. |
| Live at a frame boundary | `LATE_WAITS`, `LIGHT_REFRESH`, `MERGE_GAP`/`MERGE_RUNS`, `THIN` once ported | Store a pending value; latch it into the active one in `present_now`, so no frame is recorded half in one mode. |
| Needs a drain | `ASYNC_SUBMIT`, `QUEUE2`, `INDIRECT_*`, `STAGING_POOL`, `COPY_PLAIN`, `BACK_OWN_VIEW`, `TEXTURE_BACK`, `FILLS_PLAIN`, `CARD_*` | Request, then apply after `settle_now(WHY_SYNC)` at the next present. Mark experimental: CLAUDE.md records an all-on set crashing at startup. |
| Startup only | `INPLACE*`, `XMEM_COMMIT`, `PRECOMPILED`, `D3D12_DEBUG`, the translation flags in victory-shader | Show read-only with "restart required"; offer "write to launch config" instead. |

- Persistence: save the chosen set to a file under `D:\victory-work\` and print it as one env string.
  CLAUDE.md says a switch enters the launcher only after a measured run that beat the baseline at the same draw
  count, so the menu records, never recommends.
- Measurement guard: a banner "switches changed live: not a baseline", and the active set printed on the
  `usual frame` log line, because one switch per measurement is the repo's rule.

### A4. Shader-skip picker
- `pipeline.rs` `skipped_shader` (~532) reads `VICTORY_SKIP_PIPELINES` into a `OnceLock<Vec<String>>`
  (16 hex digits, diagnostics only). Change it to a shared list behind an epoch counter: each `Pipeline`
  caches its verdict plus the epoch it was computed at, so the draw path costs one integer compare and the
  list is re-read only after the menu changes it. `note_drop` and `is_skipped` keep working unchanged.
- UI: list shaders drawn in the last N frames (hash, stage, draw count, GPU ms if `passtime` has it), arrows to
  move, Enter to toggle, "solo" (skip all but the selected) and "clear all". Needs a small per-frame
  hash->count map filled where `draw.rs` resolves the pipeline (diagnostics only).
- The overlay's `dropped` figure must separate "skipped by menu" from real drops (the drop map already labels
  "skipped shader <hash>"), or the fps · ms · draws · dropped line lies.
- Warn in the UI: skipping a pass that writes a target a later pass reads leaves stale data.

### A5. Render-target and texture viewers
- Identity: reuse `shot.rs` naming (texture plus how many times it left a written state in the frame), already
  stable across frames built the same way. The layer tracks every texture's state itself
  (`HostList::transition`/`prelude`), because the title uses enhanced barriers.
- v1 (cheap, safe): list targets (format, size, last writing pass) from the shot data; pick one, queue a
  snapshot copied to a read-back buffer as `shot` does, convert to displayable (tone-map half-float, depth to
  grey, BC formats decoded or viewed as UNORM), upload into the overlay ring, show scaled. Latency like shot's
  (about 2 s), labelled "snapshot, not live".
- v2 (live): own SRV heap and a fullscreen-triangle PSO sampling the chosen texture at the end of the frame,
  inside the present list with a transition and transition back (as `present_now` does for the main texture).
  Hazard: textures in placed or in-place memory can be overwritten by the next pass; read only at the end of
  the frame. Only after v1 works.
- Controls: channel masks, exposure, mip, slice, depth range.

### A6. One-key frame capture
- F9 calls a new `shot::request()` that sets the same flag the `take` file does, plus `capture::read_back` for
  a plain full-size PNG of the title's frame. Toast on completion from shot's `done`.
- `capture.rs` documents that reading back inside the present costs about 130 ms for that frame: fine for a
  manual key, never per frame. Default folder `D:\victory-work\captures\` (CLAUDE.md: never Temp).
- Plain PNG works in release builds; events, stamps and snaps need diagnostics.

### A7. Effort (Part A), one engineer who knows the tree
Input isolation 1-2 days; overlay engine and panel 3-4 days; switch registry (macro, ~25 conversions,
classification) 3-4 days; skip picker 2-3 days; capture key 1 day; viewer v1 3-4 days; viewer v2 5-7 days.
Roughly 3-4 weeks for v1 of everything. Main risks: overlay cost and ring safety, and not perturbing the
measured frame. Test on diagnostics builds, one instance, nothing compiling during a run.

## Part B: title-world tools (approach only; no offsets, none guessed)

### B1. Approach
The title's x86-64 code runs in our process, so its own functions are directly reachable. Two routes:
1. Call or hook the title's script/native layer. Its script runtime has a table mapping native identifiers to
   functions (set position, create vehicle, give item, set time, set weather are normally natives). Find the
   dispatcher; then (a) call natives from our side, (b) wrap them to log arguments, which is also the main way
   to discover the rest.
2. Write world state directly (position, clock, weather) only where no clean function exists. Weaker: it
   bypasses streaming and physics invariants.
Calls must be made on the game thread between its updates, never from our overlay: the menu queues a request
and a hook on the title's per-tick update executes it.

### B2. What must be found first
1. A safe once-per-tick hook point on the game thread.
2. The native dispatcher, its identifier-to-function table, the calling convention and argument layout (our
   loader translates the console ABI; `victory-loader/src/thunks.rs` is the existing facility to reuse).
3. How to obtain the current player entity, its position setter, and the "request collision/streaming around a
   point" call a teleport needs before the move.
4. The model-streaming API (request, poll loaded, create, release); a vehicle or item cannot be created until
   its model is resident. The name hashing function.
5. Clock and weather setters, and whatever re-applies them each frame.
6. Game states in which calls are unsafe (loading, cutscenes, pause).
7. Valid name catalogues (vehicles, items, weather) taken from the title's own data we already read.

### B3. Risks
- Version fragility: every title patch moves code. Find functions by signature scans, verify at load, and
  disable Part B if a scan fails. Keep discovered facts in a per-build table keyed by the executable hash, not
  compiled in.
- Crashes from wrong thread, unloaded model or stale handles. Our fault handler only takes write faults
  (CLAUDE.md), so a read fault inside a hooked call kills the title: validate every pointer first.
- Hooks must respect how the loader treats code pages and translated code.
- Save corruption and online modes: off by default, behind an explicit flag, never active online.
- Measurement integrity: spawning changes draw counts; mark the frame line when world tools were used.
- Keep notes and identifiers private and never name the title in repo text.

### B4. Effort (Part B)
Discovery dominates and is unbounded. Hook point plus dispatcher 1-2 weeks; teleport about 1 more; spawn
vehicles and items 2-3; time and weather about 1. Roughly 6-8 weeks to something usable on one build, high
variance, plus a recurring cost per title update.

## Build order
1. A1 input isolation.
2. A2 overlay engine and stats panel (replaces fpsbox at the same call site).
3. A6 capture key.
4. A3 switch registry: live-safe class, then boundary-latched; the rest read-only.
5. A4 skip picker.
6. A5 viewer v1, then v2.
7. Part B last, reusing the menu for input and text: B2 discovery first (hook point, dispatcher), then
   teleport, time and weather, then spawning.
