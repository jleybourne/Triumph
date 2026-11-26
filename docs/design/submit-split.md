# Submit split: taking work off the submit thread

Design only. No code was changed, nothing was built or run for this. Tree read: `wt-bisect`, branch `fast-integ`, HEAD `2261b969` (`queue.rs` `release_held_now`/`send_joined`/`present_now`/`send_behind`, `host.rs` `Host::submit`/`execute_late`/`card_before`/`run_deferred`/`Batch`, `shadow.rs` `update`/`update_free`/`copy*`/`wide`, `watch.rs` `ask_ahead`/`writes_as`/`is_view`/`uncommitted_end`).

Labels: **[P]** is the run 42 sampling profile (zero-copy on, `NO_COPYBACK=1`). **[F]** is the frame log, in-world means (≥1,200 draws). **[E]** is an estimate. Effort is **S** (≤1 day), **M** (2–4 days) or **L** (a week or more).

## 0. Reading the evidence (one correction)

The profile's "ms a frame" divides by **all 3,383 presented frames**. About 1,620 of those are loading frames (~50 ms each, 81 s of the run). The submit work happens almost entirely in the world, so **in-world costs are about 1.94× the profile's figures**. Cross-check: Host::submit 95.6 [P] × 1.94 = 185 ms. The run's frame log has preparing 168 + waiting for the card 37 = 205 ms per world frame. They agree within 10 %. The *shares* inside Host::submit are unaffected. Idle time is not: most of the worker's 26.8 ms [P] of idle falls in loading frames. Below, profile numbers are quoted as given, with ×1.94 where a world figure is needed.

Profile frames are module+offset, and the PDB beside the exe is newer than the exe that was profiled. Names were matched by structure. The worker's stack is worker → `release_held_now` (`+0x42fa40`, which the title's present also enters) → `send_joined` → `execute_late` → `Host::submit` (`+0x43a680`). The inclusive sums then reproduce the brief's numbers exactly:

| frame | name | ms [P] |
|---|---|---:|
| `+0x43a680` | Host::submit | 95.6 |
| `+0x4e24b0` | card_before (36.0 of it in D3D12Core→driver, i.e. CopyBufferRegion per range) | 47.6 |
| `+0x4d5390` | run_deferred | 18.6 |
| `+0x4df6c0` | Batch::flush (9.3 inside run_deferred + 3.9 at the end) | 13.1 |
| `+0x4ffb00` → `+0x5016b0` | Batch::update → Shadow::update_free | 16.5 → 16.0 |
| WaitOnAddress directly in Host::submit | relay_wait (Condvar on the line: the card/landing wait) | 16.9 |
| `+0x6eda40` WaitOnAddress | worker idle (WORK condvar) | 26.8 |
| title thread, `+0x42fa40` → `+0xa01b90` | the present blocked on the `RELEASING` mutex | 65.4 |

Inside update_free [P]: write-watch and guard path 6.8 (NtQueryVirtualMemory **3.3**, NtProtectVirtualMemory 0.8), copying 6.3, light check 2.5. NtAllocateVirtualMemory plus NtFreeVirtualMemory add **1.5**: large buffers are made and freed on every call.

## 1. The critical path, step by step

The control is the launcher switch set without zero-copy: log `victory-run-20261127-033324`, 2,534 world frames, diag build, **146.8 ms** a frame [F]. Of those, 2,180 are "usual" frames at 118.4 ms and 354 (14 %) are "long" frames at 322 ms. The second column is run 42 (zero-copy, profiled), where the same steps cost more.

| # | step (thread: victory-submit unless said) | control [F] | run 42 [F] / [P]×1.94 |
|--:|---|---:|---:|
| 1 | Woken; `release_held_now` walks the lines and joins parts (≤4 parts / 48 lists) under `RELEASING` | ~0 | ~0 |
| 2 | Host::submit: `submitted` lock; `order_on_card`; `late_blocked` → `relay_wait(WHY_READ)`: the next frame reads bytes the last frame's shaders wrote, so we wait for the card and the copy-back | **48.3** (46.3 by the settle counter) | 37.1 / 33 |
| 3 | `sync_heaps`, `sweep`, `make_waiting`, `ask_once`/`plan_beside`, `heal_reads_all` | in "other" below | small |
| 4 | `run_deferred`, list by list in order: title copies, fills (≈9,300 copies, 68 MB a frame), refreshes batched, GpuWrite → `flush_one` + `writing`, texture refreshes | copies **51.8** in all (rows 4+5) | 36 |
| 5 | `Batch::flush` → `update` → `update_free`: write-watch asking, guards, VirtualQuery, copying changed pages | (in 51.8) | 31 (of which kernel VM calls ≈ 11) |
| 6 | `take_snapshots` | in "other" | small |
| 7 | `card_before`: the declared ranges (a linear walk of every step), `card_dirty`, recording the sync list (CopyBufferRegion per changed range, hash dispatches), Close, ExecuteCommandLists | in "other" | **92** (70 in the driver; scatter `f11a218f` since) |
| 8 | `run_ordered`, ExecuteCommandLists + Signal, `declared_writes`, texture-back ranges, `writes.clone()` | in "other" | small |
| 9 | `execute_late`: the line push, the relay notified | ~0 | ~0 |
| | **preparing in all** (rows 3–8) | **69.5** (51.8 copies + 17.7 other) | 168 |
| 10 | Idle, waiting for the title's next part (frame − preparing − card wait) | **29** (21 usual, 75 long) | 31 |

The title's present thread (not on the worker's path, but it sets row 10):

- With `VICTORY_ASYNC_PRESENT=1`, a present goes behind the line only when no other present is held (`send_behind`: `PRESENTS_HELD != 0` → the old path).
- The old path calls `release_held_now`, which blocks on `RELEASING` until the worker has finished its whole walk. That walk includes every Host::submit of the newer frame.
- Control: **140.6 ms in long frames, 22.0 mean**. Run 42: 128 ms in usual frames, because the worker was always more than a frame behind. That is the 65.4 [P].
- While the title is blocked it does not record, so when the worker drains it idles (row 10).

**What limits the frame:** rows 2 + 4/5 + 7 + 10, all on one thread. Rows 1–9 on their own are 118 ms; the card, by its own clock, is 74 ms (send-and-hash 25, compare 5, the title's lists 44–50 [F, usual frames]). So the submit thread is the wall today. **Once this design and ORDERED_COPY land, the card becomes the wall at about 74 ms.** Reaching 33 ms also needs zero-copy step 2 (the hash lists) and the GPU cuts in phase 3. The aim here is a submit thread that does at most 30 ms a frame.

## 2. What can move

### 2.1 In parallel (worker threads)

| work | unit of parallelism | why it is independent | ms at stake |
|---|---|---|---|
| Copying in shadow refreshes (`copy_stale` → `copy_plain`/`copy_owned`) | per shadow, in the order of that shadow's Refresh/GpuWrite steps. The 174 MB buffer's merged range is cut into ≥1 MiB page chunks between its GpuWrites | each refresh writes only its own copy's CPU side and reads title memory. Page stamps, `dirty` and `owned` are locks per shadow | about half of the 51.8 copies [E from the P shares: copying 6.3 + fills/title copies ~6 of ~22 profile-ms are memory moves] |
| Fills of upload memory, and title copies that nothing else in the submission touches | per list, in batches of ~64 KiB | a fill writes upload memory private to its list and reads title memory | ~6 [P] → ~11 world |
| Texture staging refreshes (`Step::Texture`) | per texture (`texture_due` already dedups) | each writes its own staging buffer | 4.5–7 [F "texture refreshes, all threads"] |
| Recording the card sync list (`card_before`) | per shadow group, into separate lists (D3D12 lists are recorded independently on separate threads), run in one ExecuteCommandLists in shadow order | the buffers are disjoint; `differs`/scatter buffers are cut into slices per job | 10–20 in zero-copy after scatter [E]; ~2–5 in control |
| `scatter_words` (a loop byte by byte) | per shadow | pure function of the ranges and the CPU side | part of the above |

What stays serial, on purpose: the write-watch asking and the guards (`JOURNAL` mutex, `GUARDS` RwLock), held across GetWriteWatch/VirtualProtect. These also take the kernel's address-space lock, which the title's own page faults need. Run 112 measured `VICTORY_WIDE_REFRESH` worse (copies 52 → 68) on an older build. The likely cause is that each worker took those locks and made those kernel calls. **The asking stays on the coordinating thread; the workers only move bytes.**

### 2.2 Earlier (into recording threads, at list Close)

Each closed list gets a digest. Recording threads stand still about 70 % of the time [P: 28 % on a processor]. Everything below is a pure function of the closed list:

- the adjacent-copy merge plan (`skip`/`extra` in `run_deferred`)
- each shadow's refresh spans (`fold_spans`, done on wide threads at submit today by `plan_beside`)
- the declared GpuWrite ranges per shadow, which `card_before` and `declared_writes` each rebuild with a linear `find` over shadows and `ranges.contains`
- the texture-back ranges and the bottom-of-pipe `writes`
- a classification for §2.1: has Run/Host steps, the read intervals (fill and copy sources, refresh ranges, texture memory), and the title-copy destinations
- the intervals `blocked_by` and `order_on_card` scan for

**Not moved earlier: the fills themselves.** On the console the card reads title memory when it runs the list, and the title may write constants after it recorded the draw and before it submitted. A copy made at record time would be wrong, and checking it at submit costs as much as the copy.

### 2.3 Later (after ExecuteCommandLists)

- **Card sends**: the sync-list recording, ExecuteCommandLists, Signal, `declared_writes` (from the digest by then), and `video_memory_look`/`log_messages`/`sweep` move to a send thread. The submit thread hands off the plan and starts preparing the next part.
- **Must stay before the lists run**: `take_snapshots`, the refreshes, `sync_heaps`/`make_waiting`, and `card_dirty` (taking the changed ranges). These are ordered on the CPU side.

### 2.4 Lock scopes that can shrink

| lock | held today | who waits | change |
|---|---|---|---|
| **`RELEASING`** (queue.rs:780) | the worker's whole walk of the line, including every `execute_late` → Host::submit | **the title's present** (old path when a present is already held): 22 ms mean / 141 ms long frames [F]; 65.4 [P]; also `before_sync` | The present does not take it. It waits for the held present to be **shown** (a condvar where `PRESENTS_HELD` is lowered), then goes behind the line as usual. Second step: `RELEASING` is not held across `execute_late` at all, because the `Held::Running` marks already keep each queue's head |
| `submitted` (host.rs:6921) | from preparing through `card_before`, the send and the line push | `late_helper` settles, `cp_fetch` (try_lock), synchronous submissions | With the send thread: held for prepare + hand-off only |
| `CARD_WORK`, `QUEUE_ORDER` | from numbering the dirty ranges to the sync list's ExecuteCommandLists | relay fetches (queue2 numbering) | Numbered at hand-off; the send thread takes them in FIFO order. Every ExecuteCommandLists on the host's direct queue goes through that FIFO |
| `JOURNAL` (watch.rs:86) | the kernel GetWriteWatch | title fault handlers (`wrote_own`) | Unchanged in size, but only the coordinator takes it; workers never do |

The `RELEASING` wait is ordering back-pressure, not data protection. Shrinking the lock on its own changes nothing: the title would still wait for its turn. What helps is that the title waits **only until the older frame is shown**, not until the worker has also prepared the newer frame. The title then records frame N+1 while the worker prepares N, and row 10 shrinks.

## 3. Hazards, move by move

**A. Present waits for its ticket (`RELEASING` not taken by the present).**
- *A wait nothing meets.* Today a present forces the lines (`forces()`). If the line stands at a `Held::Value`/`Held::Wait` nothing will meet, the held present never shows. Mitigation: a timeout (`VICTORY_PRESENT_TICKET_MS`, 250) after which the old forced path runs. The timeouts are counted.
- *Frames saved or listed, a line that is empty, a plane with no host texture:* the old path, as today.
- *How far ahead:* at most one present held. This is the same memory assumption ASYNC_PRESENT already makes; it is not PIPE_AHEAD=2.
- *Wait time:* the title thread waits on a condvar instead of a mutex. No new lock order.

**B. Wide prepare (per-shadow chains, fills and copies in parallel).**
- *Write-after-write and read-after-write through title memory:* a title copy (`Step::Copy`, `title_dst`) writes title memory that a later fill, copy, refresh or texture reads. Rule: a Copy whose destination overlaps any read interval or any other copy's destination in the submission is a **barrier**: everything before it completes first, as `Batch::title_wrote` does today. Run/Host steps are always barriers (their effects are unknown).
- *Order within a shadow:* `writing()` (GpuWrite) after the refresh of that range. A later Refresh of the same shadow skips ranges that are pending. Each shadow's steps therefore run **in step order inside one job**. A big shadow is cut into page chunks only between its GpuWrites.
- *Write watch and guards:* the asking happens before the copy, on the coordinator. A title write after the asking is caught by the next one, so the semantics are unchanged; the window is a little wider.
- *NO_COPYBACK healing* (`heal_reads`) runs inside the same job before that job's read. Owned compares (`copy_owned`) take the shadow's `owned` lock, and that is the only shadow touched.
- *Global locks touched per copy* (`CHANGED`, `CARDED`, `POOL`): contention. Batch them on the coordinator after the join.
- *`watch::wrote` for title copies:* recorded on the coordinator after the join, in order.
- *CPU:* the title pins cores. Size the pool from `available_parallelism`; the jobs stay on the fast cores (`keep_on_fast_cores`).
- *The guarded copy* (`victory_copy_guarded`) is stateless and judged by its instruction address, so it is safe on any thread.

**C. VM trims (no VirtualQuery or big allocations per refresh).**
- *A cached view/uncommitted answer goes stale* when the title maps or unmaps. The existing `title_memory_remapped` hook already forgets blocks. `VICTORY_VIEW_KNOWN` and `VICTORY_ABSENT_ONCE` (both built, off, never measured here) carry exactly this risk, which their own comments describe.
- *Pooled buffers:* a per-thread pool, or else wide prepare (B) races on it.

**D. Digests at Close.**
- *A list reset or recorded again:* the digest carries the list's generation and is dropped at Reset.
- *The same list twice in one submission:* already refused by `order_on_card`; the digest is read-only.
- *Shadow lookups* (`shadow::find`) can change if the title remaps buffers between Close and submit. Keep only address intervals in the digest and resolve shadows at submit, or check a shadow-table epoch.

**E. Send thread (prepare ∥ send).**
- *Fence values are given at hand-off, in order.* The relay lands a flight only when `GetCompletedValue ≥ fence`, which cannot happen before the send thread's Signal. `relay_wait` may wait longer, never wrongly.
- *`line.written` must hold N's declared ranges before N+1's `late_blocked`.* Push it on the submit thread at hand-off (from the digest), not on the send thread.
- *Every ExecuteCommandLists on the host's direct queue* (the present copy `execute_own`, synchronous `Host::execute`, `card_back`, `run_ordered`) must go through the send FIFO. Otherwise a list lands between a send's numbering and its execution, and queue2's `order_done` lies.
- *CPU-side bytes of card copies:* `scatter_words` reads them at record time. If N+1's refresh wrote them first, N would send N+1's bytes. That is harmless for title data (the title must not rewrite memory a flight reads), but **not for owned ranges** compared against "what was last sent". Build the scatter words, the hash plan and the dirty set on the submit thread at hand-off; the send thread only records D3D12 calls from that plan.
- *Synchronous submissions* (`!late`) hand off and wait for the send thread, then settle as today.

**F. `RELEASING` narrowed.**
- *Two releasers taking parts out of the waits' order across queues.* Parts still go through `submitted` one at a time, and a wait is passed only by `value_met`/`late_met`/`joined.meets`, which look at what has been sent or taken. Keep the per-queue `Held::Running` mark as the only head lock, and keep the join state with one releaser at a time (`Joined` is local).

## 4. Staged plan

Each stage is behind its own switch with `1` and `alt` (twenty seconds on, twenty off, logged), like `VICTORY_ASYNC_PRESENT=alt`. Every stage reads, per world frame:

- fps, ms, draws, dropped
- frame log: in the present call; submit thread preparing, waiting for the card, copies
- long-frame share
- row 10 idle, derived as frame − preparing − card wait
- card idle
- one `VICTORY_PROFILE=1` run per stage for Host::submit's children

Estimates are ms per world frame against the 146.8 ms control. Where it differs, the zero-copy figure is in brackets.

| stage | switch | what | est. ms | effort | test / measurement |
|--:|---|---|---:|:--:|---|
| 0 | `VICTORY_GPU_ORDERED_COPY=alt` (built) | title copies and fills that read in-flight shader writes go on the card in queue order | **−35 to −46** (row 2) | — | the settle counter's "title memory read" ms → ~0; `fill_check`; pictures at the usual spots. Gate for everything below: once row 2 is gone, rows 4–7 and 10 *are* the frame |
| 1 | `VICTORY_PRESENT_TICKET=1\|alt` | a present refused because one is held waits (condvar, timeout `VICTORY_PRESENT_TICKET_MS`=250) for that one to be shown, then goes behind; the title never takes `RELEASING` | **−6 to −10** (−15 to −25 where the worker is always behind, as in run 42) | S | long frames 14 % → ~0; present wait 22 → <5 ms; row 10 29 → ≤15; new counters: ticket waits, ms, timeouts (expect 0 in the world). Unit test in `queue.rs`: a fake line with one present held, a second present waits, goes behind when the first is shown, and times out into the old path |
| 2 | `VICTORY_VIEW_KNOWN=1` + `VICTORY_ABSENT_ONCE=1` (built, unmeasured), plus a new `VICTORY_REFRESH_POOL=1\|alt` | no VirtualQuery per refresh (cached view/uncommitted answers forgotten on remap); the large per-refresh buffers pooled per thread | **−5 to −9** (NtQueryVirtualMemory 3.3 + Alloc/Free 1.5 [P] ×1.94 ≈ 9) | S | the profile's ntdll VM frames under update_free → ~0; `VICTORY_REFRESH_CHECK=1` (stale copies after refresh: expect no new reports); each switch's own `alt` counters |
| 3 | `VICTORY_WIDE_PREPARE=1\|alt` (replaces `VICTORY_WIDE_REFRESH`) | coordinator: classify, find barriers, ask the write watch/guards → page lists; workers: per-shadow chains (big buffer in ≥1 MiB chunks), fill/copy batches, textures; coordinator after the join: watch notes, `current`, ordered steps | **−15 to −25** (copies 51.8 → ~25–35) | M–L | unit tests of the barrier planner (pure function over a step list: an overlapping copy splits, Run splits, a shadow's chain keeps its order, chunks only between GpuWrites); in game: copies ms, "submit thread at work" vs preparing, `VICTORY_REFRESH_CHECK=1`, `fill_check`, picture checks, the title's own fence pace (title threads must not slow: recording ms) |
| 4 | `VICTORY_LIST_DIGEST=1\|alt` | per-list digests at Close (§2.2), used by run_deferred, card_before, declared_writes, blocked_by, stage 3's planner | **−3 to −6** (card_before's own time + its range walk, 10.2 [P] ≈ 20 world in zero-copy) | M | unit test: digest = what the submit-time walk computes, for recorded step lists, and is dropped at Reset; in game: card_before and run_deferred's own time in the profile; `alt` on preparing |
| 5 | `VICTORY_SEND_THREAD=1\|alt` | prepare ∥ send: the plan (dirty set, scatter words, hash runs) on the submit thread at hand-off; D3D12 recording (per-shadow lists on the wide pool), ExecuteCommandLists, Signal on `victory-send`; every direct-queue submission through its FIFO | **−5 to −10** (−10 to −20 in zero-copy) | L | one sequence counter checked at each ExecuteCommandLists (expect no gaps); queue2 `order_done` against `QUEUE_ORDER` (no fetch merges early); DRED clean; preparing and the send thread's time on the frame line |
| 6 | `VICTORY_RELEASE_NARROW=1\|alt` | `RELEASING` not held across `execute_late`; a thread that waits for the line (`before_sync`) runs wide-pool jobs meanwhile | **0 to −3** | S | only if stage 1 leaves `RELEASING` waits in the profile; the profile's title-thread WaitOnAddress under `release_held_now` → 0 |

**Expected path [E]:** 146.8 → ~105 (stage 0) → ~95 (1) → ~88 (2) → ~68 (3) → ~64 (4) → ~57 (5).

Even so, the submit thread does ~30–35 ms a frame by then, and **the frame is card-bound at about 74 ms**. Stages 1–5 make the submit thread small enough that pipelining (`VICTORY_PIPE_AHEAD=2`, built) and the card cuts (zero-copy step 2, phase 3) are what move the frame next. Stage 1 is the one-frame-ahead form of PIPE_AHEAD's admission. It needs no `VICTORY_BACK_CAS`, and PIPE_AHEAD=2 supersedes it once that is measured.

## 5. Ranked by ms per effort

| rank | move | est. ms | effort | why here |
|--:|---|---:|:--:|---|
| 1 | **Present waits for its ticket, not `RELEASING`** (stage 1) | −6 to −10 (−15 to −25 zero-copy) | S | ~50 lines in `queue.rs`; removes the title's 141 ms stalls in long frames and the worker idle that follows them; no new memory assumption |
| 2 | **No VirtualQuery or big allocations per refresh** (stage 2) | −5 to −9 | S | two of the three switches are built and were never measured on this machine; the kernel calls also take the address-space lock that the title's page faults need |
| 3 | **Wide prepare: per-shadow chains + fill batches, asking serial, copying parallel** (stage 3) | −15 to −25 | M–L | the largest CPU block left once ORDERED_COPY removes the card wait (copies 51.8 [F]); avoids run 112's failure by keeping the locks and kernel calls on one thread |
| 4 | Digests at Close (stage 4) | −3 to −6 | M | cheap per call, many calls; also feeds stage 3's planner |
| 5 | Send thread (stage 5) | −5 to −10 (−10 to −20 zero-copy) | L | most value in zero-copy; the most ordering hazards (queue numbering, owned ranges) |
| 6 | `RELEASING` narrowed (stage 6) | 0 to −3 | S | only after stage 1 |

Rejected: fills at record time (wrong on the console's semantics, §2.2). Eager refreshes at the title's submit call (the title hands a frame over in one burst at the end of recording, so there is no head start; it also crosses copy-back and earlier parts' title writes).
