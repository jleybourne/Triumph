# Cheaper tracking of title memory the CPU wrote

Status: design only, no code changed. Tree: `wt-bisect` (fast-integ 2261b969), `crates/victory-gpu/src`.
Frame now 132-138 ms, target 33 ms. Every "est." figure below is an estimate, not a measurement.

Sources:
- Profile of run 42: `victory_profile_15952.txt`/`.folded` (448 s, 3,383 frames, 132.5 ms a frame). Names were
  resolved against a newer PDB, so names are approximate. Callers were matched by stack shape (what each frame calls).
- Run 42's log: `D:\victory-work\logs\victory-run-20261127-025612.log.err`, world steady state 16:29-16:34. Also
  compared with runs 12 (port 6 on, `-023901`), 14 (`-032053`) and 15 (`-033324`).
- `D:\victory-work\research-coherency-prior-art.md`, ideas 2, 4 and 5 (folded in below, marked "prior art").

## 1. How tracking works today

Title GPU memory has two kinds, and which kind a range is gets decided when the memory is allocated
(`victory-hle` `xmem::virtual_alloc`):

| Kind | How it is made | How writes are found |
|---|---|---|
| Write-watched | `VirtualAlloc2` with `MEM_WRITE_WATCH` (every constrained reservation that is not a placeholder) | `GetWriteWatch(RESET)`; the kernel keeps one dirty bit per 4 KiB page |
| View memory | 64 KiB placeholders, with section views mapped in by `XMemMapPhysicalPages` | Guards: each 64 KiB block a copy took is set `PAGE_READONLY`; the first write faults into `watch::install::on_write`, which notes the block written and makes it writable again |

Per refresh (`Shadow::update` → `update_free` → `watch::writes` → `writes_as`):

1. Answered from the journal if the range is inside a span already asked about for this submission
   (`ask_once`/`ask_ahead`, on by default). The submission's write-watched ranges are joined (8 MiB gaps), and one
   `GetWriteWatch` call covers each joined span.
2. Otherwise, if the first block is view memory the guards already know (`known_view`: protected, absent or viewed),
   the kernel is not asked. If not known, `GetWriteWatch` is called and fails for view memory.
3. `guard()` → `guard_at`: if the first block is not known, `is_view` calls `VirtualQuery` in a loop over the
   regions. Then `guard_alone` takes the guards' lock alone. It collects the range's clean blocks out of a
   `BTreeSet` (or Bits with port 6), reports every block that is not clean as written, and makes runs of them
   read-only again (`protect` → `VirtualProtect`, plus every other view of the same physical page). Blocks noted
   absent call `uncommitted_end` → `VirtualQuery`.
4. Faults: `on_write` (port 6: `open_shared`, shared lock plus the block's claim). The fault clears the block's
   protected and clean marks and calls `VirtualProtect(READWRITE)`. If the block was not ours, `writable()` →
   `VirtualQuery`.
5. Side channels into the journal and guards: `writing`/`title_memory_written` (fread, DirectStorage), `wrote`,
   `wrote_own`, `title_memory_remapped`, `title_memory_committed`, `views_mapped`.

### Per-frame costs, world steady state

From the copystat lines of run 42 (16:29-16:34, all frames). Runs 14 and 15 give the same picture.

| Item | Per frame |
|---|---|
| Submissions | 8.5 |
| Refreshes (`update` calls) | ~19,000, over 17.7 GB of ranges |
| Bytes checked / unchanged | 7.4 GB / 7.35 GB (about 48 MB actually changed) |
| Askings (`watches`) | ~1,900, 14-16 ms (WatchUs includes guard time) |
| Of which went to the guards because the range is known view memory | ~1,720 |
| Kernel askings that failed (view memory not known yet) | ~205 |
| View ranges left out of the submission's asking ahead | ~2,400 (they fall back to `writes` at every flush) |
| `guards` (GuardUs) | 10.6-11.4 ms |
| Guard faults | 310-350 (runs 14/15: 430-460) |
| Process page faults | ~4,800 |
| Run 12, port 6 on | guards 12.4-12.7 ms, failed 190, known 2,050. Port 6 did not lower the guard cost |

From the profile, run 42, in ms a frame (with = including callees):

| Where | ms | Notes |
|---|---|---|
| submit: `writes` (≈ `flagged_pieces`, 0x4c5bf0) with | 7.6 | the user's 6.8 figure |
| submit: `writes_as` (≈ 0x5089c0) with | 7.8 | |
| ... `VirtualQuery` inside guard (`is_view` / `uncommitted_end`) | 3.3 | of the 3.8 ms of NtQueryVirtualMemory on the submit thread |
| ... `VirtualProtect` (guard again) | 1.3 | |
| ... `GetWriteWatch` | 0.3 here, 1.5 in total | 0.9 ms in the per-submission asking (`ask_once`) |
| ... walking the guard sets, Vec building, lock | ~2.5 | own time plus small callees |
| busy (title) threads: fault handler waiting on the guards' lock (0x516eb0 → wait on address) | **5.4** | title threads stall while the submit thread holds the lock in `guard_alone` |
| record threads: another vectored handler calling `VirtualQuery` (0x7d3340 → 0x7d9220) | 3.9 | Probably the console-instruction emulator's `writable_to` (`victory-loader/console_insn.rs`), not tracking. Flagged under "other findings" |
| record threads: `KiUserExceptionDispatcher` own | 14.4 | Mostly title+0x4fb0ad0. The fault count does not fit guard faults (about 40 µs each), so this is most likely instruction emulation. Unattributed |

In total, tracking costs about **8 ms of submit-thread time a frame** plus about **5-7 ms on title threads** (lock
waits, traps, re-protection).

The structural problem: 7.35 of 7.4 GB checked a frame is unchanged, yet each of ~1,900 askings takes the exclusive
lock and walks its blocks. ~205 askings a frame also pay a failed kernel call plus a `VirtualQuery` page-table walk
(the code's own note: 0.6 ms for a 174 MB view). About 400-800 protection changes a frame come from re-guarding
the same hot blocks. Prior art reports 86-809 a frame elsewhere and calls this the main thing to cut.

## 2. Alternatives

Each entry gives the mechanism, the hazards, an estimate, a switch and a test. Estimates are for submit-thread ms
unless they say otherwise. They overlap, so they do not add up. Section 3 gives a combined figure.

### A. Turn on what is already written: `VICTORY_VIEW_KNOWN=1` + `VICTORY_ABSENT_ONCE=1` (batching/removing VirtualQuery, step 1)

- `VIEW_KNOWN` keeps the `viewed` set when a fault opens a block, so `known_view`/`guard_at` skip both the failing
  `GetWriteWatch` and `is_view`'s `VirtualQuery`. Run 235 notes say remap churn defeated it before. `views_noted`
  now keeps blocks viewed across remaps (`title_memory_remapped`, `view_known` branch).
- `ABSENT_ONCE` (with `ABSENT_MS=100` or `told`) answers absent blocks from a note instead of `uncommitted_end`.
- Hazards: a hole filled without the runtime saying so keeps old bytes until the note expires (`told`: until
  `title_memory_committed`/`remapped`). Ordinary commits are told (`virtual_alloc` → `title_memory_committed`).
- Est. **2-3.5 ms** (the 3.3 ms `VirtualQuery` plus the failed kernel calls). Effort: config only.
- Switch: existing. Both have `alt` (twenty seconds on, twenty off) for an A/B test inside one run.
- Test: one run with `VICTORY_VIEW_KNOWN=alt VICTORY_ABSENT_ONCE=1 VICTORY_REFRESH_CHECK=1`. Pass: the copystat
  `failed` count drops to about 0 in the on phases, the profile shows submit-thread `NtQueryVirtualMemory` under
  0.5 ms, and `REFRESH_CHECK` reports no stale copies.

### B. One guard pass per submission (`VICTORY_GUARD_ONCE=1`) (batching)

- What `ask_once` does for write-watched memory, done for view memory. At submission start, collect every view
  range the lists refresh (the ~2,400 now left out of the asking ahead), sort and merge them, and do one guard pass
  under one lock acquisition. Note the found blocks in the journal under the submission's asking number and add the
  merged spans to `j.ahead`, so every later `writes` in the submission answers from the journal. That turns about
  220 guard askings a submission into 1.
- Hazards: the same contract as `ask_once`. A title write after the pass is seen by the next submission. It does
  not matter here: what this submission's lists read was written before submission. Our own writes in between must
  call `wrote` (they already do for `ask_once`). A fault between the pass and a copy still opens the block, and the
  next pass reports it. The merged spans must not include blocks of other copies that are mid-copy-back
  (`pending`); the guard only reports, so over-reporting is safe.
- Est. **4-6 ms** (most of the per-asking overhead and the lock traffic; also cuts the 5.4 ms title-thread lock
  waits by about 200x fewer exclusive acquisitions: est. 3-4 ms on title threads).
- Effort: S-M (`ask_once` gets a view branch; `guard` takes a list).
- Test: a unit test in `watch.rs tests`: guard two overlapping ranges in one pass, write one block, then check that
  the next pass reports it and `writes` inside the span answers from the journal with no lock taken (count via a
  test hook). Run with `VICTORY_GUARD_ONCE=alt VICTORY_REFRESH_CHECK=1`. Pass: copystat `watches` drops from about
  1,900 to tens and `guards` falls.

### C. Region fault epochs: skip clean ranges without a walk (`VICTORY_GUARD_EPOCH=1`) (larger granularity for the check)

- One atomic counter per 32 MiB region of title address space (a flat array, about 2 MB for 47 bits of address
  space, or allocated lazily like `Bits`). Bump it wherever a block can go from clean to not clean: `open_here`,
  `open_shared`, `title_memory_remapped`, `views_mapped`, `writing`, `title_memory_written`. Each host copy
  remembers the epoch sum of its regions at its last asking. If the sum has not changed, nothing in the range was
  written or remapped: answer from the journal with no lock, no `BTreeSet` walk and no Vec.
- This is "larger tracking granularity" done the safe way: coarse for the question "anything new?", still 64 KiB
  for what is copied. The finding behind it: 99.3 % of checked bytes are unchanged.
- Hazards: a path that clears `clean` without bumping the counter would give stale copies. Centralise the bump in
  the two functions that clear `clean`/`protected`, and add a debug assertion that no other code touches those
  sets. A counter that wraps is harmless with 64-bit counters. Blocks never guarded yet (new ranges) have no epoch:
  they always take the slow path once.
- Est. **4-7 ms**. Overlaps with B: B+C together are est. 6-8 ms of the about 8.
- Effort: S. The `PUBLISHED`/light refresh code tried the same thing per block and is unfinished; it crashed in
  the all-on set. Per-region counters need no per-block publishing protocol.
- Test: a unit test that every public entry that opens a block changes the region epoch (table-driven over
  `writing`, `title_memory_written`, `title_memory_remapped`, `views_mapped`, the fault). Run with
  `VICTORY_GUARD_EPOCH=alt VICTORY_REFRESH_CHECK=1` and add a new copystat count, "askings answered by epoch".

### D. Leave hot blocks open (`VICTORY_HOT_BLOCKS=<frames>`) (stop re-protecting hot pages; prior art idea 4/8)

- A block that faulted in N frames running (default 4) is not guarded again. It is noted "hot" and counted as
  written at every asking. Its refresh copies it. For copies whose bytes the card owns (`VICTORY_NO_COPYBACK`) it
  uses the compare path (`copy_owned`, changed runs against `Owned::sent`). Hot blocks cool down after M frames
  with no change in the compare: guard them again then.
- Prior art idea 4 applies directly to hot blocks under GPU-written ranges. A CPU write over bytes the GPU
  modified marks those bytes for upload and drops the GPU-modified mark. It never flushes the GPU bytes back
  first. Our merge path (`merge_under_writes`) already only moves the bytes that differ. Keep that byte-exact
  behaviour, because hot blocks make it more common.
- Hazards: over-copying is always correct, except over owned bytes, which must go through the compare (as above).
  A block shared by views (`aliased`) must stay guarded (a write through another view would be missed): exclude
  `aliased` blocks.
- Est. **~1 ms submit** (VirtualProtect, re-walks) plus **est. 3-6 ms on title threads** (330-460 traps, handler
  and lock waits a frame). Extra copy of about 330 × 64 KiB, about 21 MB a frame at most, mostly the same 48 MB
  that changes anyway.
- Effort: M.
- Test: a unit test where a block faults in 4 consecutive "frames", is reported written at every asking after
  that, and is never `VirtualProtect`ed again (test hook counting protects). Run with `VICTORY_HOT_BLOCKS=alt`.
  Pass: copystat guard faults fall by most of the 330, copied MB rises by no more than about 25 MB, and
  `REFRESH_CHECK` stays clean.

### E. A flat page-kind map from the runtime (`VICTORY_KIND_MAP=1`) (removing VirtualQuery for good; prior art idea 5)

- The kind is known when the memory is made. `virtual_alloc` gives placeholders views and everything else
  `MEM_WRITE_WATCH`. `XMemMapPhysicalPages`/unmap already call `title_memory_remapped`. Keep one byte per 64 KiB
  block of title space: 0 unknown, 1 watched, 2 view-mapped, 3 view hole. Writes to it happen at
  allocate/map/unmap/free time, and askings read it with no lock. With it, `known_view`, `is_view`, `uncommitted_end`
  and the failed `GetWriteWatch` are all lookups. Prior art does the same: a flat page→buffer array instead of
  range trees, with dirty runs scanned with count-trailing-zeros (`Bits::next` already does that).
- Hazards: memory allocated or mapped by a path that does not tell us (some other module's `VirtualAlloc`) is
  "unknown" and takes today's path. That is safe because unknown means "ask as today".
- Est. **3-4 ms**. It covers A completely, without A's note-expiry hazard.
- Effort: M (hooks in `xmem` free/alloc/map, about 2 MB table, tests).
- Test: an xmem unit test: allocate, map, unmap and free each kind, and check the map after each step. A debug
  switch `VICTORY_KIND_MAP=check` compares the map with `VirtualQuery` for one asking in 1,024 and logs
  mismatches. Pass: 0 mismatches over a full world run.

### F. Track by the title's own calls and allocation attributes (`VICTORY_TRACK_BY_ALLOC=1`)

- The title writes GPU memory with plain CPU stores, so no API call marks each write. The calls do tell which
  memory the CPU can write at all. `XMemVirtualAlloc`/`XMemMapPhysicalPages` take console protection bits that
  `virtual_alloc` drops (`protect & HOST_PROTECTION`), and `XMemAlloc` attributes carry a memory type. Memory the
  title maps with no CPU write access (render targets, GPU-only pools) cannot be CPU-written. It needs no guard and
  no asking, only the side channels it already has: DirectStorage, fread, `write_physical` and our own writes. If
  most of the 7.4 GB checked a frame is GPU-only, the guards lose most of their work.
- First step, a census (diagnostics): bytes checked per frame by console protection/attribute class. Without it
  the gain is unknown.
- Hazards: a title that writes memory whose attributes say it will not. Map such memory host read-only (not
  read-write) so a stray write faults. The fault handler then opens it and logs "attribute lied" once. That is the
  same cost as a guard, so this is safe by construction.
- Est. **0-8 ms**, depending on the census. Effort: S for the census, M for the switch.
- Test: the census line first. Then an `alt` run with `REFRESH_CHECK=1` and a count of "attribute lied" faults
  (must be 0, or the class drops out).

### G. Hash only on demand (`VICTORY_HASH_SMALL=<KiB>`)

- Hashing everything instead of tracking does not work. 7.4 GB a frame at about 20 GB/s is about 370 ms. It can
  work for small, hot ranges. A range of 64 KiB or less that is refreshed every frame can be compared with the copy
  (`victory_differs_guarded`, which can read title memory safely) for about 3 µs, cheaper than a trap plus handler
  plus re-protect. Best used as D's "hot" path: a hot block is compared, not copied blind.
- Hazards: none for correctness: the compare is exact. Cost grows if a large range is misclassified as small.
- Est. **0.5-1 ms** on top of D. Effort: S.
- Test: a unit test that the compare path copies exactly the differing runs. In-run: copystat MB compared vs copied.

### H. Use what the GPU-side copies already know (`VICTORY_GPU_RANGES=1`) (prior art ideas 2 and 4)

- Keep, per `Shadow`, a list of GPU-modified byte ranges. A range is added when a binding is declared writable
  (`gpu_writes`), tagged with the writer's submission fence value. It replaces on-card hashing as the source of
  "what shaders wrote". For tracking, the gains are:
  1. Our copy-back writes into title memory (`write_own`, relay merge) go into the journal as known writes
     (`wrote_own` exists). The guard then never has to find them, and with `BACK_REGUARD` none of them are
     re-protected.
  2. A CPU write over a GPU-modified range drops the range from the list. Only the bytes the CPU wrote go up,
     with no flush first (prior art idea 4).
  3. A copy whose source range is GPU-modified is done on the card (CopyBufferRegion after the writer), and the
     destination inherits the mark (prior art idea 2). The CPU never reads those bytes to refresh the destination.
     This targets the next-frame stall more than tracking.
- Hazards: a missed writable binding gives a stale CPU view. Keep hashing in `alt` phases to cross-check. Byte
  granularity is needed where CPU and GPU share a 64 KiB block (prior art saw corruption at 4 KiB page
  granularity).
- Est. **0.5-1 ms** of tracking. The larger gain is elsewhere (hashing and the stall). Effort: L.
- Test: an `alt` run where both the list and the card's hash are computed, logging any byte the hash finds that
  the list does not cover (must be 0).

### I. CPU writes straight into card memory (`VICTORY_UPLOAD_HEAP_CARD=1`) (prior art idea 5)

- For copies in the card's memory, put the card-side copy in a GPU-upload heap (resizable BAR) and memcpy the
  changed runs into it, instead of a CPU staging copy, a copy command and a fence.
- Caveats from the research: do not ask D3D12's own write watch for this heap (the heap flag forces it back into
  system memory). Our tracking stays on the title side, unchanged. The mapping is write-combined, so never read
  it: `changed_runs_with` must compare against the CPU-side record (`held`/`Owned::sent`), never the mapping, and
  writes should be whole sequential runs. Watch the VRAM budget.
- Est. **0 ms of tracking**. It saves upload and copy cost elsewhere, unmeasured. Effort: M.
- Test: check the options-16 feature cap at start (log it). A byte-exact unit test of a run written into the heap
  and read back on the card.

### J. Write-watch only, no guards (`VICTORY_PRIVATE_VIEWS=1`)

- Windows cannot write-watch section views, and the guards exist only because of views. Backing view memory with
  private write-watched memory would remove guards, faults and `VirtualQuery` altogether. But the title's physical
  page aliasing (the same page at several addresses, `XMemMapPhysicalPages`) would then have to be emulated by
  copying between aliases on every map, and DirectStorage page writes would need fan-out.
- Hazards: aliasing correctness (the main reason sections are used), memory doubling, remap cost (83 remaps a
  frame).
- Est. **8-10 ms gross** (all guard cost) minus unknown alias-copy cost; `GetWriteWatch` over 7.4 GB adds about
  1-2 ms. Effort: XL. Last resort, after B-E.
- Test: the xmem alias tests (`title_memory_view`) under the switch, then a full world run with `REFRESH_CHECK`.

### K. Coarser protection blocks (2 MiB) (rejected)

- Fewer `VirtualProtect` calls, but every write dirties 2 MiB: copies would grow by up to 32x on the ~48 MB that
  changes a frame. C gets the benefit of coarse granularity without this cost. Not recommended. Est. ≤1 ms saved,
  more lost to copying.

### Other findings

1. **Instruction emulation's VirtualQuery, 3.9 ms on recording threads** (probable owner: `console_insn::writable_to`
   under the crash handler). It has a region cache (`remembers()`). Check that it is on, and widen it from 4
   entries. Probably also part of the 14.4 ms of exception dispatch on recording threads. Count those exceptions by
   code in the next diagnostic run. Not tracking, but a cheap gain.
2. **Port 6 did not lower `guards`** (run 12: 12.4-12.7 ms against 10.7-11.4 ms without). The shared lock helps
   the faults but not the cost per asking. B and C address the asking count.
3. **`known_view` takes the guards' lock** (shared, or exclusive without port 6) for every refresh of view
   memory. E or C removes that.
4. In run 42, port 6, light refresh, `VIEW_KNOWN` and `ABSENT_ONCE` were all off. A is a free first measurement.

## 3. Ranking by ms per effort

| Rank | Option | Est. ms saved | Effort | Switch |
|---|---|---|---|---|
| 1 | A: turn on VIEW_KNOWN + ABSENT_ONCE | 2-3.5 submit | config | existing (`alt` available) |
| 2 | C: region fault epochs | 4-7 submit | S | `VICTORY_GUARD_EPOCH` |
| 3 | B: one guard pass per submission | 4-6 submit + 3-4 title threads | S-M | `VICTORY_GUARD_ONCE` |
| 4 | Other 1: instruction-emulation query cache | up to 3.9 recording threads | S | existing cache, widen |
| 5 | D (+G): hot blocks left open, compared | ~1 submit + 3-6 title threads | M | `VICTORY_HOT_BLOCKS`, `VICTORY_HASH_SMALL` |
| 6 | E: flat kind map | 3-4 submit (covers A) | M | `VICTORY_KIND_MAP` |
| 7 | F: track by allocation attributes | 0-8 (census first) | S census, M switch | `VICTORY_TRACK_BY_ALLOC` |
| 8 | H: GPU-modified range lists | 0.5-1 tracking (more elsewhere) | L | `VICTORY_GPU_RANGES` |
| 9 | I: upload heaps | 0 tracking | M | `VICTORY_UPLOAD_HEAP_CARD` |
| 10 | J: private write-watched views | 8-10 gross | XL | `VICTORY_PRIVATE_VIEWS` |
| - | K: 2 MiB blocks | negative | - | not proposed |

Combined, A + B + C + D bring submit-thread tracking from about 8 ms to an estimated 1-2 ms, and title-thread
tracking (lock waits, traps) from about 5-7 ms to an estimated 1-2 ms. That is about 10-12 ms of the 100 ms gap.
Tracking alone cannot reach 33 ms. Most of the gap is the waits shown in the profile (66 % of the submit role).

Suggested order: measure A now (no build needed once the batch finishes). Then C and B together in one build,
since both answer the journal from a cheaper source. Then D. Run the F census alongside, because it may make D and
B cheaper still.
