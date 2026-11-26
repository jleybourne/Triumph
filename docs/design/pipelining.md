# Frame pipelining: N frames in flight

Design only. Nothing built or measured here. Tree read: `wt-bisect` (queue.rs `present_behind`/`send_behind`/`Held::Present`, host.rs `late*`, `settle`, `late_helper`, `Presenter`, `BUFFERS`, `present_now`). Numbers: [M] measured (from the task brief), [E] estimate.

## 1. Where the time is

Frame ~127 ms [M]: title render thread 6 ms work + ~68 ms in present; submit thread at work 74 ms; card idle ~44 ms (so busy ~83 ms of the frame including waits for our copies; research-60-120.md puts card work ~71 ms). The title's recording phase (other threads, ~53-57 ms, queue.rs comment about run 225) and our submit phase (74 ms) run one after the other: 53 + 74 = 127. That is the whole serialisation.

Pipelined, the frame becomes about max(title phase, submit phase, card phase) = max(~55, 74, ~71) [E]: **~75-80 ms**, i.e. the submit thread becomes the wall. Everything below the submit thread (UMA, copy cuts) then lowers the wall; pipelining only removes the sum.

## 2. What exists and what it assumes

- `late` (flights): submissions go to the card without a per-submission wait; `settle` waits, copies back, applies bottom-of-pipe writes, then signals title fences in order. `late_helper` settles when the card has run everything (`host.fence`, one host-wide timeline, `FINISHED`).
- `behind` (ASYNC_SUBMIT): our thread prepares and sends; the title's call returns once the submission is in the line.
- `present_behind` (ASYNC_PRESENT): the present is a `Held::Present` in the line; the title returns at once. Limit today: ONE frame ahead (`PRESENTS_HELD != 0` -> old path), and the copy snapshot is "title memory stays as it was until the fence".
- Swapchain: FLIP_DISCARD, 2 buffers, `Present(0,0)`, no waitable object, no tearing flag. The back-buffer copy (`ahead` lists, `execute_own`) already waits on nothing but the in-order queue.

So the work is mostly making "one frame ahead" into "N, safely", plus pacing.

## 3. Real GPU completion fences per frame

Today one monotonic host fence + `FINISHED` counter. Pipelining needs per-frame identity on it, not new objects:

1. **Frame ticket.** At each `send_behind` (and for non-present frames at the title's frame boundary) record `Frame { n, last_flown, back_slot, snapshot_gen }` where `last_flown` is the host-fence value of the last submission/present copy of that frame. Ring of N (2-3) in queue.rs next to `PRESENTS_HELD`.
2. **Completion** = `host.fence.GetCompletedValue() >= last_flown`, observed by `late_helper` (already waits with `SetEventOnCompletion`). Never infer completion from "Present returned" or "list closed": with FLIP_DISCARD Present returns before the copy ran.
3. **Retire in order.** `settle` already applies writes and signals in order; extend it so retirement is per frame: when frame n retires, (a) its title-visible signals and bottom-of-pipe writes are published, (b) its back-buffer slot, host lists (`ahead[i].1 <= finished`), upload/scratch ranges and snapshots are released, (c) the frame slot is free for admission.
4. **Admission** (back-pressure): the title's present (or our `hold`) blocks only when N frames are un-retired. N=1 today in effect; N=2 first, N=3 only if staged measurement shows the card idle between frames.
5. Debug: one `GetCompletedValue` read per retire compared with the ticket; log if completion is out of order (it must not be: one queue).

## 4. Title-visible fence and event semantics

What the title can wait on (a fence signalled via `Held::Signal` -> `fence::set`, a bottom-of-pipe memory write, a polled memory value, or an event-based wait `SetEventOnCompletion`-style) means one thing on the console: "the card has finished the work before this signal, memory is written". Rules:

- **Signal only on real completion** of everything before it in that queue (what `settle` does; `late_signal` sets behind flights). Never signal at submit time to "let the title run on". This is the ONLY thing that keeps the title from rewriting memory the card still reads (see section 6), and it is the same contract the console gives.
- **Cross-frame fences the title waits on to reuse a ring entry** (constants, dynamic VB, per-frame command memory, query heaps): the title waits "frame n-K done" before writing slot n. With real completion fences this is automatically correct for any N as long as K >= N. If the title uses K=2 and we run N=3, the title's own wait protects us; if the title uses K=1 (waits for the previous frame) our N=2 is already the maximum useful.
- **Present's own fence/event.** If the title treats Present as "frame submitted, you may reuse the previous frame's resources" (the console model: swap returns, the card runs later), N frames ahead is allowed up to the title's own K. If the title blocks on a fence we have not signalled yet, `settle_for_fence` (WHY_FENCE) must still force; keep that path (point 5 of the `late` list). Count it: forced settles per frame are the metric for "pipelining is being defeated".
- **Block-without-event waits** (title spins on a fence value or memory): need the helper thread poke already in `late_helper`; make sure frame-retire also pokes (`LINE_MOVED`).
- **Never signal early for latency.** Signalling n's fence before completion is exactly the lost-write hazard.
- **VICTORY_LATE_WAIT with UMA**: incompatible at level 2 (uma.md); frame pipelining under UMA must use stage 3 plus real fences per section 3.

## 5. Swapchain

| item | change | why | cost/risk |
|---|---|---|---|
| Buffers 2 -> 3 (`BUFFERS`, also `ResizeBuffers` call, one place) | needed once frames overlap: with 2 flip buffers a present waits for the buffer the compositor still holds, and the next copy must wait for the previous present's buffer. 3 gives the copy of frame n+1 somewhere to go. | +1 x 4K back buffer (~33 MB at 8-bit, ~66 MB FP16); lets pipelining reach N=2 without present stalls. |
| Waitable object: `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT`, `SetMaximumFrameLatency(L)`, `GetFrameLatencyWaitableObject` | The pacing knob. Wait on the handle on the thread that does the back-buffer copy (our submit thread, NOT the title's thread) before `GetCurrentBackBufferIndex`. L=2 with 3 buffers; L=1 only for lowest latency. | Must be set at creation and kept in `ResizeBuffers` flags (flags must repeat or ResizeBuffers fails; today it passes 0, change both). Waiting on it on the title thread would re-serialise. |
| `DXGI_PRESENT_ALLOW_TEARING` + `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING` | `CheckFeatureSupport(DXGI_FEATURE_PRESENT_ALLOW_TEARING)` first; only valid with `Present(0, ALLOW_TEARING)` in windowed/borderless flip. Removes vsync re-serialising a pipelined frame and shows >60 on a 60 Hz panel. | Visible tearing is by design; keep a switch. Flag must also be passed to `ResizeBuffers`. Not valid in exclusive fullscreen. |
| Latency | with waitable + L=2 the displayed frame is ~2 frames behind the title's input; today ~1-2 anyway [E]. | |

`Present(0,0)` today with no waitable means the driver queues up to its own default (3 frames in the OS) blindly; the waitable object replaces that with a bound we choose.

## 6. Title-memory snapshots: the hard part

Frame n+1's recording (title writes constants, vertices, command memory, indirect args, mapped upload memory) runs while our thread still prepares/sends frame n's submissions, and while the card still runs them.

### 6.1 Today's copy model (shadows)

Our thread reads title memory at *prepare* time (`Shadow::update`, `refresh`, `copy_stale`, scratch fill) and the card reads our host copies later. So the question is: when we copy bytes for submission of frame n, can the title already have written n+1's bytes into the same addresses?

- On the console the card reads title memory when it *runs* the list, so a correct title never rewrites memory used by frame n until frame n's fence. Our prepare happens before the card runs, so at prepare time the bytes are those of frame n, provided the title honours the fence (K waits) — and it does, because we signal only on completion (section 4). So **correctness rests on real fences, not on our copy timing.** Prepare may run any time between the title's write of n and the title's write of n+K.
- What breaks it: a title that rewrites a slot after waiting on a fence that we signalled too early (section 4), or memory the title treats as free after Present returns (swap-return semantic) that we have not copied yet. Defence: **copy-at-record snapshot** only for ranges flagged by the hazard detector (6.3), not by default.
- Under copy model, `copy_back` merges shader writes into title memory. With frames overlapped, copy-back of frame n lands while the title is writing n+1 into possibly the same pages (e.g. a ring the shader writes and the title also writes). The merge uses `changed_chunks` against `before`: a title write after prepare but before merge is lost if merge writes the whole chunk. Rule: **copy-back only writes chunks whose title bytes equal the `before` snapshot taken at prepare (compare-and-swap per chunk)**; a chunk the title changed in between is a conflict, kept as the title's and counted. This is what uma.md calls the stale-view hazard in the other direction; reuse `changed_runs`.
- Frame-tagged snapshots: for pages the detector flags, keep `Snapshot{frame, bytes}` for frames in flight (ring of N), released at retire. Memory: only for flagged ranges (constants/rings, typically < tens of MB) [E].

### 6.2 UMA (zero copy, tier S live reads)

The card reads title memory at execution time, so there is no snapshot at all: frame n's lists read whatever is in the pages when the card gets to them.

- **Mandatory**: the title may rewrite a page only after frame n's fence. Real completion fences (section 3, 4) are then the only protection. Early signalling, `VICTORY_LATE_WAIT` stage <3 forced signalling, or "present returns -> memory free" breaks it silently (wrong frame, partially updated constants -> flicker/garbage).
- **Ring-slot rule**: for each S-tier buffer record the last frame that read it (`last_use_frame`, set when a list binds the Region). Title write fault/notify (write watch already exists for shadows; under UMA the write-watch API remains for diagnostics) checks `frame(last_use) > completed` => hazard event. Production path: no check, just the fences.
- **C tier** (card copies): same as copy model but chunk hashes already handle it (uma.md section 5); the CPU->card refresh for frame n+1 must go on the queue AFTER frame n's reads of that buffer. In-order queue gives this when the refresh is a GPU copy from an upload ring; the upload ring itself needs N slots (one per frame in flight), retired by section 3.
- **Opened heaps** (`title_memory_freeing`): wait for the fence of `last_use_frame`, not "all flights".
- Triple-check: title command-processor writes polled by the title (S tier by design) are visible only after our signal: signal on completion, again.

## 7. Hazards and how to detect them

| hazard | symptom | detector (in-run, cheap) |
|---|---|---|
| Fence signalled before the card finished | wrong frame, garbage, one-frame-old data in a ring | at retire compare ticket vs `GetCompletedValue`; per signal log `signalled_value <= completed` ; count violations (must be 0). Debug layer + GPU-based validation in a one-off run. |
| Title rewrites memory still read (UMA) | flicker, constant-buffer noise, "previous frame" objects | tag `last_use_frame` per Region, compare at title write notifications; sampled hash of 4 constant slots at prepare vs at card-run (copy a few words into a readback ring); a counter `reused_under_flight` |
| Copy-back overwrites title's newer bytes | lost writes, objects pop | chunk compare-and-swap counter `backmerge_conflicts` per frame; any nonzero under pipelining is reported |
| Present race (back buffer reuse) | tearing-like flashes, same frame twice, ordering | per-present frame number stamped into the corner via `fpsbox` (frame mod 16 as colour) and log the sequence at retire; frames out of order or repeated is visible in framelog |
| Waitable-object deadlock / starvation | frame time spikes ~ latency, hang | waits with timeout (e.g. 100 ms) + log; fall back to non-waiting |
| Back-pressure starvation / deadlock | title blocked in present while our thread waits for the title (e.g. title waits for a fence only a later frame signals) | forced-settle counter (`WHY_FENCE`, `WHY_SYNC`) per frame; watchdog: if the line has not moved for 1 s, force settle and log which signal blocked |
| Device removal / TDR | `DXGI_ERROR_DEVICE_REMOVED` in Present or `GetDeviceRemovedReason`; more GPU work in flight raises the exposure | already: Present result logged once; add removal reason + `frames in flight` + last ticket to that log; DRED enabled in the diagnostics build |
| Memory pressure (3 buffers, N upload rings) | `log_video_memory` budget low | existing log lines before/after the swap chain; check budget at N=3 |
| Capture/diagnostics frames | forced old path | already excluded (`capture::wanted`); leave |
| Resize mid-flight | ResizeBuffers E_INVALIDARG (buffers held) | `settle_now(WHY_SYNC)` and drain all N tickets before ResizeBuffers |

Wrong frames are the silent class: add the stamp and an optional "paranoid" mode (`VICTORY_PIPE_CHECK=1`) that, every k frames, runs the frame with N=1 and compares a hash of the presented texture.

## 8. Staged plan (each behind a switch, measured in-run with `alt`)

All switches take `1` or `alt` (20 s on / 20 s off in one scene, logged, like `VICTORY_ASYNC_PRESENT=alt`). Every stage reads: fps, ms per frame, title-present wait, submit-thread work, card idle, forced settles, conflict counters, dropped.

0. **Baseline of `VICTORY_ASYNC_PRESENT=alt`** (running). Expect [E] 127 -> ~85-95 ms with one frame ahead, since the title's present wait (68) disappears but the single-frame-ahead guard and the 2-buffer swapchain still stall. Gate for everything below. Log new counters first (cheap): forced settles per frame, back-merge conflicts, present-ahead refusals (`PRESENTS_HELD != 0`).
1. **`VICTORY_PIPE_FENCES=1`**: frame tickets, per-frame retire, completion-checked signals, detectors of section 7 (no behaviour change except retire timing; alt shows it costs ~0 ms, [E] <1 ms). Gate: zero out-of-order violations.
2. **`VICTORY_PIPE_BUFFERS=3`** (swapchain 3 buffers + waitable + `SetMaximumFrameLatency(2)`, wait on our submit thread; `BUFFERS` read from the switch, `ResizeBuffers` flags repeated). [E] 0 to -5 ms (removes present stalls), -10 to -30 ms latency; verify with presented-frame stamp spacing.
3. **`VICTORY_PIPE_AHEAD=2`**: lift the `PRESENTS_HELD` one-frame guard to N=2 tickets with admission by retire. Copy model, with copy-back compare-and-swap. [E] frame ~127 -> ~80-90 ms (max of title ~55, submit 74, card ~71 plus contention [E]); title present wait 68 -> ~10 ms [E]. Watch `backmerge_conflicts` and flicker.
4. **`VICTORY_PIPE_TEAR=1`** (`ALLOW_TEARING`): only when vsync/compositor shows as the limiter (frame time quantises at 16.7 ms steps). [E] 0 ms throughput, removes 0-16 ms jitter on a 60 Hz panel.
5. **`VICTORY_PIPE_AHEAD=3`**: only if card idle between frames > 10 ms after stage 3 (3 buffers, L=3). [E] 0 to -5 ms; more latency; skip if no gain.
6. **UMA + pipelining** (after uma.md steps): same switches, with `last_use_frame` hazard check on. Submit thread 74 -> ~15 ms [E, uma.md] makes the title phase (~55) the wall: frame ~55-60 ms [E], card ~45-50 [E]. At that point the title-side CPU (THIN, call cost) is the lever.

Expected summary [E]: 127 -> ~85 (stage 3) -> ~75 (stages 2+3 tuned) -> ~55-60 (with UMA). These are estimates from max-of-phases and carry the uncertainty that submit and title phases contend for CPU (14 logical cores pinned by the title, research #7).

## 9. Order of risk

Fences-first (stage 1) is the load-bearing change: it makes both the copy model and UMA safe, and costs nothing visible. Do not raise N before a run with zero `signalled_before_complete` and zero `reused_under_flight` events. If any shows up, fall back to N=1 for that frame (ticket flagged) and log, rather than guessing.
