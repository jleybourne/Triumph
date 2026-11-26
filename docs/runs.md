# Runs

Every measurement made on the machine described in the README, oldest first. A run is only
comparable to another when the draw count and the dropped count are read with it — a frame that drops
draws is a smaller scene, not a faster frame.

Conditions common to all rows unless stated: Series X profile, `run-fx.ps1 -Seconds 600`, frames
counted only at **1,200 draws or more**, after the load phase settles. The switch set is the one
listed at the end of the README.

## 2026-11-26

| # | build / change under test | draws | fps | ms | dropped | GPU busy | copies | copy-back | verdict |
|--:|---|---:|---:|---:|---:|---:|---|---|---|
| 1 | hand-merged tree, before adopting upstream | 5,801 | 3.4 | 295.0 | — | — | 68 ms / 227 MB | 55 ms | baseline we started from |
| 2 | upstream base, default switches | ~13 | 1.0 | 738 | — | — | — | — | ❌ load-bound, not comparable |
| 3 | upstream + measured switch set, 300 s run | 744 | 9.1 | 109.4 | **522** | 0.1 (unmeasured) | 25.5 ms / 80 MB | 38.2 ms | ❌ a third of the scene; superseded by #4 |
| 4 | same, **600 s** run | 4,525 | **6.9** | **145.9** | 356 | **90.8 ms** | 75.4 ms / 114.9 MB (11,542 copies) | 73.9 ms / 64.7 MB | ✅ the honest diagnostics baseline |
| 5 | **release build**, same switches | 4,710 | **7.7** | **130.0** | 348 | (diag only) | — | — | ✅ the playable number; diagnostics costs 11% |
| 6 | `VICTORY_DISPLAY_MODE=960x540` | 4,579 | 7.3 | 140.9 | 344 | 90.8 ms | 56 ms | — | ❌ 4% for a quarter of the pixels — not fill-rate |
| 7 | `VICTORY_HEAP_SYNC_TABLES=1` | 4,753 | 6.0 | 167.3 | 347 | — | — | — | ❌ slower by 21 ms |
| 8 | `VICTORY_COPY_PLAIN=0` (copies through the kernel) | 4,692 | 6.6 | 152.7 | 344 | **89.7 ms** | — | — | ❌ no faster, and it proves GPU-busy is not copy traffic |
| 9 | `VICTORY_MERGE_RUNS=4096` (join contiguous copy ranges) | 4,988 | 5.0 | 201.3 | 347 | — | 11,782 / 206 MB | — | ❌ count unchanged; run also contended with another |
| 10 | `VICTORY_COPY_MERGE=1` (join runs of adjacent copies) | 4,562 | 6.5 | 154.8 | 345 | — | 11,336–11,795 | — | ❌ nothing joinable: only 28 copies/frame reach that list, 0 continue the one before |
| 11 | `VICTORY_EARLY_WORLD=1` (start every pipeline at creation) | **33** | — | — | — | — | — | — | ❌ never reached the world in 600 s — starves the load phase |
| 12 | integrated: per-draw dedup + copy-back merge | **33** | — | — | — | 0.0 | 18 / 0.3 MB | — | ❌ never reached the world — one of the four switches stalls loading; bisecting |
| 13 | per-draw dedup only (copy-back merge off) | **13** | — | — | — | — | — | — | ❌ still never reached the world — the dedup switches themselves stop the scene loading |
| 14 | **control**: integrated build, all four switches off | 4,940 | 6.7 | 148.9 | 354 | — | — | — | ✅ the build is sound — within variance of #4 and drawing more; the harm is the switches, not the code |
| 15 | `VICTORY_INPLACE=2 VICTORY_INPLACE_MORE=1` (zero-copy title buffers) | 4,851 | 7.0 | 142.6 | 346 | — | **6,460 / 187.4 MB** | — | ⚠️ neutral: copy count halves but bytes rise 63%, frame 142.6 against 145.9 — inside variance. It loads, unlike most of tonight's changes. (An earlier reading of 334.8 ms from this run was taken mid-load and is not the settled figure.) |
| 16 | avatarmanz fork (`avatar/main` b0cfe2ba), same harness | 4,892 | **1.9** | **538** | 392 | — | — | — | ❌ 3.6x slower than our tree on this card; the fork's 13-22 fps figures are its 24 GB RDNA3 and lighter scenes, not transferable |

## What a row must contain

- the change under test and the switch that enables it,
- draws, fps, ms and dropped **together**,
- GPU busy, copies (count and MB) and copy-back (ms and MB) where the build prints them,
- a verdict, and for a rejection *why* it failed rather than just that it did.

Rows are never removed. A rejection is a result: it stops the same idea being paid for twice.

| 17 | `VICTORY_REFRESH_COARSE=1` (coarser change detection) | 4,466 | 7.0 | 149.1 | 348 | — | **10,980-11,389 / 192-213 MB** | — | ❌ fails its own test: count falls 4% while bytes rise 70-85% — copying padding, not less work |
| 18 | draw census (`VICTORY_DRAW_CENSUS=1`) | 5,221 | 6.1 | 163.3 | 348 | — | — | — | ❌ the census printed `0 (0.00x)` for every counter — it was never hooked to the real call sites; the real barrier count (706 a frame) came from the copystat line |
| 19 | `VICTORY_XMEM_COMMIT=1 VICTORY_INPLACE=2 VICTORY_INPLACE_MORE=1` (page pool as SEC_COMMIT + zero-copy) | ~4,500 | **7.5** | **133.4** | ~345 | — | **50.0 ms / 123.7 MB** | — | ✅ **the first measured improvement**: median full frame 133.4 against 145.9, best frame 8.2 fps / 121.8 ms, copies block 75.4 → 50.0 ms at the same draw count |
| 20 | the win + `VICTORY_PIPELINE_PUMP=16` + `VICTORY_PASS_TIME=1` | 4,500 | **8.3** | **120.0** | **346** | — | — | — | ⚠️ best frame of the session (8.3 fps / 120.0 ms) but the pump does **not** cut drops (338-349 against 345-356), and the pass-time instrument printed nothing in 109 frames |
| 21 | the win + `VICTORY_INPLACE_VIEW=1` (place the page-pool view memory too) | ~4,500 | 7.6 | 131.3 | 349 | — | **48.1 ms / 116.8 MB** | **49.3 ms / 58.5 MB** | copy-back falls 73.9 to 49.3 ms and the best frame is 117.5 ms; the median only moves to 131.3 because copy-back overlaps the card on the relay thread |
| 22 | release build with both wins (two attempts, the second driving real keystrokes every 15 s) | **13** | — | — | — | — | — | — | ❌ menu only both times: the release build does not take synthetic input the way the diagnostics build does, so it never reaches the scene. Release is unmeasurable by this harness until that is solved; the diagnostics figures stand. |
| 23 | `VICTORY_CARD_BUFFERS_MB=999999` (route the big buffers through placement too) | ~3,400 | 6.9 | 146.4 | **488** | — | 47.6 ms / 123.0 MB | 49.2 ms / 59.5 MB | ❌ worse on every count: frame 146.4 against 131.3 and 488 dropped against 348, with copies unchanged. The big class belongs in card memory; placement does not pay for it. |
| 24 | `VICTORY_PORT_CONSTANTS=1` (the title's 32-bit heap in place, upstream 86dc1097 ported) | 3,689 | — | — | — | — | unchanged | unchanged | ❌ the port runs and reports `0 regions`, `refused: 2 not one stretch of a section`: our allocator hands the title pages our section registry cannot present as one contiguous stretch, so nothing can be placed. The mechanism is right; the allocator has to expose the heap as one stretch first. |
| 25 | the confirmed set only (`run_play.ps1`, build 20025213) + `fsrQuality=3` | 3,962 | **7.60** | **132.1** | best **111.3 ms / 9.00 fps** over 63 full frames; no crash dumps |
| 26 | sampler-heap flag always set (`pipeline-samplers` d1e1af34, on 20025213), `run_play.ps1`, `fsrQuality=3` | **4,706** | 7.3 | 136.6 | **344** | — | — | — | ✅ refused pipelines 311 → 7; 19 % more draws for 3 % more frame time, dropped 478 → 344; best 124.7 ms / 8.0 fps at 4,358; 135 frames, no crash. The debug layer named the cause (`Shader uses sampler descriptor heap indexing, but root signature is missing ... SAMPLER_HEAP_DIRECTLY_INDEXED`, 299 times): the earlier 'fix' that dropped the flag beside static samplers caused the refusals. Compilers ran 20:52-20:54 during loading only. |
| 27 | as 26 with `fsrQuality=4` | 5,390 | 6.9 | 145.3 | 392 | — | — | — | ✅ **not black**: the world renders (capture `fsr4-fixed-211141.png`); the black screen of 19:27 was on the build that refused ~300 compute pipelines. No gain while the frame waits on copies; render targets still logged at 2560x1440. 32 frames (run ended early). Kept `fsrQuality=3`. |
| — | (invalid) sampler-fix run 20:34 | ~5,000 | 2.3 | 442 | 367 | — | — | — | ❌ **not a measurement**: four Hermes `verify-build.sh` cargo builds started mid-run; the submit thread got 80 ms of CPU per 319 ms frame. Check `tasklist` for `rustc`/`cargo` before trusting any run. |
| 28 | zero-copy step 1 `VICTORY_NO_COPYBACK=alt` (merged test build) | ~4,900 | — | — | ~350 | — | — | — | ⚠️ the switch turned itself off: late waits are on by default via the relay. Side findings: the title presents **1 display plane** (HUD loss is not planes); **0 FidelityFX API calls** (FSR runs another way). |
| 29 | as 28 with `VICTORY_RELAY=0` so zero-copy engages (in-run A/B, 7 cycles) | 4,938 / 4,889 | 5.0 / 0.9 | **198.4** / 1,066.8 | 347 / 355 | — | ~600 MB skipped, 5–10 MB copied back | — | ⚠️ zero-copy works and the picture is intact (screenshots), but only with the relay off, which cripples the baseline; slower than the default. Fixed since in `uma-relay` (8a418757). |
| 30 | **`VICTORY_ASYNC_PRESENT=1`** baseline + `VICTORY_GATHER_BY_COPY=alt` | 4,960 | 7.3 | 136.4 | 347 | — | — | — | ✅ ASYNC_PRESENT promoted to the launcher (won in-run twice: 0.890, 0.910); GATHER_BY_COPY **neutral** (0.995 over 8 cycles), rejected. Best 8.0 fps / 125.2 ms. |
| 31 | + `BACK_FEWER_WAITS`, `BACK_POOL`, `BACK_REWATCH` | — | — | — | — | — | — | — | ❌ crash in the title's code before the world (null read at 0x2c); suspect REWATCH (it changes which pages count as current). Bisecting. |
| 32 | + `BACK_FEWER_WAITS` + `BACK_POOL` (no rewatch) | 4,787 | 6.8 | 146.6 | 345 | — | — | — | ❌ no crash (so **REWATCH caused run 31's crash**), no gain: card waits stayed ~7 a frame. Not promoted. |
| 33 | zero-copy step 1 with the relay on (`uma-relay` merged), `NO_COPYBACK=alt` | 4,972 | 5.8 | 172.9 (on) | 340 | 39 ms | **42 ms / 51 MB** (was 59 / 124) | **8.7 ms / 6 MB** (was 47 / 68) | ⚠️ copy-back and uploads fell as designed, picture intact, no crash, but **preparing grew 71.5 → 120 ms** (owned-range comparison); net slower. `alt` is unfair for this switch (OFF phases ~1,000 ms from re-syncing at each flip). Fix in progress. |
| 34 | **control** on the merged build (`17196a3c`; confirmed set incl. ASYNC_PRESENT) | 4,729 | **7.2** | **138.0** | 334 | 46 ms | 37.5 ms / 97 MB | 33.6 ms / 54 MB | the reference for the runs below. Best 8.3 fps / 120.6 ms. |
| 35 | zero-copy step 1 fully on (`NO_COPYBACK=1`) after the preparing fix | 4,770 | 5.1 | 196.9 | 352 | 57 ms | 44.8 ms / 55 MB | **12.3 ms / 6 MB** | ❌ still slower: copy-back −21 ms and card hash lists 28 → 15 ms as designed, but submit-thread CPU 33 → **135 ms** in uninstrumented code (the new counters show ~1.5 ms). Next: a sampling-profiler run. |
| 40 | fork port 6 (`VICTORY_PORT_GUARD=1`), own session | 4,929 | 7.4 | 134.3 | 346 | — | — | — | ⚠️ no crash, slightly faster at 4 % more draws, but inside the ~7 % run-to-run noise; not promoted yet (best 8.3 fps / 120.7 ms). |
| 41 | FSR draw trace (port 6 on) | 4,974 | 7.5 | 133.7 | 347 | — | — | — | ✅ diagnostic: found the 1440p→4K pass (draws `ea212fca1f658aef` + `1579833b3d76cfb1`; reads tonemapped RGBA8 colour + depth, writes 4K). |
| 42 | zero-copy step 1 (`NO_COPYBACK=1`) + sampling profiler | 4,743 | 4.9 | 202.1 | 347 | — | — | — | ✅ diagnostic (profiler overhead): the extra submit CPU is **36 ms/frame of driver work recording per-range sync commands** in the card-copy update and **16 ms in the free-range update**; sent to the zero-copy work. |
| 43 | DLSS stand-in aimed at the draw traced in run 41 | 4,607 | 7.4 | 134.8 | 342 | — | — | — | ⚠️ clean, level with the control, but **DLSS never engaged**: that draw is the loading-screen upscale (frames 1,428–1,446 only). The world's 1440p→4K pass is draw `0309a446e1a48352` + `4f4ec7b16673362f`. A test compile overlapped the run's start-up (loading only). |
| 44 | DLSS stand-in aimed at the world upscale draw `0309a446…`+`4f4ec7b1…` | 4,508 | 7.6 | 132.2 | 346 | — | — | — | ⚠️ clean, but DLSS never started: the loader picked the driver's NGX runtime and found no evaluate entry point. Loader fix in progress. |
| 45 | **control on the merged build** `2261b969` (upstream port, wave64 scoped, 16-bit, read fetch, zero-copy fixes, ordered copies; all switches at default) | 4,849 | 6.4 | 155.5 | 375 | — | — | — | ⚠️ clean, but ~17 ms slower than run 34 at 3 % more draws: a suspected regression in the merge. First suspect: the port's new `VICTORY_PAGE_WRITES` default (black-face texture fix). A check run is queued. Best 7.6 fps / 131.1 ms. |
| 46 | zero-copy (`NO_COPYBACK=1`) with scatter + in-place compare, on `2261b969` | 4,804 | 0.8 | 1,225.6 | 509 | — | — | — | ❌ **invalid: crashed** after ~4 min (null read in the title's code at a new site); frames ~1.2 s before that. Sent back for a fix. |
| 47 | ordered copies `VICTORY_GPU_ORDERED_COPY=alt` on `2261b969` | 4,527 | 6.8 | 147.3 | 297 | — | — | — | ⚖️ clean; in-run on/off 0.988 (neutral). The landing wait fell 56 → 5 ms in ON phases, but ~6,700 moved fills became ~13,000 separate steps (+19 ms submit CPU). Batched fix built. Two builders' compiles overlapped this run. |
| 48 | `VICTORY_PAGE_WRITES=guards` (old default) on `2261b969` | 4,685 | 6.8 | 146.2 | 339 | — | — | — | ⚖️ clean; vs run 45's 155.5 ms, but different sessions, so not conclusive. The black-face fix stays on. |
| 49 | **wide card hashing** `VICTORY_CARD_HASH_WIDE=alt` | 4,539 | 7.7 | 130.1 | 342 | — | — | — | ✅ **won in-run: on/off 0.921 over 8 cycles; ON 121.3 ms at 4,604 draws vs OFF 132.9 at 4,722.** Promoted to the confirmed set. Best world median so far. |
| 50 | `VICTORY_VIEW_KNOWN=alt` | 4,857 | 7.3 | 137.3 | 344 | — | — | — | ⚖️ clean, neutral (1.018). Not promoted. Ran without the build hold. |
| 51 | `VICTORY_ABSENT_ONCE=alt` | 4,680 | 7.4 | 135.9 | 342 | — | — | — | ❌ clean, slower (1.044). Not promoted. Ran without the build hold. |
| 52 | control with sampling profiler (matching symbols) | 4,637 | 7.5 | 132.9 | 341 | — | — | — | ✅ clean; profile kept for analysis. Ran without the build hold. |
| 53 | present ticket `VICTORY_PRESENT_TICKET=alt` on `00552a92` (wide hashing in the set) | 4,823 | 7.9 | 126.3 | 346 | — | — | — | ⚖️ clean, neutral (0.989). Not promoted. |
| 54 | wide prepare `VICTORY_WIDE_PREPARE=alt` | 4,359 | 8.0 | 125.7 | 339 | — | — | — | ⚖️ clean, neutral (1.012). Not promoted. |
| 55 | guard epochs + one pass `VICTORY_GUARD_EPOCH=alt` | 4,453 | 7.6 | 131.0 | 341 | — | — | — | ⚖️ clean, neutral (1.001). Guards still ~13 ms: epochs answer ~430 askings a frame, ~2,300 still walk the guards. Sent back. |
| 56 | GPU upload heaps `VICTORY_GPU_UPLOAD_HEAP=alt` | 4,810 | 3.8 | 260.0 | 344 | — | — | — | ❌ clean but 2× slower; ON drops ~1,370 draws. Likely video-memory pressure from texture staging in VRAM. Sent back. |
| 57 | zero-copy bisect: `NO_COPYBACK=1` with the command-processor read fetch off (`NO_COPYBACK_FETCH=0`) on `00552a92` | 4,808 | 7.0 | 143.3 | 339 | — | — | — | ✅ clean, **no crash** (run 46 crashed with the fetch on, at ~1.2 s frames). Points the crash and the 900 ms frames at the synchronous read fetch. Still slower than the ~126 ms control. |
