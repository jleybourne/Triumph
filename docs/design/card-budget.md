# Card budget: what the graphics card has to lose for 30 fps (33 ms)

Design only. Nothing built or run for this document. Tree read: `wt-bisect` at `2261b969`
(fast-integ). Labels: **[M]** measured in a committed log, **[C]** read in code, **[E]** estimate
with the reasoning given, **[?]** unknown until the named run.

## 0. Summary

- The card does **~80 ms of work per frame** today, not 46. The "46 ms GPU busy" in run 34's row
  is only the title's lists. Our own hash and compare lists add another 34 ms on the same queue.
- **Most of our 34 ms is latency, not bytes.** Each hash pass gives one thread a whole 64 KiB piece
  to walk serially. At ~480 hash passes and ~395 compare passes a frame, every pass costs ~50 µs
  however little it hashes. The switch that fixes this already exists and has never been run in
  the game: `VICTORY_CARD_HASH_WIDE=1`. Estimated saving: **−24 to −30 ms**. It is the best
  ms-per-effort item on this list by a wide margin.
- After that, the title's 46 ms is the wall. About 6 to 12 ms of it is ours: clamp passes and
  full waits around indirect commands, recorded inside the title's lists.
- **Verdict:** 33 ms of card time is reachable with RT lighting off (as today). It needs the hash
  fix, the indirect work, the translator cuts, and **one** of a lower render resolution or async
  compute. With RT lighting back on (the full-world goal), 33 ms is **not** reachable on the card
  alone unless RT emulation track B lands *and* the render resolution drops. Section 4 has the
  arithmetic.

## 1. Where the card's time goes (run 34, the control)

Source: `D:\victory-work\logs\victory-run-20261127-002039.log.err` (`confirmed.out`, the control
on the merged build `17196a3c`). I averaged the steady-state `framelog: usual frame` lines: 58
five-second windows with ≥150 title lists and ≥5 of our send-and-hash lists, so the load phase is
left out. The card columns come from the card's own clock (`cardmeter.rs`: one timestamp pair per
list, read 1 s later).

| group | passtime phase | cardmeter kind | ms / frame [M] | lists | notes |
|---|---|---|---:|---:|---|
| our send-and-hash (uploads to card copies, then rehash of the pieces sent) | **Sync** | `Sync` | **28.9** | 5.9 | 473–485 hash passes over ~1,400 pieces a frame (`host` line "our passes over copies in the card's memory") |
| our compare after a submission | (inside **Draw**; passtime has no phase of its own for it) | `Check` | **5.5** | 5.8 | 393–395 compare passes a frame |
| the title's lists, with our in-list additions | **Draw** | `Lists` | **46.0** | 176 | includes ~2,050 clamp passes bounded by full waits, 706 global UAV barriers for the title's barrier groups, and our present copy |
| copy-back | **CopyBack** | n/a | 0 card (38.8 ms CPU) | n/a | CPU only |
| **card busy** | | | **80.4** | | |
| card idle | | | 40.3 (23.2 of it "behind us") | | |
| frame (usual, diagnostics build) | | | 124.5 | | |

Notes:

- `VICTORY_PASS_TIME` has never printed in a run (run 20: nothing in 109 frames). The cardmeter
  kinds above stand in for its phases. Making passtime print, with Check as a fourth phase, is a
  small instrument job (section 5).
- The same split holds across all 24 runs of 26–27 November: title 43–54 ms, ours 26–37 ms.
  Run 280 (DRED plus per-list timing) showed ours at 77–83 + 15.5 ms. That inflation was the
  instrument's.
- **With zero-copy step 1 on** (`NO_COPYBACK=1`; runs 35 and 42, logs `…-002915` and `…-025612`),
  ours falls to **14.7 + 4.4 = 19.1 ms**: 315 hash passes over 523 pieces and 214 compares. Pieces
  fell by 63 % but hash time fell by only 49 %. So **per-pass cost, not bytes**, is the driver
  (~47–58 µs a pass in both runs).
- Title breakdown by pipeline (cardmeter, last 256-frame window: 55.4 ms including long frames):
  a flat tail. The largest kind is 5.45 ms (compute `03c01507…`), the next is 3.5 ms, and the top
  20 come to ~42 ms. Almost all of it is compute, plus one vertex/pixel set of 180 pipelines at
  2.5 ms and mesh at 0.85 ms. Run 280's per-command view agrees: `indirect` 28–31 ms over ~4,500
  commands (~6.6 µs each) and `dispatch` ~11 ms. The title's frame is bound by launches and
  barriers, not by pixels.
- **The 4K upscale** (draw `0309a446e1a48352` + `4f4ec7b16673362f`; a 2,000-byte pixel shader that
  reads tonemapped colour and depth) never appears in the cardmeter top lists (threshold
  0.56 ms). Its cost is **< 0.6 ms** [M, by absence]. Replacing it saves nothing by itself.
- **Hidden item: RT lighting.** ~336 of the ~340 dropped draws are the RT builder stage, which is
  off. Builder-on runs added ~40 ms of card time (PLAN.md). Track B estimates 15–20 ms. A
  full-world frame has to pay this, and today's 80 ms does not include it.

## 2. Why our hash passes cost so much [C]

`CARD_COMPARE` (host.rs ~2100) runs `[numthreads(64,1,1)]` with **one thread per 64 KiB piece**.
That thread loops 4,096 times over 16-byte loads with a dependent hash. Most passes hash 1–3
pieces (1,400 pieces / 480 passes), so a pass is one or two threads walking 4,096 dependent steps.
It occupies one SM slot for ~50 µs while the rest of the card idles, and passes are separated by
barriers. 480 × ~58 µs ≈ 28 ms. That matches the measurement.

`CARD_COMPARE_WIDE` (host.rs ~2162, `VICTORY_CARD_HASH_WIDE=1`) gives each piece a group of 64
threads with coalesced loads, 64 steps each, then a 64-step fold. Per piece that is ~64 + 64
dependent steps instead of 4,096, so the per-pass time falls to roughly a launch and barrier
cost: **~3–6 µs [E]**. 875 passes × ~4 µs ≈ **3.5 ms** for hash plus compare (range 2–6 ms).

## 3. Cuts ranked by ms per effort

The ms are card ms per frame. The frame only gains once the card is the limit (after the CPU work
in flight). Every item is verified the same way: a diagnostics build, a 600 s run, and `fps · ms ·
draws · dropped` together, read against the `framelog: usual frame` **card** columns (send-and-hash,
compare, all other lists), not against the frame time. While the CPU is the wall, a card cut shows
in those columns and not in fps. That is how `GATHER_BY_COPY` was wrongly "rejected" (run 30 judged
it on frame ratio). Use `alt` where a switch has it.

| rank | cut | est. card ms | effort | picture risk | switch | verify |
|---:|---|---:|---|---|---|---|
| 1 | **Wide card hash** (group per piece) | **−24 to −30** [E, from §2] | none: built, untested in game | low: a different hash with its own rare blind patterns; a miss means a skipped copy-back of a changed piece (stale title bytes) | `VICTORY_CARD_HASH_WIDE=1` (exists; `alt` exists) | send-and-hash 28.9 → <5 and compare 5.5 → <2 at the same pass counts; screenshots of water, boats and skinned characters; `copy-back` MB unchanged ±10 % (a large drop would mean misses) |
| 2 | **Measure the waits** (no code) | 0; gives the ceiling for 4–6 | none | none (timing only; may race) | `VICTORY_BARRIER_TEST=indirect` / `groups` / `both` (exist) | "all other lists" ms in each run against the control |
| 3 | **Re-judge `GATHER_BY_COPY` on card ms** | −2 to −5 [E, indirect-overhead.md row 1] | none: built | low: worst case a stale *clamped* record | `VICTORY_GATHER_BY_COPY=alt` (exists) | "stopped gathering" 1,096 → <300; "all other lists" down in the on phases; `VICTORY_INDIRECT_WINDOW_CHECK=1` once |
| 4 | **Clamp barrier merge + one-record clamp kernel** | −1.5 to −3 [E] | small (days) | low | `VICTORY_CLAMP_BARRIER_MERGE=alt`, `VICTORY_CLAMP_ONE=alt` (new; indirect-overhead.md rows 3, 5) | copystat waits line; "all other lists" |
| 5 | **`memory()` fault store under a branch** (translator) | −2 to −5 [E, native-60 C] | none: built, off; fresh translation (the cache key includes options) | low-medium: the fault path changes for missing pages only | `VICTORY_MEMORY_FAULT_BRANCH=1` (exists) | cardmeter per-pipeline ms of the top 20 compute kinds; fault-record counts unchanged |
| 6 | **Batch our hash/compare passes** (one dispatch per list over a table of (copy, piece)) on top of rank 1 | −1 to −3 [E: removes ~800 launches and barriers] | small-medium (week) | low (same hash) | `VICTORY_CARD_HASH_BATCH=1` (new) | send-and-hash and compare < 1 ms each |
| 7 | **Drop the card hash entirely**: modified-range lists (prior art, see §3.1) or zero-copy step 2 | −2 to −5 after rank 1 (−30 to −34 without it) | medium-high | medium: copy-back policy decides correctness | `VICTORY_CARD_MODIFIED_RANGES=1` (new) / `VICTORY_UMA_NO_HASH=1` (exists, needs `NO_COPYBACK`) | send-and-hash and compare columns → ~0; card-to-CPU readback MB; screenshots |
| 8 | **Dispatch windows** (batch dispatch clamps per barrier span) | −2 to −3 [E] | medium (a week) | low: every slot is still clamped | `VICTORY_INDIRECT_WINDOW_DISPATCH=alt` (new) | dispatch passes 952 → ~600; "all other lists" |
| 9 | **Lower render resolution** 2560x1440 → 1920x1080 | −5 to −12 [E, see note] | none to test (settings `fsrQuality` 1, 2); DLSS to make it look right is weeks | **high** with the title's spatial upscale (soft 4K); medium with DLSS | title setting `fsrQuality=1/2`; `VICTORY_DLSS=1` + `VICTORY_FSR_HASHES=0309a446,4f4ec7b1` (exist) | render-target sizes in the log; "all other lists"; screenshot pair at the same spot |
| 10 | **Scalar work marked uniform** (wave64 programs repeat the scalar stream per host wave) | −3 to −6 [E, native-60 D] | medium (offline S/V count first, then marking) | low | `VICTORY_WAVE_UNIFORM=1` (new) | S/V census of the top 35 kinds; per-pipeline cardmeter |
| 11 | **Async compute**: title compute-queue lists on a host compute queue | −3 to −8 [E: overlap of title compute with graphics; the title is compute-heavy] | high (weeks) | medium: fence translation; our Sync/Check lists assume one queue order | `VICTORY_ASYNC_COMPUTE=1` (new) | card busy (union, not sum) per frame; per-queue cardmeter; any hang means fence order |
| 12 | **Enhanced barriers for clamp scratch** (sync scopes instead of global UAV waits) | −2 to −4 [?] | medium | low | `VICTORY_CLAMP_BARRIERS=enhanced` (new) | as rank 4; only after rank 2 shows waits are over half |
| 13 | **Title barrier groups**: dedupe back-to-back global waits | −1 to −2 [E] | small | low (only dedupe; the console pays these too) | folds into `VICTORY_CLAMP_BARRIER_MERGE` | copystat "706 for the title's barrier groups" |
| 14 | **Up-lane copies on the copy engine** | −1 to −2 [E] | none to small: built | low | `VICTORY_QUEUE2` bit 2 (exists) | send-and-hash ms (its upload part) |
| 15 | f16 native ops, wide loads, quad ops (per-lane plan) | −1 to −3 each [E] | medium each | medium (bit-exactness; wide loads blacked the water once) | `VICTORY_F16_NATIVE`, `VICTORY_SHADER_OPT=127`, per-lane switches | per-pipeline cardmeter; picture checks |
| — | Replace the 4K upscale pass itself (DLSS at the same input size) | ~0 (+~1 for DLSS at 4K) | done (hook) | none | `VICTORY_DLSS=1` | only worth it as the enabler for rank 9 |
| — | `VICTORY_DISPLAY_MODE=1920x1080` (output size) | 0 to −2 | none | low | exists | run 6: card unchanged at 90.8 ms |
| — | Unclamped "trusted" indirect | −3 to −6 | medium | **device loss** | never default | — |

Notes on the estimates:

- **Rank 9 (resolution).** The title's card time is mostly a flat tail of small indirect commands
  and dispatches whose cost scales with *count*, not pixels. Only the pixel-bound share (full-screen
  passes, shading, post) scales with the 44 % pixel cut. Assuming a 25–55 % pixel-bound share of
  46 ms gives 5–12 ms. Nothing has measured this yet. The `fsrQuality` 1/2 runs (PLAN 1.6) are the
  cheapest experiment in this document. Earlier "540p saved 4 %" runs never changed the internal
  size, so they say nothing. Picture: the world upscale is a small spatial shader, not temporal
  (it reads colour and depth only), so 1080p→4K through it will look visibly soft. DLSS would need
  motion vectors the hook has to find elsewhere (`fsr::Recent`), which is unverified. Answering the
  upscaler's query is not an option: run 28 counted **0 FidelityFX API calls**.
- **Rank 11 (async compute).** Today every title queue lands on the one host direct queue
  (`host.rs` ~934; `queue.rs`: "the host picks engines itself"). The console overlaps the title's
  compute queue with graphics, and we serialise them. The saving is what the title's compute queue
  could hide. The cardmeter cannot show that today because it does not tag lists by source queue
  [?]. Add that tag first (section 5). Cheap to measure, expensive to build.
- **Wave64 forms** (pairs, split, whole-wave sections) are **not** a card-time item. The census has
  near-zero LDS exchange and barrier instructions, and the open wave64 work is about correctness
  (refused pipelines). The wave64 cost that is real is scalar replication (rank 10).

### 3.1 Modified-range lists against zero-copy step 2 (the coordinator's question)

The prior-art note (`research-coherency-prior-art.md`, ideas 1 and 4) proposes this: keep, per
host buffer, a list of byte ranges a submission bound **writable**, each tagged with that
submission's fence value. A CPU write over such a range discards it (upload only the CPU's bytes,
never flush). There is no hashing on the card at all.

| | zero-copy step 2 (`VICTORY_UMA_NO_HASH`) | modified-range lists |
|---|---|---|
| what goes from the card | hashing of bytes sent into **owned** ranges only; compares for kept ranges and stale-piece rehashes remain | both hash and compare passes; nothing runs on the card to find changes |
| card ms after rank 1 | ~4 → ~2 | ~4 → ~0 |
| card ms without rank 1 | 34 → ~19 measured with step 1 (runs 35, 42), then lower [E] | 34 → ~0 |
| prerequisite | `NO_COPYBACK` (step 1), which today costs **+100 ms submit CPU** (runs 35, 42: per-range sync commands) | a copy-back policy: off by default, plus an allowlist read back **whole** (fence-scoped) for the 0.19 % of stores anything else touches |
| precision | piece-exact: only changed 64 KiB pieces come back | over-approximate: the whole bound range is "modified"; fine while the range is only read back for the allowlist or copied card-to-card, costly if many big ranges must come down |
| CPU side | keeps `card_hashed`, stale tracking, `back_kept` | simpler: a range set per buffer and the writer's fence number; reuses `Shadow::gpu_writes` declarations (`card_before` already gathers `Step::GpuWrite` ranges) |
| picture risk | as step 1 | same class (skipped copy-back), plus over-approximation hiding a CPU-written byte under a "modified" tag. Idea 4's byte-exact upload (`CARD_SCATTER` already stores exact bytes) covers it |

**Recommendation.** For **card** time the two converge: once rank 1 has run, either saves only
~2–4 ms more. So neither should hold up rank 1. Choose between them on CPU grounds. Modified-range
lists remove the whole compare/fetch machinery, which is where step 1's +100 ms CPU came from.
Step 2 keeps it. Make modified-range lists the end state, built on the declared-write ranges
`card_before` already has, and keep step 2 as the near-term switch while step 1's CPU cost is
fixed. Keep the wide hash as the fallback for buffers that fall off the allowlist.

## 4. Is 33 ms reachable on the card alone?

Start from run 34: **80.4 ms**. All further figures are [E] ranges. I use the midpoints and say
where the ranges cross.

| step | card ms after |
|---|---:|
| rank 1 wide hash | ~54 (−26) |
| rank 7 or 6: hash and compare gone | ~51 (−3) |
| ranks 3, 4, 8, 13: indirect and barrier work | ~42 (−9; indirect-overhead.md's 6–10) |
| ranks 5, 10: fault branch, scalar uniform | ~35 (−7) |
| **title floor without resolution or async** | **~35 (range 30–42)** |
| + rank 9, 1080p render | ~27 (−8) |
| or + rank 11, async compute | ~30 (−5) |
| **+ RT lighting on, track B** | **+15 to +20** |

- **RT lighting off (today's picture):** 33 ms is **reachable, but not by translation cuts alone**.
  The midpoint lands at ~35 ms, so it also needs a lower render resolution or async compute. With
  both, there is ~5–8 ms of margin.
- **RT lighting on (full world):** ~42–55 ms. 33 ms is **not reachable** on this card without a
  deeper cut. That would take a 1080p render *and* async compute *and* track B at its low end
  (15 ms), all landing near their best estimates, or a lower RT quality.
- It depends on, in order: (1) rank 1 behaving in game (it has never run), (2) the indirect clamp
  and wait share being as large as the per-command timing suggests (rank 2 measures it in one run
  per mode), (3) the title's pixel-bound share (the `fsrQuality` sweep), and (4) whether RT counts
  toward "full world" for the 30 fps target.
- Pipelining has to get two frames in flight for card time to equal frame time. Today the submit
  thread waits ~56 ms a frame on the card (bytes the next frame reads).

## 5. Instrument work this plan needs (small)

1. **passtime prints nothing** (run 20). Fix it, and add `Check` as a fourth phase so it matches
   the cardmeter kinds. Then Sync/Draw/Check/CopyBack can be read without cardmeter's per-list
   resolves.
2. **Tag cardmeter lists by the title's source queue** (graphics or compute). This sizes rank 11
   before anyone builds it.
3. **Add our in-list work to the "all other lists" split**: clamp passes plus their waits, and the
   present copy. Then the title's 46 ms separates into title and ours without a `BARRIER_TEST` run.
4. **Card busy as a union, not a sum,** once there is more than one queue (rank 11, `QUEUE2`).

## 6. Order of runs (one switch per run, batch permitting)

1. `VICTORY_CARD_HASH_WIDE=alt` (rank 1). If clean, `=1` against the control.
2. `VICTORY_BARRIER_TEST=indirect`, then `groups` (rank 2, timing only).
3. `fsrQuality=2`, then `=1` (rank 9 sizing, no code).
4. `VICTORY_GATHER_BY_COPY=alt` re-read on card ms (rank 3), and `VICTORY_MEMORY_FAULT_BRANCH=1`
   (rank 5).
5. Then build ranks 4, 6, 8, and the queue tag, in that order.
