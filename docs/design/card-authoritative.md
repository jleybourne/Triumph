# Card-authoritative placed memory

Design only. Nothing built or run for this document. Tree read: `wt-bisect` (fast-integ `2261b969`),
`crates/victory-gpu/src` (`host.rs`, `shadow.rs`, `backskip.rs`, `nohash.rs`, `ordered.rs`, `watch.rs`,
`memory.rs`, `queue.rs`, `queue2.rs`, `plaincopy.rs`, `backreads.rs`), `victory-loader/src/windows.rs`, and the
parallel branch `wt-gpumod` (`gpumod.rs`, same base). Inputs: `research-native-paths-2.md` (item 1 of its ranking),
`design/uma.md`, `design/tracking-cost.md`, `design/card-budget.md`, `research-coherency-prior-art.md`.

Labels: **[M]** measured in a committed log or profile, **[C]** read in code, **[E]** estimate with the reasoning
given, **[?]** unknown until the named run. Every ms figure without [M] is an estimate.

## 0. Summary

- **The idea in one line:** the existing merge of shader writes into title memory (`Shadow::card_merge`,
  `relay_merge`: only bytes that differ from what was last sent) moves from *landing time, for every flight* to
  *first CPU touch, per page*. Until the CPU touches a page, the card's bytes are the only true ones and the CPU's
  page is `PAGE_NOACCESS`.
- Zero-copy step 1 (`VICTORY_NO_COPYBACK`) already makes the card "own" most shader-written ranges, but with no
  safety net: a CPU read of an owned byte silently sees stale data, so every class that *might* be read stays on
  the copy-back list (`backskip::Why`). Card-authoritative (CA) adds the net (a read fault that fetches), then a
  single card master per title byte (the arena), then removes CA pages from write tracking altogether.
- **What only CA removes** (on top of step 1, GPU-modified lists and ordered copies, which it builds on):
  the kept copy-back classes, the cross-copy sync through title memory (windows and nested buffers: 3,024 MB of
  windows over 2,852 MB of title memory [M]), refresh bookkeeping for GPU-owned bytes, and the watch askings for
  them (1,989 askings, 26.3 ms a frame; guards 13.6 ms [M, run 17]).
- **Programme estimate:** 40-90 ms off the control, *including* the foundations being built in parallel; the part
  unique to CA is est. **20-50 ms** [E]. It does not reach 33 ms by itself (card-budget.md: the card is ~80 ms
  of work today, ~35 ms after its own cuts).
- **First stage to build:** stage 1, `VICTORY_CA_FAULT` (read-fault net over step 1's owned ranges, and those
  pages leave the watch askings), with the stage 0 census in the same build. Est. **-6 to -14 ms** a frame on top
  of step 1 + `VICTORY_GPU_MODIFIED`, and it is what makes step 1's -35 to -40 ms safe to default.
- **Title write-site hooks:** the guard-fault samples of run 42's profile come from **18 title functions**
  (22 site/route pairs); the top 7 hold ~90 %. **~34 %** of that time arrives through the CRT `memcpy`/`memmove`
  that our loader binds as host imports, so a wrapper there is a free, signature-less dirty list for about a third
  of view-memory writes. The rest are inline stores in ~12 functions: not free (per-build signatures plus argument
  RE). Stage 6, optional.

## 1. Where the time is, and what is already being built

| cost (control unless noted) | ms / frame | source |
|---|---:|---|
| copy-back (shader writes merged into title memory) | 42.8 (56 MB) | [M] uma.md, diagnostics build |
| copies into host copies (refresh) | 23.5 (72 MB) | [M] uma.md |
| card: send-and-hash + compare | 28.9 + 5.5 card | [M] card-budget.md, run 34 |
| watch askings (incl. guards) / guards alone | 26.3 / 13.6, 1,989 askings, 420 guard faults | [M] run 17 copystat (`victory-run-20261127-035835`) |
| refreshes | 20,329 over 17.8 GB; 7.5 of 7.6 GB checked unchanged | [M] run 17 |
| wait for shader-written copy sources | 51-56 submit thread | [M] `ordered.rs` header |
| title-thread lock waits in the fault handler | 5.4 | [M] tracking-cost.md, run 42 profile |
| copy-back readers other than us | 0.19 % of stores | [M] run 277 sample (README) |

Built or being built in parallel (CA consumes all four):

| piece | switch | what CA takes from it |
|---|---|---|
| relay (flights landed by their own thread, in order) | `VICTORY_RELAY` (`host.rs` `relay_worker`, `relay_land`) | the landing point where a page becomes card-owned; per-flight fence numbers |
| zero-copy step 1 | `VICTORY_NO_COPYBACK` (`backskip.rs`) | "owned" ranges, the `Why` exclusion classes, the record of what was last sent (`Shadow::copy_owned`) |
| zero-copy step 2 | `VICTORY_UMA_NO_HASH` (`nohash.rs`) | no hashing of bytes sent into owned ranges |
| GPU-modified range lists | `VICTORY_GPU_MODIFIED` (`wt-gpumod`, `gpumod.rs`) | the exact set of ranges each flight may have written, tagged with its fence: the ownership set |
| ordered copies | `VICTORY_GPU_ORDERED_COPY` (`ordered.rs`) | command-processor copies whose source the card owns run on the card; their destination joins the ownership set |
| command-processor fetch | `wt-uma-cpread` (`cp_fetch`, 64-byte lines) | fetch of owned bytes the command processor reads |

Note: card-budget.md records that step 1 today costs **+100 ms submit CPU** (runs 35, 42: per-range sync commands).
CA must not start until that is fixed (the GPU-modified lists are the intended fix), or every stage below inherits
it.

## 2. Which memory qualifies

### 2.1 Classes

| class | example | home | why |
|---|---|---|---|
| **GPU-written, GPU-read, CPU never touches** | skinning out, particles, culling output, indirect arguments the GPU writes and our clamp reads on the card, compute scratch | **CA** (core) | the copy-back here serves nobody (0.19 %) |
| GPU-written structures reached by raw addresses | the ray-tracing builder's nodes (`memory.rs` numbered copies) | **CA**, stage 4 | `memory::ready` today refreshes every numbered copy and declares all written per dispatch |
| destinations of card-side title copies | `ordered.rs` copies | **CA**, stage 4 | the source is card-owned; flushing it to the CPU is the 51-56 ms stall |
| CPU-written every frame | constant rings, dynamic vertex/index data, upload streams | CPU-authoritative (today's refresh; later upload heap, uma.md step 6) | the CPU is the producer |
| written once by our loaders, then GPU-read | DirectStorage / fread destinations (meshes) | CPU-authoritative, static | no GPU writes, nothing to own; already a free dirty list (`watch::writing`) |
| command-processor touched | bottom-of-pipe writes, query results, memory values lists wait for, read-backs | CPU-authoritative (`backskip` `Keep::Parts`) | the CPU polls them; tiny |
| mapped for read by the title | `Map` with a read range | CPU-authoritative at first; stage 5 may make them CA with an early fetch at `Map` | the read is announced |
| texture kinds copied back | the ocean's height/displacement (`texback.rs`) | copy-back kept; stage 5 candidate (frame-delayed readback) | known CPU reader |
| memory under a live title texture | texture bytes the title also writes as a buffer | excluded at first | host textures are detiled from title memory on the CPU (`texture.rs`) |
| pages with more than `MANY_VIEWS` views | heavily aliased page-pool pages | excluded | every view must be protected; above the limit the guard does not cover all |

### 2.2 Classifying at runtime

One state byte per 64 KiB page of title address space, in a flat table written at allocate/map/unmap/free and at
landings, read without a lock (the same structure as tracking-cost.md option E; 16 MB for 40 bits, or lazy pages).

States: `Cpu` (today: CPU-authoritative, tracked by write watch or guard), `Both` (CPU bytes equal the card's,
read-only guard for writes), `Card` (the card's bytes are newer; CPU page `PAGE_NOACCESS`), `Excluded`.

A page becomes `Card` when a flight that wrote it **lands** (relay worker), if all hold:
1. a `GpuWrite` step of that flight covers part of it (GPU-modified list, or a step-1 owned range);
2. none of `backskip::Why` applies (the step-1 classes above, unchanged);
3. its read-fault score is below the threshold (stage 5: decays per frame; default "no read fault in the last
   64 frames");
4. it is not `Excluded`, and the arena has room (stage 2 on).

Memory attributes help only as a hint: the run 17 copystat shows the hot copies split between attribute classes
`0x40000` and `0x20000` [M]; what those classes mean for CPU access is [?] until the census tabulates faults by
attribute (tracking-cost.md option F).

## 3. The fault path: a CPU read of a card-owned page

### 3.1 Why a read fault is now allowed

uma.md section 5 says "never detect title reads with `PAGE_NOACCESS`": our handler (`watch.rs` `install`, line ~2693)
takes only write faults (`ExceptionInformation[0] != WRITE` → continue search), so a read fault reaches the
title and kills it. CA adds a handler that owns the read faults **on pages it protected itself**, found in the flat
table. The rule that stays: no fault on a page *we* protected may ever be passed on. If the fetch fails, the
handler unprotects, serves the CPU bytes it has, and logs (verification turns that into a stop).

### 3.2 Sequence

1. **Handler** (vectored, first in line, beside `watch.rs`'s and `plaincopy.rs`'s; added with
   `AddVectoredExceptionHandler(1, ..)` after them so it runs first): access violation, read or write, address in a
   `Card` page per the table. Anything else → continue search. No allocation, no environment reads in the handler
   (the `light_kept()` lesson in `install`).
2. **Claim** the page (the per-block claim of port 6, `open_shared`), so other threads faulting on it wait for
   this fetch instead of starting their own.
3. **Request** to the fetch thread: page, the GPU-modified ranges of that page and each range's writer fence. The
   handler blocks on an event. The fetch thread coalesces requests that arrive within ~20 µs and neighbouring
   ranges up to 256 KiB.
4. **Fetch** on the copy queue (`queue2.rs` already makes `D3D12_COMMAND_LIST_TYPE_COPY` lists): a GPU-side
   `Wait` on the host fence value of the latest landed writer of those ranges (not on everything in the air), then
   `CopyBufferRegion` of **only the listed ranges** (byte-exact, from the GPU-modified list) into a persistently
   mapped read-back ring (8-16 MiB), signal, CPU wait.
5. **Merge** into title memory with the existing rule: a byte goes in only where the card's byte differs from the
   record of what was last sent (`card_merge`/`relay_merge` logic, `Owned::sent`). This keeps title writes made
   since the last send (they differ from `sent` on the CPU side and are not overwritten), so a page with pending
   CPU writes needs no special case.
   - Section views: write through **our own view** of the section (the port.rs / backreads technique), so the
     title's address stays `NOACCESS` until the bytes are in.
   - Private memory: `VirtualProtect(READWRITE)` under the claim, copy, `READONLY`. A title write from another
     thread in that window is not lost: write watch still sets the kernel dirty bit, and the next refresh sends
     only bytes that differ from `sent`.
6. **Protect** every view of the page `READONLY` (state `Both`; writes are still guarded), update `sent` to the
   fetched bytes, release the claim, return `EXCEPTION_CONTINUE_EXECUTION`.
7. **Count**: faults, bytes, µs, and the page's read-fault score.

### 3.3 Ordering rules

- A page becomes `Card` only at the **landing** of its writer, so a fault can only ever ask for landed writes.
  The title is told of its fences after landing (relay order), so it cannot legitimately expect anything newer.
- A later flight in the air that writes the same page may be running while the copy queue reads it. The fetched
  bytes may then mix old and new; on the console the title would race the same way (it reads before that fence).
  When that flight lands, the page goes back to `Card` with its ranges.
- **Never wait for landing in the handler.** A title thread that faults may be the one whose CPU-side signal the
  card waits on; a landing wait could deadlock. The GPU-side `Wait` targets only fences already completed or
  in the queue ahead; the CPU wait has a timeout (default 50 ms) that falls back to step 3.1's rule and logs.
- The handler never takes the submission lock, the guards' exclusive lock, or anything the relay worker holds while
  waiting for the card.

### 3.4 Cost [E]

Exception dispatch and handler ~5-10 µs; copy-queue submit + GPU wait + signal + CPU wake ~30-80 µs; 64 KiB over
PCIe 3.0 ~6 µs; merge ~5 µs. About **50-150 µs per fault**. Budget: ≤ 20 faults a frame in steady state
(≤ 3 ms, on title threads, spread). Above ~50 a frame, stage 5's learning must turn the page's class around.

### 3.5 Readers that do not fault into the handler

| reader | what happens on a `NOACCESS` page | rule |
|---|---|---|
| our refresh (`copy_mapped` → `ReadProcessMemory`) | the kernel copy fails; today's fallback **fills zeros** and could upload them over the card's bytes | refresh must skip `Card` pages (nothing to send: the card is newer). Assert in `update_free`/`copy_stale` |
| `plaincopy` guarded copy | read fault at its own instruction → returns 0 → kernel fallback | same rule |
| command-processor reads on the CPU (`defer_copy`, `defer_fill`, `wait_for`) | stale or failing | consult the table: source `Card` → card-side copy (ordered copy) or `cp_fetch` first |
| texture detile from title memory (`texture.rs`) | fault from our thread | pages under live textures are `Excluded` (2.1); stage 4 may detile on the card instead |
| title passing a `Card` pointer to a system call (WriteFile, sockets) | the call fails with an access violation status, no exception | HLE wrappers that take title buffers call `ca::fetch(range)` first; the verification counts failures |
| debugger / crash reporter (`crash.rs`) | reads fail | harmless |

## 4. CPU writes to card-owned pages

| case | action | cost |
|---|---|---|
| title write to a `Both` page | today's guard fault → `Cpu` (unchanged) | as today |
| title write to a `Card` page, default | treat as a read fault (fetch, merge), then open for write → `Cpu` | one fetch |
| title write to a `Card` page whose class is "rewritten before read" (stage 5 learns it: after a fetch, the next refresh found ≥ 90 % of the page changed) | open `READWRITE` with **no fetch** → `Cpu` with a "stale under" mark; the next refresh sends only bytes that differ from `sent` (step 1's rule), and the card keeps the rest | no fetch. Hazard: a CPU write of the same value the CPU last sent is missed where the GPU changed it since (uma.md's "stale CPU view", step 1's known risk) |
| our own writers (`watch::writing`, `title_memory_written`, `write_physical`, DirectStorage page destinations) | call `ca::claim_write(at, len)` before writing: pages covered whole → `Cpu`, no fetch, card bytes dropped (prior art idea 4: a write discards); partial pages → fetch first | a lookup per write |
| `VirtualFree` / decommit / unmap of a `Card` page | drop ownership (and the arena tiles, stage 2); a remap (`title_memory_remapped`) follows the physical page, not the address | at map time |

A later GPU write declared over a `Cpu` or `Both` page makes it `Card` again at the writer's landing; no bytes
move.

## 5. Placement: the arena (stage 2)

Per 4 GiB window of title VA that holds CA pages: one **reserved buffer** (4 GiB - 64 KiB, the cap uma.md gives
for heaps and raw view offsets), with 64 KiB tiles mapped from our own card heaps by `UpdateTileMappings`:

- **page pool:** physical page *n* ↔ heap tile *n*. When the title maps the same physical page at several
  addresses (`XMemMapPhysicalPages`), the same heap tile is mapped at several tile positions: the console's
  aliasing reproduced on the card, with no copies between aliases (today each window and buffer copy holds its own
  bytes, and they learn of each other's shader writes only through title memory: a `backskip` keep class).
- **private memory:** tiles are allocated on the first GPU write to that page.
- **bindings** of CA pages (root views, VB/IB, table views, indirect args) resolve to `arena + (va - window)`;
  raw shader addresses keep the 64 KiB page directory (`memory.rs`), filled from the arena at map time.
- **budget:** the arena replaces per-copy card buffers and windows for CA pages (7,600 card buffers 2,240 MB and
  727 windows 3,024 MB alive in run 17 [M]); cap it at what those held (`CARD_BUFFERS_MB`), card in use was
  9.5 GB of an 11 GB budget [M]. Over the cap, pages stay `Cpu`.
- **only our heaps.** uma.md's rule stands: no `UpdateTileMappings` names an opened (system) heap. Assert on heap
  kind, as in `tiles.rs`.
- Tile tier and reserved-buffer size limits on this card are [?]: check `D3D12_FEATURE_DATA_D3D12_OPTIONS`
  `TiledResourcesTier` and the GPU VA bits per resource at start, and log them.

## 6. Interaction with the parallel work

- **Relay.** `relay_land` gains one branch: for CA-eligible ranges of a landed flight, record (page → ranges,
  fence) and protect `NOACCESS` (batched runs, every view), instead of fetching and merging. Non-CA ranges land as
  today. The fence signal to the title still follows. `VICTORY_LATE_WAIT` stage 2's `merge_under_writes` is
  unchanged for `Cpu` pages; for `Card` pages the merge happens at fault time (3.2 step 5) with the same rule.
- **Zero-copy step 1.** CA is step 1 with a net. Stage 1 reuses `own`, `copy_owned`, `Owned::sent` and every `Why`
  class. With the net in place, classes that are kept today *only because a read was possible* can move to CA:
  `Map` for read (fetch at `Map`), `VICTORY_NO_COPYBACK_KEEP`, and the `missed`/`hot_frames` whole-range class.
  Step 2 (`nohash`) applies unchanged.
- **GPU-modified lists.** They are the ownership set. For CA, over-approximation is cheap: a listed range nobody
  reads costs one protection, never a fetch. That inverts the lists' main cost (fetching whole listed ranges at
  landing). `gpumod.rs`'s fallbacks (root UAVs, the raw memory directory) mean "whole copy owned"; for CA that is
  safe (more pages protected) but may raise faults; the census sizes it.
- **Ordered copies.** With CA a title copy whose source pages are `Card` must go on the card (no CPU path can read
  them). The ordered-copy cases that still wait (`ordered::plan`) become fetches of the source ranges instead of
  full waits. Destinations join the ownership set.
- **`cp_fetch`.** Becomes a special case of the fault fetch (same copy-queue path, called by us, not by a fault).
- **Tracking work (tracking-cost.md A-E).** Independent and still worth doing for `Cpu` pages. Stage 1 removes
  `Card` pages from the askings entirely, which shrinks B's and C's work further.

## 7. Failure modes and detection

| # | failure | effect | detection |
|---|---|---|---|
| 1 | a GPU write not declared (raw address, root UAV past its range, missed step) | CPU reads stale bytes without a fault (page never became `Card`) | verification: wide card hash (`VICTORY_CARD_HASH_WIDE`) of sampled copies every N frames, compared with the declared set; log any changed piece outside it. Exists today in another form; must stay 0 |
| 2 | a reader that bypasses the handler (our kernel copies, system calls) | zeros uploaded over card bytes, or a failed call | counters: `ReadProcessMemory` failures on `Card` pages (must be 0); HLE status `0xC0000005` from title-buffer calls; refresh asked for a `Card` page (assert) |
| 3 | our own code faulting into the handler | slow, or a deadlock under the submission lock | the handler logs faults whose `Rip` is in victory.exe, by address, and a thread-local "holds submission lock" flag turns them into a stop in verification |
| 4 | fault storm (bad class) | title threads stall | faults a frame and µs in the frame log; per-page score; stage 5 demotes. Gate: ≤ 20 a frame |
| 5 | same-value CPU write over a GPU-changed byte (no-fetch write case) | the GPU byte wins where the CPU's should | only in the learned no-fetch class; verification fetches anyway and compares |
| 6 | aliased view left unprotected | read through the other view sees stale bytes | verification: `VirtualQuery` every view of a sampled `Card` page once a second |
| 7 | handler wait times out (card blocked on a CPU signal) | stale read served | count; any non-zero is a bug report with the page and fence |
| 8 | remap/free of a `Card` page not followed | wrong tiles / leaked ownership | assert in `title_memory_remapped`, xmem free path |
| 9 | arena over budget / tile mapping failure | device removal risk if mapped into the wrong heap kind | heap-kind assert; fall back to `Cpu` |
| 10 | protection churn | VirtualProtect cost (prior art: 86-809 a frame elsewhere) | protects a frame and ms in copystat |

Open item: run 18 (`run18_nocb.out`, step 1 on) ended in a title access violation (read of address 0); its cause is
not established here. Stage 1 must start from a step-1 configuration that completes a 600 s run.

**Verification mode (`VICTORY_CA_VERIFY=1`, diagnostics builds):** does everything CA does and in addition
(a) keeps the old copy-back into a private buffer for `Card` pages and, on each fault, compares the fetched
bytes with it (mismatch = ordering or declaration bug); (b) failure checks 1, 2, 3, 6 above; (c) once a second
logs `ca: pages card N, both N, faults N (µs), fetch MB, no-fetch writes N, bypass N, undeclared N, timeouts N`.
Pass for every stage: bypass, undeclared, timeouts and mismatches all 0 over a 600 s world run, plus the usual
screenshots (water, boats, skinned characters).

## 8. Staged plan

Every stage: diagnostics build, `run_viewplace.ps1` A/B, 600 s, one switch per run, `fps · ms · draws · dropped`,
screenshots, the `ca:` line, and `alt` (twenty seconds on, twenty off) where the stage allows it. Estimates are
submit-thread ms unless marked, and are **on top of step 1 + step 2 + `VICTORY_GPU_MODIFIED` + ordered copies**
(they overlap with those, so the column "vs control" is the programme view).

| stage | switch | change | est. ms (unique to CA) | proved by |
|---|---|---|---|---|
| 0 | `VICTORY_CA_CENSUS=1` | dry run: the state table is kept and transitions simulated at landings; reads seen through the backreads working-set trim (1 page in 8, no faults) and guard faults for writes; tabulate by attribute class and `Why` | 0 (diagnostics cost only) [E] | predicted `Card` MB vs copy-back MB (want ≥ 80 %), predicted read faults a frame (want ≤ 20), protects a frame, watch askings that would vanish |
| 1 | `VICTORY_CA_FAULT=1` (needs `NO_COPYBACK`, `GPU_MODIFIED`) | read/write fault handler and fetch thread (section 3), protect at landing, `Card` pages answered "unchanged" with no asking or guard walk, refresh skips them, `claim_write` in our writers | **-6 to -14** (watch askings and guard walks for owned ranges, part of 26.3 + 5.4 ms; kept classes that move to CA) [E]; fault cost +0.5-3 on title threads | copystat `watches` count and ms, `guards` ms, copy-back MB of kept classes; `ca:` line faults ≤ 20, all verification counters 0 |
| 2 | `VICTORY_CA_ARENA=1` | reserved-buffer arena, tiles per physical page, CA bindings to the arena, windows and per-copy card buffers retired for CA pages | **-10 to -25** (refresh bookkeeping of 20k refreshes, cross-copy keep class, part of 23.5 ms copies) [E] | refreshes a frame and MB; "windows ... lie over" MB; card MB in use; submit-thread ms |
| 3 | `VICTORY_CA_PROMOTE=1` | ordered-copy destinations and raw-memory directory pages become CA; `memory::ready` stops refreshing and declaring every numbered copy | **-3 to -10** (incl. what remains of the 51-56 ms source wait) [E] | ordered-copy "kept wait" count; `memory::ready` time; BVH stage still drawn |
| 4 | `VICTORY_CA_TEXTURES=1` | pages under live textures allowed CA; detile reads the arena on the card (GPU detile, research round 2 item 7) | -1 to -4 [E] | textures hashed/changed MB; red-block screenshots |
| 5 | `VICTORY_CA_LEARN=1` | per-page read-fault score: pages read every frame get a frame-delayed async readback after their writer (prior art ideas 9, 10) or go back to `Cpu`; the "rewritten before read" no-fetch class | 0 to -5 (only if stage 1 shows faults) [E] | faults a frame and µs fall; no new verification failures |
| 6 | `VICTORY_CA_MEMCPY_HOOK=1` | loader binds the CRT `memcpy`/`memmove` imports to a wrapper of ours that notes destination pages in tracked memory (section 9) | -1 to -4 title threads (about a third of guard faults and their lock waits) [E]; +? per call | guard faults a frame fall ~30 %; wrapper calls a frame and ns each |

Programme total, vs the control: **40-90 ms** [E], of which the foundations (step 1, lists, ordered copies) carry
roughly half; unique to CA: **20-50 ms**. Stage 1 is the gate for all others: if the census predicts more than ~50
read faults a frame for the ownership set, CA stays at stage 1 for the classes that pass and the rest stays on
copy-back.

## 9. Title write sites: a free dirty list?

Question: if the title writes GPU-visible memory through a few code sites, hooking them would give a precise
dirty list without guards. Counted without reading any title code, from run 42's profile
(`victory_profile_15952.txt.folded`) and the loader's binding report:

- **Method.** In every sampled stack that enters `KiUserExceptionDispatcher` and then the guard handler
  (`victory.exe+0x516eb0`), take the frame just before the dispatcher (the writer), or, when that is a CRT
  routine, the frame before it (its caller). Sum sample weights.
- **Result [M].** 22 distinct site/route pairs, **18 distinct title functions**. Shares of guard-fault handler
  time: top 1 function 39 %, top 7 functions ~90 %, the remaining 11 ~10 %. **~34 %** of the time arrives through
  two CRT entry points (`VCRUNTIME140` at two offsets, called from 6 title functions).
- **Loader.** `victory-loader` binds every import of the image itself (`windows.rs` `bind_imports`; the main image
  binds 88 HLE and 427 host imports, run 17 log [M]). So `memcpy`/`memmove` can be bound to a wrapper of ours:
  no title code, no signatures, no patching. Which import names the two CRT offsets are is [?] (they are most
  likely `memcpy` and `memmove`; confirm from the export table at bind time).
- **Limits of the count.** (1) Only view memory faults; writes to write-watched private memory never fault, so
  their writers are invisible here. (2) Sampling misses rare sites. (3) A wrapper sees every `memcpy`, not only
  GPU memory: each call pays a flat-table lookup (est. 2-5 ns); the call count a frame is [?].
- **Verdict.** The CRT rebind is cheap (days) and gives exact destination ranges for about a third of view-memory
  writes: worth it as stage 6, as a tracking aid. Hooking the ~12 functions with inline stores is not free: it
  needs per-build signatures (kept in a per-build table outside the repo) and the meaning of each function's
  arguments to know what range it wrote. Not recommended. The DirectStorage/fread destinations are already a free
  dirty list (our own writers, `watch::writing`).
- **Measure first (diagnostics, no behaviour change):** `VICTORY_WRITE_SITES=1`, logging the faulting `Rip` of
  every guard fault, bucketed by module offset, plus a few seconds of guards on private memory too, to get the
  true count of sites over a full world run.

## 10. Risks not covered above

- Fetch latency on the copy queue may be higher than estimated on this driver (queue scheduling); measure
  `ca:` µs per fault in stage 1 before relying on the 20-a-frame budget.
- Two vectored handlers that both take faults on the same page (guard for writes, CA for reads/writes): the CA
  handler must run first and handle `Card` pages entirely; the guard handler keeps `Both`/`Cpu`.
- `PAGE_NOACCESS` on write-watched private memory: write watch keeps working, but a kernel write into the page
  (a system call writing into a title buffer) fails. Covered by `claim_write` for our writers; title system
  calls are failure mode 2.
- `VICTORY_LATE_WAIT` timing semantics change (uma.md): same here; CA needs the relay order of fence signals.
