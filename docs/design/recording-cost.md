# Recording cost: from the title's command stream to D3D12 lists

Design only. Nothing built or run. Tree read: `D:\triumph\victory-work\wt-bisect` at fast-integ `2261b969`, `crates/victory-gpu/src` (draw.rs `State`, `tables`, `bind`, `prepare_for`, fsrhook.rs `fsr_seen`; host.rs `HostList::run_deferred`, `Batch`, `Host::card_before`, the submission body; descriptor/host.rs `HostHeap::mirror`, `dynamic`, `make`; shadow.rs `lookup`, `refresh`, `update`; indirect.rs) and `crates/victory-shader/src/emit` (lower.rs `cbuffer_dword`, host.rs root signature).

Labels: **[M]** measured (log or profile, source named), **[E]** my estimate (assumptions given), **[?]** needs a measurement first.

## 0. What the evidence is, and one correction

- Profile `victory_profile_15952` [M]: 448 s, 3,383 presents, "ms a frame" = **per present, 132.5 ms**. The run is `run14_nocb_prof.ps1` (log `victory-run-20261127-025612`), which sets **`VICTORY_NO_COPYBACK=1`**. That is the row 35/42 configuration in `docs/runs.md` (~197-202 ms/frame), **not the 138 ms control (row 34)**. Its frame line says 191.8 ms/frame; presents alternate ~2 ms and ~400 ms, so a per-present figure is about two thirds of a per-frame one.
- So the 18.6 ms `run_deferred` and 13.1 ms `Batch::flush` are per present, in the zero-copy configuration. The biggest submit-thread item in this profile, 36 ms in the driver from `card_before`, exists **only with NO_COPYBACK** (226,099 ranges a frame against ~900 without, host.rs doc of `card_scatter`).
- The exe had no PDB beside it. I aligned the newer PDB at four exact anchors: `run_deferred` = profile `+0x4d5390`, `Batch::flush` = `+0x4df6c0`, `Host::card_before` = `+0x4e24b0`, `State::bind` = `+0x5c5800`. Nearby frames are named only where the offset fits. Others are left as offsets. Next profile: put `victory.pdb` beside `victory.exe` (item 0).
- `VICTORY_CARD_SCATTER` landed at 03:12 (`f11a218f`), after this run started at 02:56. The profile does not include it.

## 1. Why the recorders wait 70 % of the time, and on what

The six "recording" threads are the title's job workers (they start at `title+0x2215ea0`). Waits per present, from the RECORDING section [M]:

| Wait | ms / present (6 threads) | Share | What it is |
|---|---:|---:|---|
| `title+0x43c4140` (`WaitForSingleObject` at the worker's top level) | 345.9 | 72 % | **The title's job scheduler with no job to run.** It is the worker's idle loop (the frame directly under the thread start). |
| `title+0x43c38a0` (inside a job) | 52.5 | 11 % | A job waiting for another job (a join in the title's job graph) |
| our futex locks (`+0xa01b90`, `+0xa01870`, `+0xa02530`) | 40.0 | 8 % | Contention inside our list calls, under `State::bind` (see 4.1) |
| `title+0x569f0` (event) | 26.7 | 6 % | The same title event wait the title's submit thread uses (a fence or "frame done" event) |
| sleeps, graphics kernel | ~12 | 3 % | title back-off sleeps; D3D12 allocations |

Total 479 ms. CPU on recorders is 188.9 ms per present, about 31 ms per thread [M].

**Why.** The recorders are starved: no work arrives. It is not that their work is blocked. The title's frame advances only as fast as our submission pipeline:
- The title's submit thread (19368) waits **65.4 ms per present on the submission lock** in `Host::submit` and another 53.7 ms on the title event [M].
- `victory-submit` (35708) is the serial wall: 60.7 % on CPU (~80 ms per present). It spends 101 ms per present in the submission body: `card_before` 47.6 (36.05 in `D3D12Core+0x20a0e0` and the driver), `run_deferred` 18.6, a final `flush` 3.9, and 16.9 in `WaitOnAddress` [M]. That 16.9 ms is most likely the settle waits ("title memory read 28.9 ms waiting for the card" per copystat frame), not a lock [E]. Between submissions it idles 26.3 ms per present waiting for the next one [M].
- The title presents through ASYNC_PRESENT and holds in "lines and settling" (252 ms in a long frame) until our line drains.

**Consequences for this design:**
1. Today, cutting per-draw CPU on the recorders buys almost nothing directly. The bound is the 26 ms per present when the submit thread idles waiting for the title. **Recording costs the frame only where it puts serial work on the submit thread**, through the `Deferred` steps a draw queues (fills, refreshes, GPU-write snapshots, card ranges).
2. At the 33 ms target it flips. Recorders do ~31 ms of CPU each per present today [M]. The five busy ones spend 58-67 ms in list calls per copystat frame (~200 ms) [M]. Once the submit thread is fast, **recording is the next wall**, so per-draw cuts must reach about 40 % for 33 ms. Section 4 is that plan. It is ranked lower only because it pays later.

## 2. What `run_deferred` and `flush` do, and how to shrink or parallelise them

`HostList::run_deferred(batch)` (host.rs ~8861) runs on the submit thread, for each list of a submission, in order. It walks the list's `Deferred::copies` steps, which the recorders queued:

| Step | Queued by | Work at submission |
|---|---|---|
| `Copy(dst, src, len)` | title buffer copies | `batch.title_wrote` (**a full `flush` of every pending refresh**), heal, `copy_mapped` into title memory, `watch::wrote` |
| `Fill(dst, src, len)` | `defer_fill`: **constant buffers** of fresh tables and misaligned root CBVs (draw.rs `tables`, `root_view`) | plain or kernel copy of title bytes into the list's upload scratch (`copy_fill`) |
| `Refresh(shadow, addr, len)` | `Shadow::refresh` (vertex/index buffers, buffer views, root SRVs) | `batch.add`: merged per copy into one span `[min, max]`. Nothing is copied yet. |
| `GpuWrite` | UAV ranges | `flush_one(shadow)`, then `shadow.writing` (snapshot) |
| `Run` / `Host` | our CPU emulation of title commands | `title_written` (full flush), then the closure |
| `Texture`, `Ordered` | textures, steps moved to the card | refresh once per submission; queue for the card |

`Batch::flush` (host.rs ~9265) runs at every `Copy`/`Run` step and at the end of the submission. It does one write-watch asking ahead over all pending spans (`watch::ask_ahead`). Then `Shadow::update` per copy (write-watch or journal, `copy_stale`), either serially or spread over helpers with `VICTORY_WIDE_REFRESH`.

Profile split per present [M]: `run_deferred` 18.6 = flushes from `Copy`/`Run` 9.3 + `flush_one`/`update` 3.4 + ~5.9 of everything else (fills, copies, snapshots, per-step `Instant::now` and atomics). The final flush is 3.9. Under `Shadow::update`, `GetWriteWatch` is ~6.8 and copies are ~2.5.

How to shrink and parallelise them (items 2, 3, 5 and 11 in section 6):
- **Narrow the mid-submission flushes** (`title_wrote`): only pending refreshes whose span overlaps the copy's destination have to be done first. The others can wait for the end flush. That turns ~80 flushes a frame (28 copies + 43 runs + 8.5 ends [M, copystat and runs.md row 10]) into few, each with one big asking.
- **Run the end flush wide.** The copies are independent (each writes its own copy and reads title memory). `VICTORY_WIDE_REFRESH` exists. It was measured worse in run 112, when copies went through the kernel one by one; they are plain now.
- **Take the constant buffers out** (port 5, section 5). The ~12,600 "title copies" a frame [M] are almost all `Fill`s (runs.md row 10: only 28 are `Copy`). Port 5 replaces them with refreshes of shared copies.
- **Fold the planning onto helpers**: `plan_beside` (port 6) already does this for the ask-once spans; the same per-list fold can precompute each list's merged refresh spans on the recorder thread at `Close`, so `run_deferred` only joins them.

## 3. Cutting the submit thread's own list recording (`card_before`, NO_COPYBACK only)

`card_before` (host.rs 5420) records the "send-and-hash" list: one `CopyBufferRegion` per changed range of every card copy, then hash or compare dispatches. With NO_COPYBACK the ranges are exact (226,099 a frame, 31.3 MB) [M], and recording them is the 36 ms in the driver [M, and runs.md row 42]. Without NO_COPYBACK it is ~900 ranges and cheap.
- `VICTORY_CARD_SCATTER` (in tree, on by default when NO_COPYBACK is armed) turns copies with at least 16 ranges of 256 bytes or less into one masked-dword scatter dispatch. **It has not been measured.**
- If the measured remainder is still over ~8 ms: record the sync list as K lists on the `wide` helpers. Partition by copy, so each list owns whole copies and its barriers. Submit them in order ahead of the title's lists. D3D12 lists are free-threaded per list; `CARD_WORK.sync` needs K pooled lists.

## 4. Cutting per-draw cost (recorders)

Recorder CPU per present [M]: `State::bind` 82 ms in total, about 13-18 µs per bind over ~4,700 draws and ~1-2 k dispatches. The heaviest entry is around `+0x5924d0` (aligned to `list::execute_indirect`) at 48.6 ms, then `+0x586900` at 34.3 ms. D3D12 and driver calls on recorders are only ~15 ms of it [M]. **The cost is our bookkeeping, not API calls.** Guard-page exceptions (`KiUserExceptionDispatcher`, 21 ms) are port 6 territory and not repeated here.

### 4.1 Lock contention inside list calls
About 30-40 ms per present of futex waits, plus spinning, under `bind`. The paths are `bind > +0x4c5f00 > lock` (16.7) and `bind > +0x5ea080 > +0x52d5b0 > lock` (13.1). `+0x535f90` (aligned near `shadow::lookup`) also reaches `+0x52d5b0` [M]. The locks are not resolved by name [?]. Candidates:
- `shadow::BUFFERS`, a single `Mutex<BTreeMap>` taken by every `find`/`find_any`. That is every vertex buffer, index buffer, root view and buffer-view refresh, with a 64-entry backwards scan under the lock.
- `backskip::COMMAND`, taken in every `defer_fill` and `defer_read_back` when NO_COPYBACK is armed.
- the view heap's `next` lock in `HostHeap::dynamic` (12 k tables a frame).

Design:
- (a) Named contention counters (diagnostics) on those locks, so the next profile names them.
- (b) Make `BUFFERS` an `RwLock`; lookups take it shared, `entry.shadow` creation upgrades.
- (c) Add a thread-local last-hit cache `(start, end, Arc<Shadow>, generation)` in front of `lookup`. A global generation bumps on any `BUFFERS` change. Consecutive draws hit the same buffers.

### 4.2 Descriptor tables
`tables()` makes a **fresh table in dynamic slots for every table that holds a CBV** (12,023 a frame [M]). For each one it:
- hashes all `n` descriptor records with SipHash;
- takes `n` dynamic slots;
- runs `Create*View` for every slot up to the highest used entry ("hundreds per draw" in world tables, per the comment);
- queues one `Fill` per CBV.

The per-list `fresh_tables` cache cannot help across lists or across draws whose CB addresses differ, which is every draw with per-draw constants. Copystat counts ~3,000 CBV records written a frame (10.8 M over the run [M]) against 12,000 tables made.
- **The fix is port 5** (section 5). With it, CBVs no longer force a fresh table.
- Without port 5: hash the words with FxHash or xxh3 instead of SipHash. Copy the non-CB entries with one `CopyDescriptorsSimple` from a CPU-only mirror of the heap, instead of `n` `Create*View` calls (shader-visible heaps cannot be a copy source, so the mirror has to be kept). Est. 1-2 ms per recorder thread [E].

### 4.3 Root constants and redundant state
`bind` rebuilds `values: Vec<u32>` (a heap allocation) per stage per draw and re-issues root constants, root views, `IASetVertexBuffers`/`IASetIndexBuffer` and `ensure_state` for targets. Three switches exist, off by default: `VICTORY_DEDUPE_ROOT_ARGS`, `VICTORY_SKIP_VERTEX_REBIND`, `VICTORY_SKIP_TARGET_TRANSITIONS`. **runs.md rows 12-13: they stop the scene loading.** Reading the code, there are two causes:
- `State::replay` (after a list cut onto a new host list) resets `host_pipeline`, `host_roots` and `host_heaps`, but **not** `host_vertex_views`, `host_index_view`, `host_targets` or `host_depth*`. The new host list then draws with no vertex or index buffers bound and without its render-target `ensure_state`, and that state feeds the list's prelude transitions.
- `skip_target_transitions` compares only "same targets as the last draw". A texture moved out of the render-target state in between (read as SRV via `readable`, copied, cleared, `twin_use`) is not moved back.

Fix:
- `replay` clears every `host_*` cache.
- The target skip keys on a per-list **transition epoch** (a counter bumped by every `list.transition`), stored at the last ensure.
- `values` becomes a `[u32; 64]` stack array.

Worth ≤1 ms per recorder thread [E]: the API calls are cheap on this driver. Do it for hygiene after 4.1, 4.4 and port 5.

### 4.4 Bind elision for unchanged state
Each `set_*` (`set_table`, `set_view`, `set_constants`, `set_pipeline`, `set_heaps`, targets, vertex and index buffers) sets a per-stage dirty bit. If a draw or `execute_indirect` finds nothing dirty since the last successful `bind` on the **same host list**, it skips `bind` entirely, including `tables`, `early_textures` and the refresh registrations. Three conditions must also hold:
- (i) the list's `refreshed` set has not been cleared since (a title copy clears it; give `Deferred` a counter);
- (ii) the descriptor heap's write generation is unchanged (bumped by `sync_heaps`/`mirror` when a title slot changes);
- (iii) no indirect clamp or gather pass has rebound our compute state (`host_roots[1] = 0` already marks that).

GPU-driven titles walk args buffers with long runs of the same pipeline: windows serve 1,113 of 1,708 passes (indirect-overhead.md). Est. 2-5 ms per recorder thread [E]. The share of draws with unchanged state is [?]; add a census counter first.

### 4.5 ExecuteIndirect batching
Built on 4.4. Consecutive title `ExecuteIndirect`s qualify when they have:
- the same command signature and stride;
- contiguous argument ranges;
- no count buffer (none in this scene [M]);
- no barrier and no dirty state between them.

Those can become one host `ExecuteIndirect(MaxCommandCount = n)` over the window's clamped scratch. The window already clamps several records in one pass. The CPU saving is the per-call `prepare_for`, window bookkeeping and refresh; the GPU-side clamp story stays in indirect-overhead.md. Constraint: the merge needs lookahead, so it is a deferred host call, closed at the next non-mergeable command or the list's `Close`. Est. 0.5-2 ms per recorder thread on top of 4.4 [E]. High effort.

### 4.6 Bundles: rejected
Our per-draw cost is bookkeeping, not D3D12 calls (~15 ms of ~190 on recorders [M]). Bundles cannot change descriptor heaps, would still need every refresh and table, and on this driver they replay a CPU-side record. The title's lists differ per frame (constant addresses). Lists the title runs again unchanged already reuse our host list. Revisit only if a census shows identical per-frame sequences.

### 4.7 `fsr_seen` per draw (only with `VICTORY_DLSS=1` or the FSR trace)
`fsrhook::active()` makes every draw and dispatch build a `Dispatch`:
- a joined hash string;
- `format!` labels per resource;
- descriptor lookups;
- `words_at` reads of every CB (dispatches);
- the global `RECENT` mutex when depth or motion vectors are read.

Fix: precompute "candidate" bits per `HostPipeline` at creation (is in hashes or skip; reads depth or motion; output-shaped), so `fsr_seen` runs only for candidates. Est. 0 now, ~1-3 ms per recorder thread with DLSS on [E].

## 5. Port 5 in our tree: constant buffers read in place

What the fork's renderer did (described, not named): translated shaders read CBs as raw bytes, so the title's CB memory is bound in place and ~22,000 copies a frame went to zero (−4.7 ms there). In our tree, a CBV in a table forces a fresh table plus a `Fill` per CBV per draw (4.2). Root CBVs are filled only when the host address is not 256-aligned (`root_view`).

**Equivalence argument.** `Fill`s copy title bytes at our submission. A `Refresh` of the shared host copy also copies at our submission. Both rely on the same rule: the title rewrites a CB only after its fence for the earlier use, and our fences signal only after the flight lands ("fence signals set behind flights"). Hazards on shared copies are already handled for vertex and index data (`pipefence::note_read`, `stale_under_flights`). Constant data becomes one more such range.

### Stage A: aligned CBVs from the title's own slots (no shader change)
- In `tables()`, a CBV entry stops forcing `fresh` when `descriptor::host::make()` would make a real CBV for it. That means `shadow.gpu(address) % 256 == 0`; `make()` already does exactly this and returns `None` otherwise. Tables with a misaligned CBV, half views or past-heap entries stay fresh as today.
- The entry goes through `table_entry → HostHeap::mirror(index, handle, true)`. A CBV slot is made once per title write of that record, not once per draw.
- `readable()` must refresh `[address, address + size)` of the shadow for `View::Constant`. Check that `buffer_range` covers `Constant`; if not, add it, as `fsr_seen` and draw.rs:2646 already decode it.
- In the fresh-table path, a `Constant` entry whose shadow is aligned gets a CBV on the shadow (`constant_at(index, shadow.gpu(address), size)` plus `shadow.refresh`) instead of scratch plus `defer_fill`.
- Root CBVs: unchanged (aligned ones already bind the shadow).
- Counters: CBV entries by alignment (aligned / misaligned), fills a frame, fresh tables a frame, CB refresh bytes. **First measurement [?]: the aligned share.** The device reports itself as D3D12.X, whose CBVs are normally 256-aligned, so it may be most of them.
- Switch: `VICTORY_CBV_IN_PLACE=1` (runtime only, so `alt` works).

### Stage B: raw loads for what stays misaligned
- victory-shader, `emit/lower.rs`: `cbuffer_dword` and its three call sites (scalar dword loads ~6306, row loads ~6362, per-lane indexed ~11189-11208). With `HostShader::constants_raw`, request the handle as a raw SRV (`handle(site, src, false, (11, …))` in place of kind 13). Load with `rawBufferLoad` at byte address `a`: one masked 4-dword load per 16-byte row.
- Root CBVs: `root_cbv_handle` becomes a raw root SRV in a new `ROOT_CBV_RAW_SPACE`, so it cannot clash with `t<slot>` in `ROOT_CBV_SPACE`. `emit/host.rs` (~160) and `pipeline/host.rs:420` declare it as `D3D12_ROOT_PARAMETER_TYPE_SRV`.
- descriptor/host.rs `make()`: `View::Constant` with raw mode → `CreateShaderResourceView` with `R32_TYPELESS` and the RAW flag, `FirstElement = shadow.offset(address) / 4` and `NumElements = ceil(size / 4)`. That is bounds-checked, so reads past the end give zeros, as on the console. The null-view branch (~3160) becomes a null raw SRV.
- draw.rs `bind`: in raw mode a root CBV binds `SetGraphicsRootShaderResourceView(shadow.gpu(address))` after `shadow.refresh`. It keeps the scratch fill when `address % 4 != 0` or `shadow.left(address) < 64 KiB`, because root descriptors have no bounds and must not read past the copy.
- `shader_cache.rs` key and the pipeline library key gain a `constants_raw` bit. Mixed shaders would read an SRV as a CBV through the same heap index, so the switch is global and process-wide.
- Switch: `VICTORY_CBV_IN_PLACE=2` (no `alt`: it changes translated shaders).

### Stage C: share the remaining fresh tables across lists
Once no fresh table points into list-private scratch, key fresh tables globally by `(table, n, content hash)`. Refcount the slots per list, and give them back when the last list using them has run (`tables_done`). That cuts the 12 k tables down to distinct contents. Switch: `VICTORY_TABLES_SHARED=1`.

### Estimates, hazards, tests
- **Estimate.** Submit thread: −3 to −5 ms per present, from fills (~12.6 k a frame) and their per-step overhead out of the ~5.9 ms non-flush remainder of `run_deferred`, plus a smaller step list [E]. Recorders: −2 to −4 ms per thread, from 12 k fresh tables down to only the misaligned, half and past-heap tables [E]. **Frame now: 3-6 ms; at the target it is also a large share of the recorder cut needed.** If the CB memory lies in placed (in-place) copies (`INPLACE=2`, `INPLACE_MORE=1` are in the launch set), refresh is free and the gain is at the top of the range. Whether it does is [?]. The research note's 5-12 ms assumed in-place memory.
- **Hazards.**
  - (1) A shared slot rewritten while an earlier flight still reads it. The title's own fence discipline covers this; mirror rewrites a slot only after the title rewrote its record.
  - (2) CB ring spans: `Batch::add` unions to `[min, max]` per copy. A wrapping ring gives one wide span, which costs one write-watch walk over it, not a copy of it.
  - (3) Stage B GPU cost: raw loads skip the constant cache. Est. +0.5-2 ms of card time [E]. The card is not the wall today (idle 7.4 of 17 ms in a usual frame [M]), but it gets close at the target. Mitigation: stage B only for misaligned CBs. Aligned ones stay real CBVs (stage A).
  - (4) The first stage B run rebuilds every pipeline. Warm the cache in a soak run before measuring.
  - (5) Dynamic indexing past a CB's size: zeros (raw view), as on the console. Root path: guarded by the 64 KiB-left rule.
- **Tests.**
  - Unit, stage A: `make()` of an aligned `View::Constant` gives a CBV on the shadow, and a misaligned one still goes fresh (WARP, as host.rs's adapter test).
  - Unit, stage B: translate a CB-reading shader both ways (`emit/diff_tests.rs`) and compare the output buffers on WARP for aligned, 16-aligned and dynamically indexed reads.
  - In-run, stage A: `VICTORY_CBV_IN_PLACE=alt` with picture checks (`picture-checks.md`). Pass criteria: copystat fills a frame fall by the aligned share, "tables asked" falls, `run_deferred` falls, and dropped draws are unchanged.
  - In-run, stage B: two runs with a warm cache.

## 6. Ranked items (ms per effort)

Effort: S = a run or under a day, M = 2-4 days, L = a week or more. "Frame now" is what the critical path (submit thread) gains per present in the profiled configuration. "Recorder" is per recording thread, which pays once the submit thread is under ~35 ms.

| # | Item | Frame now [E] | Recorder [E] | Effort | Hazard | Switch | Test |
|--:|---|---:|---:|:-:|---|---|---|
| 0 | Profile with matching PDB, in the **control** config (row 34), and name the locks of 4.1(a) | 0 (unlocks the rest) | — | S | none | `VICTORY_PROFILE=1`, `victory.pdb` beside exe | profile names `run_deferred`/`bind` children |
| 1 | Measure card scatter (NO_COPYBACK only) | **20-30** of the 36 ms driver time (226 k ranges; share ≤256 B is [?]) | — | S (exists) | masked bytes over card writes: existing compare lists | `VICTORY_CARD_SCATTER` (=0 off) | two runs; "scattered as … dwords" line; card compare mismatches 0; picture check |
| 2 | Wide end-of-submission flush, measured again with plain copies | 4-8 (13.1 flush + 3.4 `flush_one`) | — | S (exists) | journal and guard lock contention with recorders (port 6/7) | `VICTORY_WIDE_REFRESH=alt` | in-run alt; "preparing" and "copies" in frame line |
| 3 | Narrow mid-submission flushes to overlapping spans (`Batch::title_wrote`); give `Run` steps a known range where they have one | 2-4 (part of the 9.3 flush in `run_deferred`) | — | S | a refresh overlapping the destination must still go first; `current` trimming as today | `VICTORY_FLUSH_NARROW=alt` (new) | unit: `Batch` order test (copy over a pending span flushes it, others stay); `fill_check` counters unchanged; picture check |
| 4 | Port 5 stage A: aligned CBVs from title slots, CB data via refresh | 3-5 | 2-4 | M | section 5 (1), (2) | `VICTORY_CBV_IN_PLACE=1`/`alt` (new) | section 5 tests; first the alignment counter |
| 5 | Recorder locks: `BUFFERS` RwLock plus thread-local last hit; fix whatever item 0 names | ~0 (≤26 ms of submit-thread starvation bound) | 3-5 | S-M | generation bump on every `BUFFERS` change, or a stale Arc is used | `VICTORY_SHADOW_LOOKUP_CACHE=1` (new) | unit: lookup after register/retire returns the new copy; lock-wait counters drop |
| 6 | Parallel `card_before` recording (K lists) if item 1 leaves >8 ms | 4-8 of the remainder | — | M | list order and per-copy barriers must stay in one list | `VICTORY_CARD_LISTS=4` (new) | in-run A/B; compare mismatches 0 |
| 7 | Bind elision on unchanged state (4.4) | ~0 | 2-5 | M | missed descriptor rewrite between draws; covered by the heap write generation | `VICTORY_BIND_ELIDE=alt` (new) | census counter first; in-run alt plus picture check; `VICTORY_INDIRECT_WINDOW_CHECK=1` |
| 8 | Port 5 stage B: raw CB loads for the misaligned rest | 0.5-2 beyond A | 0.5-2 | L | section 5 (3), (4), (5) | `VICTORY_CBV_IN_PLACE=2` (new) | WARP diff tests; warm-cache runs |
| 9 | Port 5 stage C: fresh tables shared across lists | ~0 | 1-2 | M | slot lifetime across lists | `VICTORY_TABLES_SHARED=1` (new) | unit: slots given back only after the last list; "tables asked" |
| 10 | Table cost without port 5: FxHash, `CopyDescriptorsSimple` from a CPU mirror | ~0 | 1-2 | M | CPU mirror must track every slot write | `VICTORY_TABLE_COPY=1` (new) | unit on the mirror; picture check |
| 11 | Fills copied at record time with a journal check at submission (fallback if port 5 stalls) | 2-3 | −0.5 | M | title writes CBs after recording the draw; re-copy pages the journal marks | `VICTORY_FILL_EARLY=alt` (new) | `fill_check` "read other bytes" stays 0 |
| 12 | Fix and re-test the dedupe switches (4.3) | ~0 | ≤1 | S | rows 12-13 broke loading; the epoch and `replay` fixes address it | existing three `VICTORY_*` | soak to the world, then alt |
| 13 | ExecuteIndirect merging (4.5) | ~0 | 0.5-2 | L | lookahead across calls; barriers | `VICTORY_EI_MERGE=1` (new) | warp tests with garbage tails; window check |
| 14 | `fsr_seen` pre-filter (only with DLSS on) | 0 | 1-3 with DLSS | S | candidate bits must match `listed`/`shape` | `VICTORY_FSR_PREFILTER=1` (new) | FSR trace still finds the traced draws |
| — | Bundles | — | — | — | rejected (4.6) | — | — |

**Order of work:** 0, 1, 2, 3 (a day of runs plus a small change), then 4 and 5, then 6 if item 1 falls short, then 7. Items 8-14 only if the recorders become the wall.

**What this buys, and what it does not.** In the profiled configuration, items 1-6 take the submit body from ~101 ms per present to about 55-65 [E]. That turns NO_COPYBACK from a loss (row 35) into a likely win. In the 138 ms control, items 2-5 give about 8-15 ms [E]. Neither reaches 33 ms on its own. The rest of `preparing` is write-watch, refresh and copy-back work (ports 6, 7 and 2 of the fork-ports research note; `pipelining.md`). After that, the recorders' ~31 ms per thread needs items 4, 5 and 7 to leave headroom under 33 ms.
