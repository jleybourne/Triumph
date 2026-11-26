# Native paths, round 2: what could run natively instead of being translated or synced

Research only; nothing built or run. Builds on `research-native-paths.md` (read first; its verified on-disk facts are not
repeated). Projects are described, not named; names and URLs are in `private/native-sources.md`.

Grades: **[M]** our measurement quoted from our own docs (not re-measured), **[P]** number published by a project,
**[Q]** qualitative claim by a project, **[R]** I read the design, no number, **[K]** my background knowledge, not
re-checked online this session, **[E]** my estimate with reasoning. Web access this session was thin (a handful of searches);
anything graded [K] should be confirmed before it drives a decision.

## Bottom line

The frame is 135 ms because of **memory synchronisation** (submit thread ~74-96 ms of it: copies into host copies, copy-back,
hash/compare lists), not because of command translation or shader quality. Every "make it native" idea therefore scores by one
question: does it remove a sync step, or only a translation step? Translation steps (HLE of draw calls, recompilation
tricks) are already cheap on our critical path (list recording 0 ms on the render thread, [M]). Only items that stop us
discovering, copying or hashing bytes move the wall clock. Ranked list at the end; the top three are all variants of
"stop treating all title memory as possibly GPU-written and possibly CPU-read".

## (1) High-level emulation of the renderer

### 1a. Hooking engine-level render functions in the title's code
- **Mechanism:** patch the title's own draw-submission functions and replace them with PC D3D12 code.
- **Prior art:** every mature GPU-heavy emulator found works at the **API or command-processor boundary**, never inside the
  game's own engine. The one place HLE of the *engine* exists is for old, small, fixed-function titles, or for ports of
  titles whose source was available (decomp ports). For a closed, modern, multi-million-instruction engine I found no
  example that hooked engine-level render code. [K]
- **Evidence for gain:** none to quote. Our own numbers say the ceiling is small: recording is off the render thread [M], and
  a thinner translation measured 75 -> 68 ms in a sibling project [earlier file]. **[E]** 0-7 ms.
- **Effort/risk:** months, high; per-build signatures; no symbols. **Not recommended.** Unchanged from round 1.
- **Legal:** per-build offsets and byte signatures are allowed by our policy; no title code in the repo.

### 1b. API-boundary HLE (what we and one sibling already do)
The title talks to the console's D3D12-like API in `d3d12_xs.dll`. We already implement that surface. The question is
whether we implement it *shallowly* (record, then translate raw command buffers) or *deeply* (use the API's semantic
information). The reported wins from deep API-level HLE in other projects came from three places:
1. **Resource metadata.** The API tells you at creation time which resources are render targets, UAV-capable, depth, or
   copy-only. Projects that keep this metadata skip tracking for read-only resources entirely. [R] A console emulator that
   works at the GPU command level has to rediscover this by watching writes, which is why its buffer cache is a large part
   of its CPU cost. One Switch emulator rewrote its buffer cache around page bitmaps and reported up to 84 % frame-rate gain
   on one GPU/API and 55 % on another [P, GPL, ideas only] (already in `research-coherency-prior-art.md`).
2. **Descriptor-level binding knowledge.** A writable view bound in a submission is a precise "GPU may have written this
   range" signal, replacing content hashing. [R] (same prior-art file, idea 1).
3. **Barriers and fences with intent.** API-level transitions and fences say when a GPU write becomes visible to the CPU, so
   copy-back can be scheduled on a helper thread against a specific fence instead of a global stall.
- **Applies to us:** this is the "hybrid" of HLE: stay at the API boundary but harvest resource flags, heap creation
  parameters, and writable-view bindings to build a **per-range GPU-writable set**, then drop hashing and copy-back for
  everything outside that set. **[E]** Largely subsumes item (4C of round 1). Gain up to ~26 ms card (hash lists) and a
  large share of the 42.8 ms copy-back, because only ranges that could be GPU-written are ever examined.
- **Caveat (important):** the console allows shaders to write through raw 40-bit addresses with no resource object
  (round-1 finding: translated shaders use raw addresses and title-written descriptors). So metadata alone is a
  *hint*, not a proof. Mitigation: keep a verification mode (hash everything every N frames, log any change in a range the
  metadata called read-only) and run it on the same long sample that produced the 0.19 % figure. Over-approximation is
  safe if the action on a hit is a GPU-side copy rather than a CPU readback.
- **Effort:** 1-3 weeks for descriptor/flag harvesting if the API layer already sees creates and descriptor writes (it does, being
  our code). Risk medium; failure mode is a silently stale range, so the verification mode is part of the cost.
- **Legal:** all our own code, written from public D3D12 semantics.

### 1c. A frame-level "retained" renderer (what some projects did for old consoles)
Old-console projects with a dedicated high-level renderer (reproduce the final image from scene state rather than from the
command stream) report large gains but only because the GPU there is a fixed-function pipeline with a few hundred state
combinations. Not transferable to programmable, compute-driven, indirect-heavy rendering (2,088 indirect commands/frame, [M]). Skip.

## (2) Offline asset conversion at install time

### Mechanism
At first launch (or install), decode the user's own archives once: BCPack/zlib unpack, detile console swizzle, write plain
BCn in a PC layout (and optionally GDeflate-compressed) to a derived cache outside the repo. At runtime the title still asks
for its tiled data, so the cache is **only useful where we control what the title receives**.

### The central problem for us
The title's code runs natively and reads/writes console-layout memory itself. A conversion cache works in projects where the
emulated game asks the *emulator* for a texture by handle, because the emulator then picks the format. Here the title's loader
streams tiled bytes into mapped pages and our code detiles them for the GPU. Pre-converting means we must **intercept the
streaming service** (the DirectStorage-equivalent we already implement) and deliver converted data to the GPU path while
giving the title either nothing, a small stub, or the original bytes. Anything the title CPU-reads (ocean displacement, physics
sampling) needs the original. So it is a *routing* problem, not a conversion problem.

### Evidence from comparable projects
- Emulators that cache converted/decoded textures or shaders on disk report the win in **load/stutter**, not steady-state:
  shader cache pre-build is the canonical example and we already have it (100 % coverage). [Q]
- Texture packs / replaced-texture workflows (pre-decoded BC) report removing per-load decode cost, again in streaming
  hitches. [Q]
- Console-to-PC ports that re-cooked assets at build time (any native port) avoid detile entirely because the data never
  exists in tiled form on PC. That is the full version of this idea; it needs the engine's cooperation, which we lack.
- **Our numbers [M]:** `textures` 6 ms steady, but hundreds of ms/frame while streaming. So a conversion path helps the
  *streaming* regime much more than the steady 135 ms frame.

### Feasibility and impact
- **GPU detile compute (round-1 3A)** reaches most of the benefit without any install step and without the routing problem:
  upload tiled bytes, detile on the card. This is the practical form. **[E]** 4-6 ms steady, large during streaming.
  Effort 1 week; the XOR-mode textures still excluded in `texture.rs` (`addressable()` false for XOR modes) could be covered
  by the same table-driven shader, which also removes the "texture not detiled" class of red blocks. Risk low-medium.
- **Full install-time conversion with interception:** weeks-months; **[E]** adds only load-time gain over GPU detile. A hard
  coupling: the title may checksum or size-assume resources. Not worth it for the frame target.
- **Install-time work that is clearly worth it:** (a) shader DXIL cache (done); (b) pre-built PSO library / driver shader
  cache warm so no compile hitches (check whether driver-side compile of our 26,084 programs is warmed; hitches here look
  like frame-time spikes, not the 135 ms floor); (c) precomputed hash of static, never-written data so the first-sight
  hash cost disappears (a one-time 1-2 GB scan at install, then "static" flag by range). (c) is new and directly attacks
  the hash lists: ranges never written by title or GPU in the install-time map and not in the writable set can skip
  hashing forever, with the write tracker as the safety net. **[E]** a few ms; low effort, low risk.
- **Legal:** derived caches are the user's data, stored outside the repo, never distributed. Conversion code is ours. The
  console texture format knowledge comes from public GPU architecture documents and our own `victory-formats`.

## (3) Recompilation and decompilation ports: what made them fast

### What I found
- **Static recompilers for older consoles** (MIPS and similar): translate instructions ahead of time to C/C++, compile with
  a normal optimising compiler. They are fast because the CPU emulation overhead (interpreter or JIT, memory-access
  checks) disappears and the compiler can inline. The *GPU* side in those projects is a separate, usually modern, renderer
  that reimplements the old GPU at high level, and that is where the frame-rate headroom comes from (uncapped frames,
  interpolation). No published emulator-vs-recompiled benchmark found. [Q]
- **Community decomp ports** (original source reconstructed): the large win is that the engine is rewritten to talk to a modern
  graphics API directly with PC-native resources and no memory model emulation. Their performance is simply "a PC game". [Q]
- **Console GPU emulators that went from translation to native**: the big historical gains came from (a) host-memory
  mapping instead of copy (see 4), (b) not emulating small-scale tiling/EDRAM-like memory but using host render targets and
  resolving only at the points the game reads them. One 360 emulator documents that its render-target-copy approach has a
  "very heavy performance impact" compared with the alternative and describes a 2021 cache rewrite as a large speed-up [Q, no figure].

### What transfers to an x86 title with GPU as the bottleneck
- **CPU recompilation: nothing.** The title's CPU code is already native x86-64. There is no emulation overhead to remove.
  The one lesson is *the title's code is the bottleneck-free part; don't touch it*.
- **Their renderer lesson transfers:** stop emulating memory and use host resources, resolve to guest memory only where the
  guest reads. That is exactly 1b and 4.
- **Decomp lesson does not transfer:** no source, and decomp of a modern engine is years of work.
- **Binary patching of hot spots in the title** (a lighter relative of recompilation): patch known title functions that do
  pure CPU work our layer depends on, e.g. a title-side copy or memcpy-like helper that touches mapped GPU pages. If the title
  writes GPU-visible memory through a small number of functions, hooking those yields a *precise dirty list for free*
  (replaces page-fault write watching and hashing for that traffic). **[E]** speculative; needs title RE; per-build
  signatures. Worth a one-day look: count how many distinct code sites write the 143k DirectStorage destinations and the
  per-frame constant/vertex uploads. If under ~20 sites, hooking them is cheap. Effort days (investigation) to weeks.
- **Legal:** recompilation tools by definition process the user's binary; ours would patch in memory, not distribute. GPL
  considerations: some recompilation projects are MIT (code reusable), others unspecified; use as ideas only until each
  license is read.

## (4) Replacing unified memory with native resources

### 4a. Host-pointer import (the "zero-copy" idea)
- **D3D12 OpenExistingHeapFromAddress:** the documentation describes it as a diagnostic facility, and states applications
  should not use it for performance or compatibility reasons. [P: vendor docs] The heap is system memory (pool L0) and the card
  reads it across PCIe. Our own measurement agrees: opened heaps 146 ms vs 131 ms [M]; PCIe 3.0 x16 means ~8 ms of card time per 100 MB
  read per frame.
- **Vulkan VK_EXT_external_memory_host:** the same idea on Vulkan; same PCIe cost for device-read data. Hosted-memory import is
  documented as useful for upload staging and for CPU-visible readback targets, not for hot GPU data. [K] A Vulkan backend
  would also need us to re-do shader output (DXIL to SPIR-V), so it buys nothing here.
- **Conclusion:** for *GPU-hot* data it is the wrong tool on a PCIe-3 desktop. It *is* the right tool for the two small classes
  where the CPU genuinely touches the bytes: coherent small buffers (draw arguments, readback, queries, constants). That is
  where our current design already puts the "coherent system-memory tier".

### 4b. Placed resources in card memory, authoritative on card
- **Mechanism:** one big default heap (or sparse reserved resource) mirroring the title's address space in card memory;
  placed buffers at fixed offsets; writes by the CPU come through the write tracker as small uploads; shader writes stay on
  the card; copy-back only for the allow-list. This is "VRAM-authoritative" in round 1.
- **Evidence:** our own: copy-back 42.8 ms + copies 23.5 ms + hash 19.8 + compare 6.4 [M]; 0.19 % of stores read by anything
  else (provenance: our run-277 sample, unverified by me). A sibling project built this (windows with chunked uploads and
  copy-back only for command-processor writes and `Map` reads) [Q, GPL-2.0-only, ideas only]; it did not publish a frame-time delta I could verify.
- **Impact [E]:** potentially the whole of the sync cost, 60-90 ms of the 135 ms. This is the only single item that could land
  the 33 ms goal alone, together with the fixes already queued for the GPU-side clamp and barrier waits. It does not by itself
  reach 33 ms (the card side was ~26 ms of GPU work plus translation overhead).
- **Effort/risk:** weeks, high. Main risks: (i) a missed CPU reader (silent wrong answer), mitigated by verification mode and
  by the page-protection fallback (read of a card-authoritative page faults -> flush that range -> continue); (ii) the title
  maps pages one by one, so contiguous placement isn't free (round-1 blocker 3); a sparse reserved resource with tile mappings
  avoids needing contiguity. 
- **The page-fault fallback is what makes it safe and cheap.** Mark card-authoritative ranges no-access on the CPU side. A
  title read faults, the handler does a targeted copy-back of just those pages, unprotects, and counts it. Rate of faults in
  steady state is the direct measurement of how wrong the allow-list is. Other projects use this "lazy flush on CPU touch"
  design for exactly this reason [R]. Cost per fault is tens of microseconds plus the readback, so the design is only good if
  faults are rare; the 0.19 % figure says they should be.

### 4c. Resizable BAR / GPU upload heaps
- **Mechanism:** D3D12 GPU upload heaps (agility SDK, OS and driver support required) put CPU-writable memory in VRAM
  through the BAR, so CPU writes go to the card directly without a staging copy and the card reads at full VRAM bandwidth.
  [K] Support depends on driver, ReBAR being enabled, and a current Agility SDK; I could not confirm numbers online.
- **Hardware fit:** an RTX 4070 Ti on an i7-10700KF (Comet Lake, PCIe 3.0) can run ReBAR if the BIOS exposes it; the CPU-write
  path through the BAR is write-combined and slow for reads (tens of MB/s on CPU reads, never read it back from CPU). Our
  notes already mention a window-upload mode (`VICTORY_WINDOW_UPLOAD`).
- **Impact [E]:** removes one copy for CPU-written streams (constants, vertex data, streamed textures). Maybe 5-15 ms of
  submit-thread copy time, because it saves the staging memcpy and the second GPU copy. Requires the write tracker to hand
  pages to a BAR window rather than a staging ring. Effort 1-2 weeks; risk medium (CPU reads from WC memory stall badly, so
  the allow-list/ fault logic from 4b must route any CPU-read data away from it).
- **Check first:** enable `D3D12_FEATURE_DATA_D3D12_OPTIONS16.GPUUploadHeapSupported` query; it is a few lines of code and
  tells you whether the platform supports it today.

### 4d. Persistent mappings / avoiding page-protection costs
- Write tracking via page protection costs a fault per first write per page per frame. Projects that moved to
  **write-watch APIs** (Windows `GetWriteWatch` with `MEM_WRITE_WATCH`, or soft-dirty style bitmaps) got large wins because
  there are no faults and a single query returns the dirty page set. [K] A sibling emulator's tracking rewrite is part of the
  84 % / 55 % figure above [P]. Check which mechanism `watch::writes` uses; if it faults, `GetWriteWatch` on committed
  memory the title allocates itself is a drop-in for the region types where we control the allocation flags (the page pool
  and `XMemAllocatePhysicalPages` path, which we implement). **[E]** several ms; effort days; risk low. It fits the title because
  its memory comes from our implementation of the allocator.

## (5) Other things in this vein

### 5a. Fence-tied lazy copy-back on a helper thread
Where copy-back is unavoidable (allow-list), wait only on the fence of the submission that wrote the range, on a helper
thread, batched. Published emulator designs do this [R, MIT-licensed one in coherency file]. Removes the global stall, not the bytes. **[E]**
10-30 ms of the submit thread if copy-back is not eliminated outright. Effort 1-2 weeks.

### 5b. GPU-to-GPU copy for GPU-modified sources
When the title copies from a range the GPU modified, do the copy on the card and propagate the modified flag instead of
flushing to CPU first [R]. This directly attacks the "next-frame copies read shader-written bytes" stall noted in the coherency
file. **[E]** 5-20 ms. Needs the writable-range set (1b).

### 5c. Pipeline the sync work off the critical path
Even without removing it, double-buffer: let the submit thread of frame N+1 begin while frame N's copy-back finishes (one frame
of latency). Published designs for presentation pipelining exist [R]. Gives 1.3-1.8x on a sync-bound loop **[E]** if the
dependencies allow; our `design/pipelining.md` covers it. Complements everything above.

### 5d. Dedicated PC fast paths for things the title does in compute that PC has hardware for
- **Hardware BVH traversal:** round 1 dismissed it correctly (opaque formats, title shaders use raw nodes).
- **Hardware video decode (Bink etc.):** nil, already native CPU.
- **Mesh shaders:** already native. **Indirect execution:** native `ExecuteIndirect` replaces our clamp passes (2,088 indirect,
  960 clamp dispatches, [M]); **[E]** 6-12 ms of card time (from our docs). This is a card-side win, so it only matters after
  CPU-side sync is fixed.
- **Upscaler seam at the FFX loader:** unchanged from round 1; needs a binary we do not hold.

### 5e. Reducing what is mirrored: lifetimes
Much per-frame mirrored memory is transient (constant uploads, ring buffers the title reuses every frame). Treat ring-buffer
regions (detect by the pattern of full overwrite every frame) as upload-only: no hash, no copy-back, copy just the written
extent at submit. **[E]** several ms, low effort; falls out of 1b's metadata.

### 5f. Measure first
Before any of this, add three counters per frame (we may already have them): bytes uploaded, bytes copied back, count and
size of ranges examined by hash/compare but unchanged. The ratio "examined but unchanged" is the upper bound on what
metadata-based writable sets can save. No build needed to read from existing logs.

## Evidence summary table

| Claim | Number | Basis |
|---|---|---|
| Sync dominates the frame | submit thread 74-96 ms of 135 | [M] |
| Copy-back serves almost nobody | 0.19 % of stores | [M], run sample |
| Opened system-memory heap is slower on this PC | 146 vs 131 ms | [M] |
| Vendor says host-address heaps are diagnostic only | n/a | [P docs] |
| Rewriting a buffer cache around page bitmaps | +84 % / +55 % fps | [P, other project, other bottleneck] |
| Host render targets vs EDRAM-copy path | "heavy" vs fast | [Q, no figure] |
| Recompilation speedups | none published | none found |

## Ranking by expected win on our CPU-bound frame

| # | Item | Expected removal | Effort | Risk |
|---|---|---|---|---|
| 1 | Card-authoritative placed memory with fault-driven allow-list (4b), fed by API-level writable sets (1b) | 40-90 ms | weeks | high |
| 2 | GPU-writable range set from resource flags + writable bindings, drop hashing (1b) | 20-45 ms (card + part of copy-back) | 1-3 wk | medium |
| 3 | Fence-tied lazy copy-back and GPU-to-GPU copies (5a, 5b) | 10-30 ms | 1-2 wk | medium |
| 4 | Pipeline sync one frame ahead (5c) | up to 1.3-1.8x on loop | 1-2 wk | medium |
| 5 | Write-watch API instead of faults (4d), ring-buffer treatment (5e) | 3-10 ms | days | low |
| 6 | GPU upload heap / ReBAR for CPU-written streams (4c) | 5-15 ms | 1-2 wk | medium |
| 7 | GPU detile compute (2) | 4-6 ms steady, large in streaming | 1 wk | low-med |
| 8 | Native ExecuteIndirect, drop clamp passes | 6-12 ms card | 1 wk | low-med |
| 9 | Install-time static-range map to skip hashing (2c) | 2-5 ms | days | low |
| 10 | Title-side write-site hooks (3) | speculative | days to investigate | med |
| - | Engine-level HLE hooks (1a), full install-time conversion with interception (2), host-pointer import for hot data (4a), CPU recompilation (3) | ~0 or negative | - | - |

Realistic expectation: items 1-5 together can plausibly reach the 40-60 ms range; reaching 33 ms also needs the card-side
fixes (indirect, barriers, GPU time) because the card portion is ~30-40 ms on its own [M, partial]. No single native path
reaches 33 ms by itself.

## Legal and licensing notes
- Everything above is implementable from public API semantics and our own measurements; nothing needs title code or assets in the
  repo. Per-build signatures, if used, stay in a per-build table outside distributed code.
- GPL-2.0-only projects (a sibling console-title translation layer and a Switch emulator rewrite mentioned) are **ideas only**.
  MIT/BSD sources (a C# Switch emulator, N64-style recompilers where MIT) can be reused with attribution. Verify each license
  before reuse; I did not re-verify them this session.
- Derived caches (converted textures, static-range maps) are the user's data, stored outside the repo, never shared. Round 1's
  ownership flag on the configured game folder still stands.
