# Why the fixes don't deliver their estimated ms

*2026-11-27, written from the logs of 26–27 November (`D:\victory-work\logs`, 25 runs from 20:49 to 04:07), run 42's sampling profile, the runs table and the designs. I did no builds and no game runs. Scripts are in `D:\victory-work\gap-model\`: `rows.py` builds the per-interval table, `model.py`, `noise.py` and `altcomp.py` do the analysis, and `sim3.py` / `sens.py` hold the predictive model.*

Labels: **[M]** is measured in a log, **[C]** is read in code, **[P]** comes from the run 42 profile, and **[E]** is an estimate or model output.

---

## 1. The answer in one paragraph

The frame is **a closed loop, not a pipeline with one bottleneck**. Our submit thread prepares a submission and the card runs it. The relay then lands its copy-back. The next submission's preparation **reads title memory that the in-flight shaders write**, so the submit thread waits for that landing before it can go on (`relay_wait(WHY_READ)` in `Host::submit`, host.rs ~5980 [C]). Each of the three stages is busy only part of the frame: the submit thread is ~53 % working, the card ~69 % busy and the relay ~35 % landing. Each spends most of the rest waiting for the one before it. In a loop like this, a millisecond removed from one stage turns partly into idle on that stage. On average only **~0.5–0.7 of it reaches the frame**, and work added to the busiest stage costs ~1:1 or more. The designs estimate each cut as if its stage were the only wall, or they add up savings from threads that already run in parallel. So the measured results come in at a half, zero, or (for zero-copy) negative. **ASYNC_PRESENT** did not remove that wait. It moved it from the title's present call to our submit thread. **GPU-ordered copies** is the first queued change aimed at the dependency itself, so it is the one to measure first.

---

## 2. One frame of the control, per thread (run 34, `victory-run-20261127-002039`, 138 ms)

The "usual frame" line averages only the non-long frames, and those include the 2 ms partners of the long frames. So it reads ~16 % low (116 against 138). Its components are scaled by line/usual = 1.19 to get **ms per presented frame**. The lines that say "per frame" (relay, per-thread lines) already divide by presented frames.

| thread | working | waiting | what it waits for |
|---|--:|--:|---|
| **Title, 5 recording threads** | 63–70 ms each inside **our** list calls [M: "list calls by thread"]; ~28 % on a processor [P] | ~70 % [P], mostly on the title's own job events (`title+0x43c4140`) | the title's own fork/join; the title is not the busy party |
| **Title, thread that calls submit** | ~2 % on a processor [P] | ~98 % [P]: **~65 ms in our hand-off** (`release_held_now` → `execute_late`, the same lock the present takes; submit-split.md §1), ~54 ms on its own event | our submit thread |
| **Title, present thread** | 0 ms in the present call in normal frames (ASYNC_PRESENT) [M] | **110–186 ms in long frames** [M: "long frame" lines] | the worker's walk of the line (submit-split.md row 10) |
| **Title, fence polling** | — | mwait **~20 ms a frame** [M]; fence values are set **1.0–1.1 frames** after the title's Signal [M: `pace, per frame`] | flights landing (fence values are set at landing, `Late::Signal` [C]) |
| **Our submit thread** | ~73 ms CPU inside submissions (58 user + 16 kernel) [M] | **~64 ms before preparing** (the counter named "waiting for the submission lock"), plus 5.6 ms idle between submissions | **~57 ms of the 64 is waiting for flights to land** ("title memory read" settles, 41–57 ms usual-scale, 100 % of settle time [M]); ~12 ms is `sync_heaps` (descriptor mirror, 12.3 ms [M]) |
| **Card** | **95 ms busy**: title lists 55, our send-and-hash 34, compare 6.5 [M] | **37 ms idle**: 22 "behind us" (the submit thread is working and nothing is queued), 15 otherwise | the submit thread |
| **Relay (copy-back)** | landing 47 ms [M] | 67 ms "for the card with nothing to land", 10 ms for piece fetches [M] | the card |

**The chain:** submit prepares *i* → card runs *i* → relay lands *i* → submit prepares *i+1* (or *i+2*), which reads what *i* wrote. Three stages, all partly busy, each waiting on the one before. The waits chain into each other: the submit thread's 57 ms waits on the card and relay, the card's 22 ms "behind us" waits on the submit thread, and the relay's 67 ms waits on the card.

Two counters are misleading:
- **"waiting for the submission lock"** (copystat thread line) is `started − called` in `Host::submit` [C]. That span includes the mutex, but also `settle`/`relay_wait` (the landing waits), `sync_heaps`, `sweep` and `make_waiting`. Most of it is the data dependency, not lock contention.
- **"usual frame"** includes the 2 ms short frames and leaves out the long ones. Every component on that line is ~16–19 % low against the frame line. Its on/off differences are also biased by the switch's effect on pairing: ASYNC_PRESENT "cut title card time 10–12 ms" in-run, which is impossible. It is a denominator artefact.

### Long + short pairs

Since ASYNC_PRESENT, **20–25 % of world frames come in pairs**: a ~280 ms frame followed by a ~2 ms frame (two presents released together). Before it, there were none (run `213605`: 0 pairs). Zero-copy makes 98 % of frames pairs. A pair costs only **~+8 ms per frame over single frames** (control: pairs/2 151.7 against singles 143.2). That is **~2 ms on the mean**, but it moves the median and the "usual" line a lot. So the present-ticket fix (submit-split stage 1, est. −6 to −10) should be expected to give about **−2 to −3 ms** on the mean.

---

## 3. Is the title pacing itself?

- **It has a 30 fps cap, but the cap is not active in the world.** Menu and loading windows show 150 frames in 5.0 s, every one 33–34 ms [M, `frame times`]. We present with `Present(0, 0)` (no vsync, host.rs 8970 [C]), so the cap is the title's own limiter. It will matter only at the goal: 30 fps is the ceiling unless the limiter is found and lifted.
- **No sleep pacing in the world.** title-main sleeps ~12 ms a frame [P], and recorders ~6 ms. These are back-off sleeps inside wait loops (`SleepEx` under our HLE wait), not a frame limiter.
- **Frames in flight:** the title waits through no fence API (0 waits [M]). It polls fence memory (mwait ~20 ms a frame) and its fences are set ~1.05 frames after Signal. A simulation in which the title allows **2** frames ahead of landing reproduces the control. One frame ahead does not (it predicts 156 ms). So today the title is held back by our hand-off (~65 ms [P]), not by its own frame limit. Once the landing waits go, its fence depth becomes the next link in the chain (section 6).

---

## 4. Noise and session effects

| source | size | evidence |
|---|---|---|
| within a run (SE of the run's median) | **0.4–1.4 ms (~1 %)** | bootstrap over 5-s intervals, `noise.py` |
| **between runs, same build and settings** | **SD ~2 ms (1.6 %), range 132.2–138.1** | 6 runs 00:20–03:33 (control, FSR trace, port 6, FSR draw trace, 03:20, DLSS-not-engaged) |
| outlier runs | **+15 %** | 03:58 control: 155.5 ms. It slowed within the run (thirds 144 → 150 → 156) while card busy fell (73 → 67), then crashed. Not thermal: no other run slows in its last third |
| **in-run `alt` (8 cycles of 20 s)** | **95 % CI of the on/off ratio ≈ ±6–9 %** | per-cycle ratios spread 0.86–1.33. ASYNC_PRESENT read **1.001** (`220710`) and **0.910** (`221651`) on the same switch. The "3 % band" verdict claims more resolution than 8 cycles give |
| warm-up | first third 3–8 ms slower in 17 of 20 runs | streaming after the scene loads |
| window size | not visible | `213605` at 936×526 ran 136.9, inside the 468×263 cluster; run 6 at 960×540 left the card unchanged |
| remote desktop / CPU contention | cannot be separated from the logs | `frames.py --check-cpu` prints **0.49–0.56 in every run, including the reference**, so its 0.6 threshold always warns and tells nothing. "At work" includes page-fault and kernel time, so ~0.55 is normal. Re-base the warning on the control (e.g. < 0.45) |
| scene load (common cause) | within-run slopes overstated 2–4× | within one run every component scales with the scene: copy-back gets a slope of **4.06 ms of frame per ms**, recording r = 0.95. Never estimate a saving from within-run correlation |

**Consequence:** a design estimate below ~5 ms (~4 %) cannot be confirmed or rejected from the frame number of one run or one 8-cycle `alt`. GATHER_BY_COPY (2–5 ms) and port 6 were always going to read as "neutral". They have to be judged on their **component** counters, which are 3–10× less noisy: card busy is 79.7–80.9 ms usual-scale across the six same-build runs (SD 0.6 ms).

---

## 5. Why each change came out as it did

| change | design estimate | what moved [M] | why the frame didn't follow |
|---|---|---|---|
| **ASYNC_PRESENT** | 9–11 % | The present block (60–65 ms) and the submit thread's 39 ms idle between submissions became **pre-preparation waits 16 → 64 ms** and "title memory read" settles **0 → 48–56 ms** | **It moved the wait.** The same data dependency is now waited on by our submit thread instead of the title's present. Across runs it is −1.5 % (4 runs before: mean 136.5; 6 after: 134.4; the builds differ). In-run it read 1.001 and 0.910 |
| **GATHER_BY_COPY** | −2 to −5 ms card | card −6 to −8 ms (CI −11 to −2), **card idle +4.4 ms** (CI +1.8 to +7.5) | Over half the saved card time became card idle waiting for the submit thread. Frame −2.4 ms (CI −6 to +5.5): a real but small win, below what the test can resolve. Rejecting it was probably wrong |
| **BACK_FEWER_WAITS + BACK_POOL** | 2–15 ms submit | card waits still ~7 a frame | The waits are a **data dependency** (preparing reads shader-written bytes), not a policy. Fewer wait calls cannot remove it |
| **PIPE_AHEAD=2** | max(phases) instead of the sum | never 2 in flight; frame +3.4 (CI +0.5 to +5.6) | pipelining.md assumes independent frames. The same read dependency serialises them |
| **zero-copy step 1** | −35 to −40 "submit thread" (uma.md row 1) | copy-back −21 to −25, landing 47 → 15, hash −13 to −19; **preparing +89 (usual-scale), submit CPU +100** | (a) The estimate added copy-back (relay thread) and the compare list (card) and called the sum "submit thread". Both run in parallel with the submit thread, and only their tails are on the chain. (b) The new cost landed on the submit thread, so the loop switched to **submit-bound** (at-work 196 ≈ frame 197). The model predicts +65; measured +59 to +64 |
| **port 6** | (guard/tracking) | guards unchanged at 12.4–12.7 ms (tracking-cost.md) | nothing on the chain moved; 134.3 is inside the same-build cluster |

---

## 6. A model that predicts the results

### 6.1 Which single metric predicts best

Run-level medians across 20 runs (`fit.py`):

| predictor | r with frame (runs) | note |
|---|--:|---|
| submit preparing | **+0.85** (frame-line target +0.78) | best single metric, but only because the submit thread is in every regime's chain |
| max(card busy, submit at work, relay landing) | +0.81 | right regime switch, wrong level |
| submit at work + waiting for the card | +0.73 | |
| card busy alone | **−0.13** | card busy hardly varies (91–98 ms per presented frame in every card-bound run) |
| copy-back / relay landing | −0.75 / −0.85 | negative: zero-copy cut it and the frame rose |

**The most useful invariant:** in every card-bound run, **frame ≈ card busy + K**, with card busy 91–98 and **K = 41–55 ms of card idle**. Almost all run-to-run variation is in K, the serialisation gap, not in anyone's work.

### 6.2 The loop model (`sim3.py`)

This is a max-plus simulation of 8 submissions a frame. Submit thread: once per frame `pre` (sync_heaps, sweep), then `prep/8` per submission. The card runs each submission when it is sent and the card is free. The relay lands it after the card. Before preparing, a submission waits until flight *i−1* (probability *q*) or *i−2* has landed. The title can run at most 2 frames ahead of landing (its fences).

There is one free parameter, **q = 0.10**. It is fitted to the control and matches three observables at once: frame 116 usual-scale (measured 116.5), submit waits 43 ms (measured 41–48) and card idle 31 % (measured 28 %). The output is converted to frame-line ms (×138/116.5).

| case | model | measured |
|---|--:|---|
| control | 135 | 138.0 |
| GATHER_BY_COPY (card −6) | **−4** | −2.4 (CI −6 to +5.5) |
| BACK_FEWER_WAITS / PIPE_AHEAD (dependency kept) | **0** | +8 (noise) / +3 |
| zero-copy step 1 (prep 55 → 144, landing 57 → 20, card 80 → 74) | **+65** | **+59 to +64** |
| before ASYNC_PRESENT (present drains every flight) | +8 | 127–145 (older build) |

The model reproduces every result that a "sum the saving" or a "max of phases" estimate got wrong. Its **elasticities** (frame ms per ms of work, at the control):

| stage | elasticity now | when it becomes 1:1 |
|---|--:|---|
| card work | **~0.6** | once the landing waits are gone (card becomes the wall) |
| submit preparing | **~0.7** | once it exceeds card + K, as with zero-copy |
| relay landing / copy-back | **~0.5**, falling to 0 once the submit thread dominates | never on its own |
| a 2 ms pair cut (present ticket) | ~0.25 on the mean | — |

**Zero-copy breaks even at preparing ≈ +30 ms over the control** (usual-scale P ≈ 86). With its preparing back at the control's level, it is worth **−22 ms**. So the in-flight fixes (`f11a218f` scatter, `4bec9514` in-place compare) have to remove ~60 of the ~90 ms they added before zero-copy wins at all.

### 6.3 How to predict a win from now on

1. State the change as **Δ(work) per stage**: submit preparing, submit pre-work, card, relay landing, title. Write which thread each cost runs on, and never add savings from two different threads.
2. Run `sim3.py` with those Δs (`sens.py` shows how) and read the frame change. Read the regime too: card-bound, submit-bound, or title-fence-bound.
3. Measure the **Δ(work)** with the component counters (card busy, preparing, landing), split by phase with `altcomp.py`. These resolve 1–2 ms. The frame number does not.
4. Promote on **predicted Δframe from the measured Δ(work)**, then confirm with the frame only when the prediction is above ~5 %.

---

## 7. Tonight's queue, ranked by the model

| rank | change | Δ work assumed | model Δframe | what decides it |
|--:|---|---|--:|---|
| 1 | **GPU-ordered copies** (`VICTORY_GPU_ORDERED_COPY`) | the CPU landing waits go (title copies that read shader writes run on the card in queue order); card +0 to +4 | **−36 to −41** if the title runs 2 frames ahead; **−19 to −24** if only 1 | It is the only queued change that cuts the dependency rather than a stage's work. Afterwards the card is the wall (~95–100 ms busy), and every card cut becomes 1:1. Watch the title's mwait and "fence is set N frames after": if mwait rises by what the frame gained, the title's fence depth is the next link, and fences should then be set at card completion where it is safe to |
| 2 | **Wide card hashing** (`VICTORY_CARD_HASH_WIDE`) | card −24 (card-budget.md: −24 to −30) | **−15** alone; **~−24 to −30 on top of ordered copies** | Card-side, so only ~0.6 now. After rank 1 it is the largest card cut available. Ordered copies + wide hash ≈ **86 ms**, where the submit thread becomes the wall |
| 3 | **GPU-modified ranges** | compare −5 card, landing −40, preparing +5 (an assumption) | **~−20** | Large uncertainty and L effort. Most of its value is landing, which falls to ~0 once rank 1 removes the landing waits. Re-model after rank 1 |
| 4 | **Guard fixes** (tracking-cost A+B+C) | preparing −5 to −10, title threads −3 to −5 | **−3 to −7** | Below the frame's resolution. Judge on guards ms (11–12.7 → <3) and preparing. Its value rises after rank 1, when the submit thread is the wall |
| 5 | **Clamp batching** (indirect-overhead rows 2/5) | card −1 to −3 | **−1 to −2** | Unmeasurable on the frame. Use cardmeter "all other lists" |

Zero-copy step 1 with fixes: **+13** at preparing 100 ms. Do not promote it until preparing is within ~30 ms of the control's. The present ticket: **−2 to −3** on the mean (section 2).

---

## 8. What to measure or change first

1. **Run ordered copies `=alt` against the control.** Pass criteria: "title memory read" settles → ~0; submit pre-preparation wait 64 → <15 ms; card idle 37 → <15 ms. Expected frame: −30 to −40 ms. Also read the title's mwait and fence-set frames, because they show the title's fence depth (the next link).
2. **Fix the instruments before trusting more "neutral" verdicts:**
   - Add the per-phase **component** split (`altcomp.py`) to `frames.py --alt`, with a CI per metric.
   - Rename or split "waiting for the submission lock" into lock, landing waits, sync_heaps and sweep.
   - Make the "usual frame" line exclude the 2 ms partners (or divide by presented frames).
   - Re-base `--check-cpu` on the control's 0.54.
   - Use ≥16 `alt` cycles for effects under 8 %.
3. **Then wide card hashing** on top of ordered copies, where its 24–30 ms should arrive close to 1:1.
4. **Re-judge GATHER_BY_COPY on card ms.** It cut 6–8 ms of card. Under ordered copies it would be worth that much on the frame.

---

## 9. Update 04:30: run 19 (ordered copies `=alt`) and run 17 (control, same build)

Logs: `victory-run-20261127-041155` (run 19, build 2261b969) and `victory-run-20261127-035835` (run 17). Per-phase splits come from `gap-model\phasegrep.py`, `phase2.py` and `altcomp.py`; the run comparison from `cmp.py`. All figures are per presented frame unless marked.

### 9.1 Ordered copies did remove the landing wait

The "copies on the card in queue order" line covers ~9 s windows, but a phase lasts 20 s, so about half the windows mix ON and OFF. Averaging every window gives "~0.9 avoided, 0 ms". The windows that lie wholly inside a phase read:

| window | title copies moved | fills moved | waits avoided | read waits left | ms waited |
|---|--:|--:|--:|--:|--:|
| wholly ON (8 windows) | 10–13 | **6,400–7,070 (~2.6 MB)** | 3.7–4.0 | 0.3–0.5 | **1.9–5.9** |
| wholly OFF (6 windows) | 0 | 0 | 0 | 1.4–2.0 | **52–64** |

The settle counter agrees: "title memory read" is **4.8 ms ON against 55.6 ms OFF**. The model's dependency was real, and the switch removes it.

### 9.2 The wait moved again: the submit thread became CPU-bound

The submit thread's frame, from the per-thread line (per presented frame):

| | OFF | ON | Δ |
|---|--:|--:|--:|
| before preparing ("lock": landing waits, `sync_heaps`, sweep) | 71.6 | **28.4** | **−43** |
| CPU inside submissions (user + kernel) | 81.7 | **101.0** | **+19** |
| idle between submissions (waiting for the title) | 5.8 | **17.6** | **+12** |
| sum ≈ frame | 159 / 151 | 147 / 145 | |
| relay landing | 53.8 | 67.7 | +14 |
| copy-back MB (usual-scale) | 51.7 | 64.3 | +12 |
| title bytes put in under flights (pages) | 63 | 285 | 4.5× |
| title list calls, all threads | 380 | 401 | +21 |

The ON phase is **submit-bound**: 101 ms of CPU + 28 ms before preparing + 18 ms waiting for the title ≈ the 145 ms frame. Of the 43 ms the switch freed, 19 went into new submit-thread CPU and 12 into waiting for the title. The frame kept **−6 ms** (145.2 against 151.4; per-cycle median −8, CI −25 to +52 over 5 cycles). This is zero-copy's pattern again, and section 6.2 named it as the risk: once the dependency goes, every ms the change adds on the submit thread counts 1:1.

### 9.3 What triggers the wait, and why moving it is expensive

- **The trigger is one title copy a frame, not the fills.** `ordered.rs`'s header and `fill_check` both name the copy `0xdc07ea000 → 0xd1f3xx000 + 0x800`: 2 KB whose source the card writes (545 + 271 hits in run 34's `fill_check`). OFF there are ~2 such waits a frame. Refreshes, `sync_heaps` and the sweep show **0 ms** in every other settle reason (sync, stale, most, step and fence are all 0.0 in both phases).
- **The cost comes from the cascade.** `plan()` moves every later step that reads what a moved copy wrote ("what reads a moved copy moves with it"). So that 2 KB copy drags **~6,700 fills a frame** (~400 B each, ~1.4 per draw: per-draw constants read from the copied block) onto the card. In host.rs (~2690–2715) each moved fill becomes a `Refresh` of its source plus its own `Step::Ordered` copy command. That is ~13,000 extra steps and ~6,700 extra `CopyBufferRegion` calls per frame: the +19 ms CPU. It also brings more title bytes put in under flights (63 → 285 pages) and more copy-back. Fills of shader-written bytes outside upload memory: **0** ("kept on the CPU: 0 without upload memory the card can write").

### 9.4 The change that removes it at the right cost

**Keep ordered copies, but replace the per-fill path with one batched scatter per submission.**

1. Collect the moved fills of a submission as a table of (source offset in the host copy, destination offset in the writable upload chunk, length).
2. Refresh the **union** of their source spans once, instead of once per fill.
3. Write the table into upload memory, then copy everything with one `CopyBufferRegion` per contiguous run, or with **one compute dispatch** that reads the table (~6,700 pieces, ~2.6 MB: well under 1 ms of card).

Expected: the +19 ms of submit CPU falls to ~1–2 ms. The submit thread then comes to about 6 + 28 + 82 ≈ **~116–125 ms a frame**: −20 to −30 against this run's OFF phase, about −13 to −22 against run 34.

Measure it on the per-thread line (CPU inside submissions back to ~82 ms), not on the frame.

The next items in order:
1. The 28 ms left before preparing (`sync_heaps` ~12 ms, the sweep, the residual 5 ms of waits).
2. The 12 ms of waiting for the title. Watch mwait and fence-set ms, because heavier landing delays the title's fences.
3. Wide card hashing, once the submit thread is under the card's ~95 ms.

### 9.5 Run 17's +17 ms against run 34

No work counter explains it. Copies, guards (12.3 → 13.0 ms), watches, hash passes and fetched pieces are all level or lower. Draws are +2 %. The extra time shows as:
- **waits:** before-preparing +11.4 ms, of which "title memory read" settles +7.8; relay landing +4.0
- **CPU per unit of work:** submit kernel +3.8 ms; title list calls +29 ms across threads (**8.46 → 8.87 µs per call**, +5 %); process user +38 ms
- **pairing:** long frames 16.5 % → 23.5 %

The run also slowed within itself (thirds 144 → 150 → 156) with a falling card, and had the lowest CPU/at-work ratio of the night (0.49). Through the loop's elasticities, a ~5 % slower CPU explains the +17. That points to the session, or to the new build's per-call cost, not to a single regression.

For the `VICTORY_PAGE_WRITES=guards` check, compare **µs per list call** (8.46 on run 34, 8.87 on run 17, 9.32 on run 19), submit kernel ms and guards ms against run 17, not the frame alone. Repeat the control once to tell the session from the build.
