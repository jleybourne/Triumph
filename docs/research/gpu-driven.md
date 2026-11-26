# Removing CPU work from the console-GPU-to-D3D12 layer

Scope: desk research only. No builds, no game runs. Web search was thin on emulator-specific evidence (see "Evidence quality"), so several numbers below are engineering estimates from public driver and API behaviour, marked as such. Names and URLs are in `private\gpu-driven-sources.md`.

## 0. Reality check before anything else

- 135 ms / 40,700 calls = about 3.3 microseconds per call. A plain D3D12 list call (draw, set root table, set PSO) costs roughly 0.05 to 0.5 microseconds on a modern NVIDIA driver, so bare recording of 40,700 calls should be about 5 to 20 ms. The other ~115 ms is almost certainly work done around the calls: packet decode, state hashing, PSO lookup, descriptor creation and copying, barrier tracking, memory dirty checks, texture detiling, uploads. This is an estimate, not a measurement.
- Consequence: ideas that only shrink the call count (indirect, bundles, Vulkan) attack the smaller slice. Ideas that remove per-draw translation work or spread it over cores attack the larger slice. A per-function CPU profile (ETW or a sampling profiler on one captured frame, no game run needed if a capture replay exists) should precede all effort. Wins below assume a split of roughly 90 ms decode/record/descriptors, 30 ms memory sync, 15 ms other. Wins overlap and do not add.
- The 8 cores / 16 threads of the CPU are the largest unused resource. If the translator is single-threaded, parallelism beats every exotic option.

## 1. GPU-driven command processing

Mechanism. Upload the raw console command stream to the GPU. A compute pass walks the packets (headers give length, so a parallel scan is possible), resolves register state per draw, and writes ExecuteIndirect argument buffers, or work-graph input records, or Vulkan generated-command sequences. The CPU records only a handful of indirect calls per pass.

State resolution on the GPU is tractable: state at draw N is the last write to each register before N, which is a segmented "last writer" scan. Indirect-buffer chaining, predication and conditional execution make the stream non-linear and need a CPU pre-pass or iterative GPU passes.

Hard limits (properties of the APIs):
- D3D12 ExecuteIndirect can change root constants, root CBV/SRV/UAV addresses, vertex/index buffer views and the draw/dispatch arguments. It cannot change the pipeline state, render targets, descriptor heaps or issue barriers. Console state is mostly register writes that become a different PSO (blend, depth, raster, shader pair, vertex layout), so each distinct combination needs a distinct PSO and a CPU-side switch.
- Work graphs with mesh nodes (tier 1.1) can select among shader programs on the GPU, the first PC route to GPU-chosen graphics programs. But fixed-function state is baked per node, graph depth is limited (32 reported), and early write-ups said work graphs cannot transition resource states mid-execution. A vendor demo reported ExecuteIndirect about 1.64x slower than mesh nodes on one pre-release AMD driver. Independent numbers on GeForce are scarce; support on this card needs checking (Agility SDK version, tier query) before any plan depends on it.
- Vulkan device-generated commands (EXT) is ratified and lists emulation of D3D12 work graphs among its motivations. It adds pipeline or shader-object switching from the GPU, which ExecuteIndirect lacks, but needs a Vulkan backend (idea 3) and has per-sequence layout restrictions and device limits on stream size and token types.
- Descriptors: indirect work cannot create descriptors. Everything referenced must already be in one heap, which pushes toward idea 2.

Prior art. I found no public emulator or translation layer that parses a console command processor on the GPU. The nearest evidence is the opposite direction: a layer that emulates D3D12 ExecuteIndirect on Vulkan by preprocessing arguments in compute and, newer, using device-generated commands, with driver-specific performance cliffs and state-restriction workarounds. Existing emulators keep command-processor emulation on the CPU. Treat full GPU-side parsing as research, not an engineering task with known outcome.

Pragmatic variant (recommended instead of full GPU parse): a fast CPU pre-pass (SIMD packet scan, no per-draw API work) splits the stream into runs of draws that share PSO, render targets and root signature. Each run becomes one ExecuteIndirect whose per-draw arguments (vertex/index addresses, counts, constant pointers) are written into an argument buffer by the CPU or a small compute pass. State changes stay CPU-side but happen once per run rather than once per draw.

- Effort: full GPU parse, very high (months, uncertain). Run-batching hybrid, medium (2 to 4 weeks), depends on idea 2.
- Risks: PSO explosion if runs fragment; ExecuteIndirect cost on this GPU; debuggability; predication/conditional rendering semantics.
- Licensing: public API specs; no code needed from any GPL-2.0-only project.
- Expected win: run-batching hybrid 20 to 40 ms (conditional on average run length; if most draws change state, near zero). Full GPU parse: theoretical 40 to 60 ms, low probability of delivery.

## 2. Bindless everything

Mechanism. One large shader-visible descriptor heap and shader model 6.6 direct heap indexing. Console texture/buffer/sampler descriptors are small fixed-size structures in guest memory. Two variants:
- A. Translate the console's resource descriptors into D3D12 descriptors lazily, deduplicated by content hash, into a persistent heap slot table. Shaders receive slot indices through root constants or a per-draw record. The per-draw descriptor-table copy disappears; only first-use creation remains.
- B. Pull model: shaders fetch buffers (vertex, constant, structured) from a GPU-visible mirror of guest memory by address using raw loads, so no buffer descriptors are needed. Textures still need real descriptors (hardware sampling), so variant A applies to them.

Evidence/numbers. Descriptor copies cost roughly 50 to 200 ns per descriptor, worse if the source is a shader-visible (write-combined) heap; creating descriptors costs a few hundred ns each. For tens of thousands of draws with several resources each that is plausibly 10 to 30 ms (estimate). Bindless is the standard answer in public rendering talks and every modern GPU-driven pipeline depends on it. Pull-model vertex fetch lets many draws share one pipeline and vertex layout, the precondition for idea 1 batching and idea 4 replay.

- Effort: medium-high (3 to 6 weeks): shader translator changes, heap slot allocator with eviction, sampler handling (2048 sampler heap limit needs a dedupe table), root signature redesign.
- Risks: 1M descriptor heap limit and slot lifetime/eviction; slower shaders from divergent dynamic indexing (small on this GPU); descriptor validity when texture memory is dirty.
- Licensing: clean; standard design.
- Expected win: 10 to 25 ms directly; it is the enabler for ideas 1 (hybrid), 4 and draw merging, which together are worth more.

## 3. Vulkan backend, host-pointer import, PCIe trade-offs

Per-call overhead. Vulkan recording has no runtime layer, and driver cost per call is comparable to or somewhat lower than D3D12 on NVIDIA. Expect at most 20 to 40% off the bare recording slice (5 to 20 ms), so 2 to 8 ms, not the 100 ms gap. Vulkan features that matter beyond call cost: descriptor buffers, shader objects (no PSO compile stutter), graphics pipeline libraries, device-generated commands with pipeline switching. A second backend is a large ongoing cost.

Host memory import.
- Vulkan: the external-memory-host extension imports an application allocation into a Vulkan memory object. Alignment is device-reported (typically page size; verify on this GPU). The memory is pinned system RAM accessed by the GPU over PCIe.
- D3D12: OpenExistingHeapFromAddress (and the file-mapping variant, which lets one guest-memory section be mapped both for the emulator's guest range and for the GPU) gives the same capability, so a Vulkan switch is not required. Placed resources on such heaps are essentially buffers; textures should still be copied to VRAM.
- Related D3D12 option: the GPU upload heap type (Agility SDK, needs resizable BAR) gives CPU-writable VRAM, removing upload-then-copy for CPU-produced data. Needs BIOS resizable BAR on the Z490-class board (a BIOS update on many boards; verify).

Bandwidth and latency on PCIe 4.0 x16 (4070 Ti: x16 link, 504 GB/s VRAM):
- Link peak 31.5 GB/s per direction; achievable roughly 24 to 27 GB/s for large DMA. At 33 ms frames that is about 0.8 GB per frame per direction, shared with everything else.
- GPU reads of host memory in place: bandwidth capped at the link (about 5% of VRAM bandwidth) and latency on the order of 1 to 2 microseconds versus a few hundred ns for VRAM. Fine for data read once per frame (vertex streams, constants, small structured buffers, indirect-argument inputs). Poor for textures, render targets, or anything read many times; cache those in VRAM.
- CPU writes to a mapped VRAM upload heap are write-combined: fast sequential (about 10+ GB/s), very slow if read back or written randomly.
- The big win is not bandwidth but removing the CPU copy and synchronisation: today every sync presumably costs a memcpy plus a copy-queue submit plus barriers.

Recommended memory-sync design (works on D3D12 today):
1. Guest memory allocated as a section; mapped as the guest range and opened as a D3D12 heap; one placed buffer covers the range for pull-model buffer reads (no copy).
2. Dirty tracking with write watch (page granular) or a GPU-side hash diff, so only modified pages of texture and render-surface regions are copied into VRAM mirrors.
3. Texture detiling/swizzling in a compute shader reading raw guest pages (no CPU de-swizzle).
4. Lazy GPU-to-CPU readback only when the guest CPU actually touches the range.
- Effort: medium (2 to 4 weeks) for 1 to 2; detile compute shaders 1 to 3 weeks per surface family.
- Risks: page-granular false sharing; pinned-memory limits; heap-from-address alignment and size restrictions; coherency on write-combined mappings; ranges the guest rewrites every frame.
- Licensing: public APIs. A permissively licensed emulator and a GPL-2.0-or-later emulator use similar tracked-range schemes; compatible with GPL-3.0 (verify file headers). GPL-2.0-only projects: ideas only.
- Expected win: memory sync 15 to 30 ms (of an assumed 30). Vulkan port itself: 0 to 8 ms on the call slice; not worth it unless device-generated commands become essential.

## 4. Caching and replaying command lists and bundles

Mechanism. Hash the structure of a pass's command packets with data payload excluded (draw counts, state registers, binding structure). If the structure matches a previous frame, replay the recorded D3D12 list. Replay is valid only if the list embeds no per-frame values: it must read addresses and constants indirectly (idea 2 variant B, constants and descriptor slot indices in a per-frame record buffer indexed at execute time, ExecuteIndirect for draw parameters).

Facts:
- D3D12 bundles cannot set render targets or issue barriers, and inherit descriptor heaps, render targets and optionally PSO from the caller. Public vendor guidance says bundles save little on modern drivers; the gain is skipping API-call overhead (the small slice). A closed command list can be executed again as long as its allocator stays alive until the GPU is finished.
- The structural hash costs memory bandwidth: 5 to 10+ GB/s per core with a fast hash, so a few MB of commands costs well under a millisecond.
- Hit rates are unknown and title-dependent: UI, post-processing, shadow setup and fixed effect chains tend to be steady; scene geometry varies with culling. Whole-frame hits are unlikely; per-pass hits are plausible.

Cheap experiment first: compute structural hashes of consecutive frames of a captured sequence and measure per-pass match rate. No game run needed if captures exist.
- Effort: medium (2 to 3 weeks) once idea 2B exists; much harder without it.
- Risks: stale-state bugs (hard to detect), memory held by lists, GPU lifetime tracking, resources whose addresses change (render-target pools) breaking replay.
- Licensing: clean.
- Expected win: 0 to 5 ms without bindless; 10 to 40 ms with bindless and a good hit rate on heavy passes.

## 5. Other ideas (several outrank the above)

A. Parallelise and pipeline the translator (highest confidence). Split decode from record, and split a frame into independent segments (by render-target change or explicit fence points), recording each into its own command list and allocator on a worker thread, then submit once in order. Overlap frame N+1 decode with frame N recording and GPU execution so the budget is the slowest stage, not the sum. Needs deterministic state handoff at segment boundaries (a state snapshot per segment from the pre-pass). On 8 cores expect 3 to 5x on the parallel part. Effort 2 to 4 weeks. Risk: shared caches (PSO, descriptor, texture) need sharded or lock-free structures; ordering of guest memory sync. Expected win 40 to 80 ms. Licensing: clean.

B. Draw merging and state elision: collapse consecutive draws sharing state and contiguous index ranges; detect instancing patterns; drop redundant SetPipelineState, root-signature and descriptor-table calls via a known-state cache. This lowers the 40,700 call count directly. Effort 1 to 2 weeks. Win 5 to 20 ms.

C. PSO and shader handling off the hot path: precomputed state-hash lookup, on-disk pipeline libraries, asynchronous PSO creation with a fallback, per-title pre-warm cache. Mostly removes stalls and hitches. Effort 1 to 2 weeks.

D. Descriptor staging hygiene: build descriptors in non-shader-visible heaps, copy in large batched CopyDescriptors calls, never read from shader-visible heaps. Cheap if present code copies per draw. Win 2 to 10 ms.

E. Copy queue and async compute: uploads, detiling and resolves off the direct queue with fences so the CPU does not wait. Win depends on stalls; 5 to 15 ms of wait time is plausible.

F. Allocation and COM hygiene: arena allocators for per-frame state, no per-call heap allocation, no debug layer or GPU-based validation, recycled allocators and lists, Enhanced Barriers batched and coalesced (barrier tracking is a frequent hidden hot spot). Win 5 to 15 ms; low effort.

G. Per-stage CPU and GPU timing behind a switch (ETW markers) so each idea's real win is measured, not guessed.

H. Decode-time tricks: SIMD packet scanning, prefetching, table-driven register-write handlers instead of branchy switches. Win 3 to 10 ms.

## Ranking by expected win (ms off 135, overlapping, estimates)

1. Parallel and pipelined translator (5A): 40 to 80.
2. Bindless + pull model with run-batched ExecuteIndirect hybrid (2 + 1-hybrid): 30 to 60 combined (direct 10 to 25).
3. Memory-sync rework: host heaps, write watch, GPU detile (3): 15 to 30.
4. Per-pass command-list replay (4), requires 2: 10 to 40 if hit rate is good, else 0 to 5.
5. Draw merging, state elision, barrier and descriptor hygiene (5B, 5D, 5F): 10 to 30 combined.
6. Async queues (5E): 5 to 15.
7. Vulkan backend (3): 0 to 8 on call overhead; only for device-generated commands.
8. Full GPU-side console command parse (1): speculative, high risk.

Realistic path to 33 ms: parallelise and pipeline first (cheap, large), then bindless/pull enabling batching and replay, then fix memory sync. The unconventional GPU-driven routes are needed only if profiling shows the call slice itself dominates after those steps.

## Licensing summary (GPL-3.0 project)
- Public API specs and vendor guidance: free to implement.
- Permissive (MIT, BSD, zlib, Apache-2.0) code: reusable with attribution.
- LGPL-2.1-or-later and GPL-2.0-or-later code: compatible with GPL-3.0 (verify the exact SPDX identifier in each file).
- GPL-2.0-only code: ideas only; no copying, avoid close paraphrase.

## Evidence quality
- Strong: API behaviour (ExecuteIndirect limits, bundle limits, PCIe link rates, extension existence and alignment rules).
- Medium: per-call and descriptor costs (public driver behaviour and talks, not measured here).
- Weak or absent: any emulator doing GPU-side command-processor emulation; replay hit rates; work-graph performance on GeForce. Searches returned specs and vendor posts but no emulator case study, so those claims are estimates.
