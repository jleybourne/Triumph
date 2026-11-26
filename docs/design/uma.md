# UMA: title graphics memory the card uses directly

Design only; nothing here is built or measured. Tree read: `wt-bisect` (`b28414fc`, branch
`pipeline-samplers`). Reference read (ideas only, GPL-2.0-only, no code taken): xcl
`crates/xcl/src/gpu/uma.rs`, `crates/xcl/src/hle/d3d12x/sparse.rs`.

## 1. What the numbers say, and what they imply

| cost (diagnostics build, world) | ms | what it is |
|---|---|---|
| copies into host copies | 23.5 (72 MB) | refresh of title bytes into shadows (`Shadow::update`, `copy_stale`) |
| copy-back | 42.8 (56 MB) | shader writes merged into title memory (`copy_back_with`, `relay_merge`, `card_dirty`) |
| card: send-and-hash + compare | 19.8 + 6.4 | lists of ours that exist only to find what shaders changed |
| title waits in present | 67.5 | for the submit thread above |

Two facts decide the design:

1. **Copy-back serves almost nobody.** README, run 277 (`VICTORY_BACK_READS=alt`, 49 min): of
   438,000 stores sampled, 60 % touched by nothing before the next store, 36 % only by our own
   refreshes, 3.8 % written by the title, **0.19 % touched by anything else** (the most the title's
   code can have read). Known real readers: the ocean's height/displacement textures (`texback.rs`),
   query/readback resources, command-processor writes the title spins on.
2. **System memory is the wrong home for big GPU-read data.** Placing big buffers in opened heaps
   measured 146 ms against 131. The i7-10700KF is PCIe 3.0 x16 (~12 GB/s), so every 100 MB the card
   reads from the PC per frame is ~8 ms of card time. Opened heaps (`inplace.rs` header) are always
   custom heaps in the PC's memory (pool L0), so "in place" always means "over the bus".

Hence xcl's split, adapted: **coherent system-memory tier** for what the CPU writes every frame or
reads, **card tier** for what the GPU reads heavily or writes, and **no copy-back** except for a
short, known list. xcl does exactly this: its sparse VRAM windows upload CPU-changed 64-byte chunks
and copy back only command-processor writes and explicit `Map` reads; its committed coherent window
holds draw arguments and readback buffers.

## 2. Allocation (crates/victory-hle `modules/xmem.rs`)

The title's addresses are fixed by the title (it runs natively; console GPU VA == CPU VA), so the
CPU side of title memory stays ordinary Windows memory at the title's addresses. What changes is how
it is made so the GPU side can open it:

- **Page pool** (`XMemAllocatePhysicalPages`): always `SEC_COMMIT` when UMA is on (today
  `VICTORY_XMEM_COMMIT=1`; 6.1 GiB pagefile commit, fine with 64 GB). Each section is opened ONCE with
  `OpenExistingHeapFromFileMapping` (no pages locked, 0.2 ms / 256 MiB, title's views stay free to
  map/unmap/protect). Page numbers are handed out contiguously per call (already: `next_page += n`),
  so a title that maps an array in order gets VA runs that are section runs (port.rs shows the 32-bit
  heap does exactly that).
- **Private memory** (`virtual_alloc`, `MEM_WRITE_WATCH` kept): opened per allocation with
  `OpenExistingHeapFromAddress1` only when it is placed in the system tier; that pins the pages
  (`VirtualFree`/`VirtualProtect` fail with 87), so xmem must call
  `victory_gpu::title_memory_freeing` before forwarding any title `VirtualFree`/`VirtualProtect`/
  decommit on graphics memory (flags `GRAPHICS`), and the GPU side waits the last fence that used it.
  Prefer port.rs's trick where possible: map a second view of our own and open that, never the
  title's addresses.
- **Address-range rules unchanged** (`address_range`: 32-bit memory below 4 GiB, the rest 4 GiB..1 TiB
  for the BVH builder).
- New hook: `xmem::map` / `unmap` already notify `title_memory_view` / `views_gone`; these become the
  source of truth for the **region table** below (no Windows queries on the hot path; `private_whole`
  costs ~1 ms in big blocks).

Not proposed: handing the title a `Map()` pointer of a GPU upload (ReBAR) heap as its allocation. The
VA is driver-chosen (may break the 4 GiB..1 TiB rule, cannot back the 32-bit heap or page-pool views),
cannot be write-watched, and any title CPU read of it is an uncached read over PCIe. Kept as a late,
probe-gated option (step 7) for buffers the census shows the title only writes.

## 3. Tiers (which memory goes where)

Decided per title buffer / 4 MiB window when the shadow would be made (`shadow::find` creation path),
from a per-buffer census (step 0) and kept for its life:

| tier | home | gets | coherence |
|---|---|---|---|
| **S** system, in place | opened heap (section or private), buffer placed with `ALLOW_CROSS_ADAPTER` | CPU-written each frame (constant rings, dynamic VB/IB, the 32-bit heap of port.rs), anything the CPU reads (readback/query targets, CP-write targets, `Map`-for-read buffers), small buffers (< `CARD_BUFFERS_MIN_KB`) | none needed: same bytes; visibility by the title's own fences, which we signal only after the host list completes |
| **C** card | DEFAULT heap, or GPU upload heap (`host.gpu_upload_buffer`) | big GPU-read-mostly (meshes, streamed geometry, BVH nodes), GPU-written GPU-read (skinning out, particles, culling, indirect args the CPU never reads) | CPU→card: changed 64 B chunks of title-written pages only. card→CPU: **nothing**, except the allowlist (section 5) |

Budget rule for S: bytes the card reads from S per frame (census: bound ranges × uses) ≤ ~150 MB
(~12 ms of PCIe 3.0 card time, spread across the frame). A buffer over its share moves to C. This is
the rule that the 146-vs-131 run did not have.

Card room: 12 GB with 2.2-3.1 GB of render targets plus textures; C keeps the existing
`CARD_BUFFERS_MB` cap and falls back to S, then to a copy.

## 4. Addressing

**Region table** (new, victory-gpu `uma.rs`, our own design): `BTreeMap<title_va, Region>` where a
Region is `{start, len, heap, buffer (≤ 4 GiB - 64 KiB), gpu_base, tier, section/offset}`. A page-pool
section larger than 4 GiB - 64 KiB (exactly 4 GiB is refused, xcl measured) gets overlapping placed
buffers every 2 GiB so any binding ≤ 2 GiB fits one. Filled from map/unmap notifications.

- **Root descriptors, VB/IB views, indirect args** (`draw.rs` `root_view`, the VB/IB/indirect paths in
  `host.rs`): `title_va → Region → gpu_base + (va - start)`; no `refresh`, no `scratch` fill unless the
  constant buffer is not 256-aligned (that case keeps today's upload fill, ~small). Range not one run
  (`inplace::cover == Scattered|Holes`) → today's shadow copy, counted (`uma: fallback` line).
- **Buffer views in tables** (`descriptor/host.rs` `buffer_copy`, `make`, `own_bytes`): the SRV/UAV is
  made on the Region's buffer with `FirstElement = (va - start)/stride`; raw views need byte offsets
  < 4 GiB (DXIL raw offsets are 32-bit), which the 4 GiB buffer cap guarantees.
- **Raw shader addresses** (`resource::MEMORY`: `global_*`/`s_load` on computed pointers; BVH builder
  and walkers): keep the existing 64 KiB page directory (`emit/shader.rs` `SLOT_MEMORY`,
  `MEMORY_PAGES = 2^21`, lowered in `emit/lower.rs` `fn memory`). Unlike xcl's window-index-plus-offset
  arithmetic, a per-page table survives page-pool aliasing (scattered physical pages), and it is
  already in the translator. Change only who fills it: `memory.rs` writes directory entries from the
  region table at map time (copy number = the Region's raw view slot, start = page's byte in that
  buffer) instead of on faults, and stops "bring every numbered copy up to date and declare all
  written" per dispatch (`memory::ready`). No shader change; the fault path stays as a safety net.
  Optional later: a 4 GiB-window fast path (xcl-style `(hi - base_hi, lo)`) for private-memory
  regions only, if the directory load shows up in the card profile.

## 5. GPU→CPU visibility without copy-back

- **S tier**: coherent by construction (write-back pages, snooped). A CPU read after the title's
  fence sees GPU writes. Requirement: our signal of a title fence follows completion of every host
  list that wrote S memory (true with waits on; **incompatible with `VICTORY_LATE_WAIT`** — the
  inplace.rs note already says so for level 2).
- **C tier**: GPU writes stay on the card. Copied back only for:
  1. `texback.rs` kinds (ocean) — unchanged;
  2. command-processor writes (`Write32/64`-style, fills, buffer-to-buffer copies) whose destination
     is title memory the CPU may poll: route their destination to S instead (cheapest: CP writes are
     tiny);
  3. resources the title `Map`s for read: on `Map` with a read range, copy that range back once
     (xcl's `readback_now` idea);
  4. an allowlist `VICTORY_UMA_BACK_KEEP=<start-len,...>` and buffers the census flagged as
     touched-by-other.
- **Stale CPU view hazard.** With no copy-back, title memory under C buffers is older than the card.
  When the title later writes part of such a page, today's refresh would send the whole 64 KiB page and
  overwrite GPU-written bytes. Rule (xcl does the same): per C page keep 64-byte chunk hashes of what
  was last *sent*; a refresh sends only chunks whose title bytes differ from that hash. Memory cost:
  8 B per 64 B (1/8 of C bytes). Reuse `shadow.rs` `changed_chunks` / `changed_runs`; the hashes
  replace the `before`/`kept` snapshots.
- **Never** detect title reads with `PAGE_NOACCESS`: our handler takes only write faults; a read
  fault reaches the title and kills it (the `safecopy` lesson). Read detection stays the
  working-set trim of `backreads.rs`, diagnostics only.

## 6. Textures versus buffers

Opened heaps take only row-major single-level textures, and the title's textures are in the
console's tiled layout. So **textures stay host copies on the card** (`texture.rs` detile on change,
`tiles.rs` reserved textures mapped onto *our own* heaps). UMA does not touch them, except:
- texture memory the title also reads as a buffer stays in its Region (S or C) as buffer bytes; the
  host texture is still made from it by detiling;
- render targets never copy back (only `texback` kinds, as today);
- **no `UpdateTileMappings` may name a Region heap**: tagged heap kinds plus an assert in every
  `tiles.rs` mapping call (line ~557), since a tile mapping into an opened heap removes the device.
  The title's own `CopyPageMappings` keeps mapping into our heaps, never into Region heaps.

## 7. What happens to shadow.rs and watch.rs

- `shadow.rs` (5,019 lines): with UMA on, a `Shadow` is a thin Region view (the existing in-place form,
  `Shadow::placed`) for S, and a card buffer + chunk hashes for C. Unused under UMA and left only for
  the fallback (scattered bindings, UMA off): `copy_back_with`, `gpu_writes`' pending/flights/
  `before`, `relay_merge`, `card_dirty`/`take_dirty`, `card_hashed`, the hash/compare piece machinery
  (`CardSpan`, `find_pieces`, `back_piece`), `put_back_own`. Removed once UMA is the default and the
  fallback count is ~0.
- `watch.rs` (2,913 lines): shrinks to one job, "which pages of C-tier memory did the title write
  since they were sent": write watch on private memory, read-only guards (write faults only) on
  page-pool pages backing C buffers. S memory needs no watching at all, so the guards and their
  resets drop to the C pages (far fewer: 3.8 % of stores were title writes). `LIGHT_REFRESH` and the
  journal stay for C.

## 8. Landing plan (each step behind a switch, measurable alone)

Every step: diagnostics build, `run_viewplace.ps1` A/B, 600 s, report `fps · ms · draws · dropped`,
screenshot (water, boats, skinned characters), one switch per run, no compiler running.

| step | switch | change (files / functions) | expected |
|---|---|---|---|
| 0 | `VICTORY_UMA_CENSUS=1` | per shadow per frame: bytes refreshed, copied back, card-read estimate, `inplace::cover` kind, backreads touch; one log line + CSV (`shadow.rs` `placed_line`, `backreads.rs`, `inplace.rs` `census_of`) | 0 ms; decides tiers and the allowlist. Gate: fallback (Scattered) share of bound bytes |
| 1 | `VICTORY_UMA_BACK=1` | no copy-back except section 5's list; chunk-hash refresh for C pages (`shadow.rs` `copy_back_with`, `card_dirty`, `update_free`, `copy_stale`; `host.rs` copy-back landing) | copy-back 42.8 → ~2 ms; compare list −6.4 ms card; also removes the "36 % only our own refreshes" re-reads. **−35 to −40 ms submit thread.** Biggest, simplest, riskiest to correctness |
| 2 | `VICTORY_UMA_HASH=0` (with 1) | stop send-and-hash lists for C buffers (`shadow.rs` `card_make_before`, `card_hashed`; the list recording in `host.rs`) | −19.8 ms card time (card busy 71 → ~45 ms) |
| 3 | `VICTORY_UMA_POOL=1` | open each committed section once (`xmem.rs` `XMemAllocatePhysicalPages` → `inplace::title_memory_section` → `inplace::open(From::Section)`); region table in new `victory-gpu/src/uma.rs` fed by `title_memory_view`/`views_gone`; `inplace::place_for` answers from it for S-tier buffers (fixes blocker 3: `stretch()` needs one view; a section heap does not) | copies 23.5 → ~8 ms (S share of 72 MB is CPU-written data); +a few ms card time over PCIe — watch card busy |
| 4 | `VICTORY_UMA_BIND=1` | root views / VB / IB / indirect / table buffer views resolve through the region table, no `refresh` call (`draw.rs` `root_view`, `descriptor/host.rs` `buffer_copy`/`make`) | −3 to −6 ms submit thread (lookups and refresh bookkeeping per bind; 20k calls a frame) |
| 5 | `VICTORY_UMA_DIRECTORY=1` | `memory.rs` fills the directory from regions at map time; `memory::ready` no longer refreshes/declares all numbered copies | BVH dispatch copies and their compare go; size from step 0 (expect 1-5 ms) |
| 6 | `VICTORY_UMA_UPLOAD=1` | C-tier changed chunks written straight into a GPU upload heap buffer (`host.gpu_upload_buffer`, as `window_upload_for` does) instead of copy ops; CPU never reads it | remaining C refresh ~5 ms → ~2 ms; fewer copy operations (5.5 µs each) |
| 7 | `VICTORY_UMA_DIRECT=1` (probe first) | title allocations the census marks write-only served from a mapped GPU upload heap in xmem | only if the Map VA lands in 4 GiB..1 TiB; otherwise dropped |

Expected end state: submit thread ~74 → ~15-20 ms at work, so present wait falls to what the card
needs: card ~71 − 26 (our lists) + PCIe reads of S (~3-8) ≈ 50 ms. Frame ≈ card-bound **~50-55 ms
(18-20 fps)**, not 30: the title's own ~45 ms of card work is then the wall (CLAUDE.md, "What 30 fps
actually requires").

## 9. Risks

- **Device removal from tile mappings into opened heaps** (`inplace.rs` header). Assert on heap kind
  in every `UpdateTileMappings`; never translate a title tile mapping into a Region heap.
- **Pinned pages**: a heap opened from an address makes the title's `VirtualFree`/`VirtualProtect`/
  unmap fail with 87. Use section heaps and our own views; xmem must call `title_memory_freeing` and
  wait fences before forwarding frees. Opening from an address also locks the whole range into RAM
  (256 MiB: 70 ms) — never open large private blocks on the hot path.
- **PCIe 3.0 bandwidth**: S tier misplacement repeats 146 vs 131. The census budget (≤ ~150 MB/frame
  card reads from S) and the card-busy figure in each run guard it.
- **Silent wrong data** from skipped copy-back (0.19 % max readers, but e.g. boats sank without
  texback). Screenshots per run, census-built allowlist, the per-range keep switch.
- **Stale CPU view overwriting GPU writes** on partial title writes: the chunk-hash rule; unit-test it
  as `the_merge_stores_only_what_the_card_changed` does.
- **Timing semantics change**: in place, the card reads at execution, not at our submit snapshot. Any
  title race the copies masked becomes visible. Not with `VICTORY_LATE_WAIT`.
- **Cross-adapter buffer limits**: buffers in opened heaps need `ALLOW_CROSS_ADAPTER`; acceleration
  structures, UAV counters and some indirect forms may be refused there — those stay C.
- **4 GiB caps**: heap/buffer ≤ 4 GiB − 64 KiB; raw view offsets 32-bit; overlapping placed buffers
  over big sections.
- **Scattered page-pool bindings** (blocker 3) fall back to copies; step 0 says how many.
- **GPU upload heap reads**: uncached over PCIe; nothing of ours may hash, compare or `memcpy` from it.
- **Card memory**: 12 GB; C tier competes with render targets and textures (`tiles.rs` already had the
  card full at 4.3-5.5 GB of texture copies).
