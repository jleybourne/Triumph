# Wait hunt: what still keeps the submit thread and the card from overlapping

Phase 1 of the plan to 30 fps: the frame should cost max(CPU, card), not their sum. This lists every remaining place the submit thread waits for the card or the relay mid-frame, and every reason fewer than 2 frames are in flight, with ms per frame, then the fixes built on branch `wait-hunt` (worktree `D:\victory-work\wt-waits`, from `fast-integ` 00552a92).

Sources: the logs of runs 25–28 (build 00552a92, 08:01–08:36; each one an `alt` A/B whose **off** phases are the control), run 19 (`victory-run-20261127-041155`, ordered copies `=alt`, build 2261b969), and run 8 (`victory-run-20261127-004949`, `VICTORY_PIPE_AHEAD=alt`, older build). Parser: `D:\victory-work\gap-model\waits.py LOG...` averages every wait figure over the stats windows wholly inside an ON or off phase (the first window after a flip is dropped). All figures are ms per presented frame unless marked.

## 1. The control frame, per thread (runs 25–27, off phases)

Run 28 is left out of the means: it ran ~60 % slower in both phases (frame 237 ms, relay landing 81 ms, remapping 157 ms), which points to the session rather than its switch. Its rows are in the per-run table (section 6).

| | run 25 | run 26 | run 27 | mean |
|---|--:|--:|--:|--:|
| frame (frame-times mean) | 112.7 | 127.8 | 134.3 | **125** |
| usual frame (not long) | 95.1 | 116.5 | 114.1 | 109 |
| **submit thread** | | | | |
| idle between submissions (waiting for the title) | 23.5 | 18.5 | 20.9 | **21.0** |
| CPU inside submissions | 57.8 | 70.8 | 75.7 | **68.1** |
| before preparing ("lock" on the per-thread line) | 31.9 | 39.4 | 37.9 | **36.4** |
|  of it: settle for "title memory read" (relay wait) | 20.3 | 25.8 | 25.2 | **23.8** |
|  of it: `sync_heaps`, sweep, the mutex (CPU, no card) | 11.6 | 13.6 | 12.7 | 12.6 |
| **relay thread** | | | | |
| landing (fetch, compare, merge, then signals) | 34.8 | 43.3 | 46.6 | 41.6 |
| waiting for fetches of card copies' pieces | 6.8 | 8.9 | 9.0 | 8.2 |
| idle, waiting for the card with nothing to land | 32.6 | 38.1 | 37.9 | 36.2 |
| **card** | | | | |
| idle (of the usual frame) | 50.1 | 56.9 | 58.8 | 55.3 |
|  of it while the submit thread was working ("behind us") | 30.1 | 36.7 | 39.2 | 35.3 |
| title lists + our send/compare lists | 43.6 | 56.7 | 52.7 | 51 |
| **title render thread** | | | | |
| in the present call / ticket wait / paced | 2.9 / 0.5 / 7.0 | 3.2 / 0 / 2.4 | 2.8 / 0 / 2.5 | 3.0 / 0.2 / 4.0 |

The card is busy ~51 ms of a ~109–125 ms frame; the submit thread's own CPU is ~68 + ~13 = ~81 ms. So max(CPU, card) ≈ 81 ms, and the frame is 109–125: **28–44 ms are waits**, and they are the two waits in bold below.

## 2. Every remaining mid-frame wait of the submit thread

| # | wait | count / frame | ms / frame (control) | ms / frame (ordered ON, run 19) | why |
|--:|---|--:|--:|--:|---|
| W1 | relay settle, **title memory read, by a title buffer copy** | 0.7 | ~13–14 (of 23.8) | 0 (moved) | The copy `0xdc07ea000 → 0xd1f3xx000 + 0x800` reads the 2 KiB block the card writes every frame (`fill_check`: its source had other bytes after the wait 410 of 647 times). A true dependency. Ordered copies remove it. |
| W2 | relay settle, **title memory read, by a fill of upload memory** | 0.6 | ~10 (of 23.8) | 0 (moved, at +19 ms CPU) | **Not a dependency.** `fill_ahead` lets fills read ahead of flights, and turns the waiting on *for every fill, for good* at the first miss. In every run (19, 25, 26, 27, 28) that first miss is one 2 KiB fill at `0xdc07ea300` (`0xdc13eab00` in run 28), inside the same card-written block as W1, within the first minutes of the world. 632 fills that waited were compared after their wait; **0** read other bytes. With ordered copies the same global state makes `ordered::plan` move every fill of the 174 MB buffer onto the card (2,300–7,070 a frame), which is the +19 ms of submit CPU section 9.2 measured. |
| W3 | relay settle, lists in the air ≥ 256 (`WHY_MOST`) | — | 0 | **4.7** | Shows once W1/W2 stop draining the line; doubles with 2 frames in flight. |
| W4 | relay settle, sweep's forced settle (every 1.5 s with presents sent ahead) | ~0.1 | 0.0 | 0.3 | Drains the line once per ~10 frames. Small. |
| W5 | settles for a memory value, stale title memory, step of ours, fence, bottom-of-pipe writes | 0 | 0.0 | 0.0 | All zero in every run. |
| W6 | idle between submissions (the title has not submitted) | 7 gaps | 21.0 | 87.5 (older build) | Not a card wait: the title records. The tail from our last submission to the title's present call is ~25 ms on its own (measured from the frame account, partly overlapping this figure). Title fence waits (mwaitx) are only ~41–54 ms a **second** (~5–7 ms a frame), and the title's fences are set ~100 ms after its Signal, mostly in a later frame, i.e. it is already about one frame ahead. |
| W7 | waits for the copy queue's fetches of card copies' pieces | 7 jobs | 8.2 (**relay thread**, not the submit thread) | 6.2–7.2 | Lengthens each landing, so W1/W2 and the title's fences come later. |

The submit thread itself has no other card wait: "waiting for the card per frame" lists 0 synchronous waits in every bucket, the present's own settle ("a synchronous submission or present") is 0, and card-copy fetches under the submission lock are 0 with the relay on (the 5–6 "more waits for the card" on the host-copies line are W7, on the relay).

### Run 19, ordered copies ON against off (older build, same session)

| | off | ON |
|---|--:|--:|
| settles / frame | 1.0 | 0.2 |
| settle ms: read / lists / sync | 31.5 / 0 / 0 | 2.9 / 4.7 / 0.3 |
| submit thread: between submissions / CPU in them / before preparing | 55.3 / 52.6 / 43.6 | 87.5 / 67.9 / 45.1 |
| relay landing | 40.6 | 68.7 |
| fills moved onto the card | — | 2,300–7,070 |

With W1 and W2 gone, the wait did not disappear; it moved into the submit thread's CPU (+15–19 ms) and the gaps between submissions (+32 ms), and into the relay's landing (+28 ms; the moved fills' destinations come back with the copy-back).

## 3. Why fewer than 2 frames are in flight

| # | reason | effect per frame | where |
|--:|---|---|---|
| F1 | `VICTORY_PIPE_AHEAD` not set in runs 17–28 | at most 1 present held in a queue's line (`PRESENTS_HELD` guard) | config |
| F2 | W1 + W2: each "title memory read" settle waits until every flight declaring that memory has landed. Every flight declares the whole 174 MB buffer (the title has an unordered view over all of it), so `relay_exact` waits for **everything** in the air | 1.4 drains a frame: 0 flights in the air in the middle of every frame; frame n must finish (card + landing) before frame n+1's second half is prepared | `host.rs` `late_blocked` / `relay_wait` |
| F3 | W3: 256 lists in the air | 4.7 ms with ordered ON at 1 frame; at 2 frames the lists double | `late_most` |
| F4 | W4: sweep settle every 1.5 s | one drain per ~10 frames | `Host::submit` |
| F5 | run 8 (`PIPE_AHEAD=alt`, older build): "at most 1 in flight" in nearly every 64-frame summary of the ON phases, with 0 admits refused: the drains of F2 empty the tickets before the next present | F2 | `pipefence` summary |
| F6 | admission: a present waits up to 100 ms on the title's thread for an older frame, then runs the old way | not seen (no run with admit counts on this build) | `pipefence::admit_present` |

So the order is: remove W2 (no dependency at all), keep W1 on the card (ordered copies, batched on `ordered-2`), then lift F3 so two frames' lists fit, then turn on `VICTORY_PIPE_AHEAD=2`.

## 4. Fixes built (branch `wait-hunt`)

### 4.1 `VICTORY_FILL_AHEAD_PIECES=1|alt`: a missed fill makes only its own pieces wait

`host.rs` `fill_ahead`. Without the switch nothing changes (the first miss sets `WAIT` for every fill). With it:

- A fill found read ahead of a flight that changed it marks the 64 KiB pieces it read (`Hot`, sorted piece numbers). Fills and title copies (that only read) in a marked piece wait as before; every other fill keeps reading ahead and is still checked when the flights land. A new miss marks its pieces too; past 16 pieces every fill waits, as before.
- `may_read(src, len)` replaces `fill_ahead::on()` in `blocked_by` for fills and for title copies that only read.
- With `VICTORY_GPU_ORDERED_COPY` on too: `ordered::plan_ahead` takes `ahead(i)`, the steps the pieces allow to read on the CPU. Such a step no longer counts as "reads what the card writes" and stays on the CPU, unless it reads what a moved copy writes (that rule is untouched: it moves with the copy, or nothing moves). Steps in a marked piece move as before. So ordered copies keep the W1 copy and the fills of the hot block on the card, and the thousands of per-draw fills elsewhere in the 174 MB buffer stay on the CPU. With the switch off, `plan` is called with `ahead = false` and moves exactly what it did.
- `alt`: twenty seconds with, twenty without; in the off phases every fill waits once a miss was seen, as today, so the A/B is clean.
- Risk: each new piece costs what the first miss costs today (one submission drawn with the bytes from before); every miss is logged as a warning naming the bytes and the pieces marked; the fill-ahead line says how many pieces are marked.

Expected:
- ordered copies off: W2's ~0.6 settles a frame go; ~5–10 ms of the 23.8 (the copy's settle that remains then finds more flights to wait for). The copy settle keeps draining the line (F2).
- ordered copies on (`=1`): fills moved per frame fall from 2,300–7,070 to those in the hot piece plus those reading the moved copy's 2 KiB destination. Most of the +15–19 ms of submit CPU and of the +28 ms relay landing should go. It is complementary to the batched scatter on `ordered-2`, which then batches only what is left. If the moved count stays in the thousands, the moved fills read the moved copy's destination (the cascade section 9.3 assumed), and only `ordered-2`'s batching helps.

Measure: the ordered line's "fills moved" and "waits for title memory read"; the submit thread's per-thread line (CPU inside submissions, before preparing); the settle line's "a fill of upload memory"; the relay line's landing ms; and how many warnings "pieces marked" appear (should stay 1–2).

### 4.2 `VICTORY_LATE_WAIT_LISTS=alt`: 512 lists in the air in turn with 256

`host.rs` `late_most`. A number works as before (default 256); `alt` alternates 512 and 256 every twenty seconds and says so in the log. It targets W3/F3: run 19's ordered phases settled 4.7 ms a frame at 256 lists, and two frames in flight hold twice the lists. Watch "tables asked, N refused" on the recording line (dynamic descriptor slots, run 45) and the video-memory line. Use it with ordered copies on; at 1 frame in flight without them it does nothing (0 ms of W3 in the control).

### Not built, and why

- W1 itself: ordered copies (`VICTORY_GPU_ORDERED_COPY`), batched on `ordered-2`. Not redone here.
- W4 (sweep, 0.3 ms): too small to be worth the risk now. When `PIPE_AHEAD=2` runs, check "a synchronous submission or present" on the settle line; if it grows, let the sweep keep idle objects only of retired tickets instead of draining.
- W6: the title's own recording time, not a card wait. It shrinks only when the title is let further ahead (F1, `PIPE_AHEAD=2`).
- W7: the relay's fetch already overlaps later submissions (`inflight_fetch`, ~1,000 pieces a frame fetched beside them). The residual 8 ms is the copy queue's own run time.

## 5. Runs to make

Base: run 25's environment (the control lines of `run25_ticket.ps1`, without its own `alt`).

1. **Pieces alone**: base + `VICTORY_FILL_AHEAD_PIECES=alt`. Look at: settles by "a fill of upload memory" (0.6 → ~0), "title memory read" ms, the submit thread's before-preparing ms, the fill-ahead line's pieces marked.
2. **Pieces on top of ordered copies**: base + `VICTORY_GPU_ORDERED_COPY=1` + `VICTORY_FILL_AHEAD_PIECES=alt`. Look at: "fills moved" per frame, the submit thread's CPU inside submissions, relay landing. This is the run that says whether the per-fill cost goes away without batching. (On `ordered-2`, after a merge, the same pair.)
3. **Two frames**: base + `VICTORY_GPU_ORDERED_COPY=1` + `VICTORY_FILL_AHEAD_PIECES=1` + `VICTORY_LATE_WAIT_LISTS=512` + `VICTORY_BACK_CAS=1` + `VICTORY_PIPE_FENCES=1` + `VICTORY_PIPE_BUFFERS=3` + `VICTORY_PIPE_AHEAD=alt`. Look at: the pipelining summary's "at most N in flight" and "admitted with one held", settles per frame (should stay ≤0.2), and the card idle "behind us".
4. Optional: run 3 with `VICTORY_LATE_WAIT_LISTS=alt` instead of 512, to price W3 directly.

## 6. Per-run figures (off / ON of each run's own switch)

| figure | 25 ticket | 26 wide-prep | 27 guard-epoch | 28 upload-heap |
|---|--:|--:|--:|--:|
| frame (frame-times mean) | 112.7 / 130.0 | 127.8 / 136.2 | 134.3 / 137.9 | 237.5 / 232.9 |
| submit: between / CPU in / before preparing | 23.5 / 57.8 / 31.9 · 15.3 / 72.6 / 40.1 | 18.5 / 70.8 / 39.4 · 32.2 / 70.2 / 37.5 | 20.9 / 75.7 / 37.9 · 21.8 / 78.1 / 39.5 | 39.2 / 107.2 / 70.4 · 41.1 / 110.0 / 72.2 |
| settle "title memory read" ms (copy / fill settles) | 20.3 (0.7/0.5) · 27.0 (0.8/0.8) | 25.8 (0.8/0.7) · 21.8 (0.9/0.5) | 25.2 (0.7/0.7) · 25.7 (0.9/0.7) | 34.2 (0.7/0.7) · 38.5 (0.6/0.7) |
| relay: landing / fetch wait / idle | 34.8 / 6.8 / 32.6 · 44.1 / 9.1 / 43.0 | 43.3 / 8.9 / 38.1 · 46.2 / 9.3 / 36.6 | 46.6 / 9.0 / 37.9 · 47.6 / 9.4 / 38.7 | 80.9 / 12.0 / 38.7 · 83.0 / 12.2 / 40.2 |
| card idle (behind us) | 50.1 (30.1) · 54.1 (36.7) | 56.9 (36.7) · 53.1 (33.9) | 58.8 (39.2) · 58.7 (39.5) | 126.6 (89.4) · 120.5 (87.9) |
| last submission → present call | 22.9 · 20.7 | 27.2 · 35.6 | 25.7 · 31.7 | 41.6 · 57.4 |
| fill-ahead tripped (first miss) | 0xdc07ea300, frame 2260 | 0xdc07ea300, frame 1662 | 0xdc07ea300, frame 1510 | 0xdc13eab00, frame 1714 |

## 7. State

Commit `174bacaa` on `wait-hunt` (from `fast-integ` 00552a92), not pushed. `cargo test -p victory-gpu --lib` (target `target/vwh`, LTO off, 16 units): 365 passed, 0 failed, 5 ignored; new tests: `host::fill_ahead::tests::{the_pieces_switch_reads_one_and_alt, a_miss_marks_only_the_pieces_it_read, past_the_most_pieces_every_fill_waits}`, `host::late_most_tests::the_lists_switch_reads_a_number_or_alt`, `ordered::tests::{steps_that_may_read_ahead_stay_on_the_cpu, reading_ahead_never_skips_what_a_moved_copy_writes}`. No diag-profile exe built and nothing run in the game. Merging with `ordered-2`: `plan_ahead` sits beside `plan` in `ordered.rs`, and the only call site change is in `order_on_card` (host.rs), so the batched scatter should merge with a small conflict there at most.
