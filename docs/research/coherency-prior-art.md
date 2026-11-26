# Prior art: keeping CPU-written and GPU-written unified memory coherent without stalling

Scope: web search plus reading of public source mirrors, PR/issue threads and vendor specs. No builds, no runs.
Projects are described, not named; names and URLs are in the private source list.
Evidence grading: [N] = numbers published by the source; [Q] = qualitative claim by the source; [R] = read in code by me, no numbers.
Licenses: MIT / BSD / zlib code can be reused with attribution. GPL-2.0-only code is ideas only (it cannot be merged into GPL-3.0). Where I could not verify a license I say so; treat as "ideas only" until checked.

## Our problem in one paragraph

Per frame we write-watch CPU writes, upload them, hash on-card pieces to find shader-written changes, copy those back, and stall ~56 ms because next-frame copies read shader-written bytes. The stall is a data dependency we are resolving through the CPU. Every mature design below avoids that by (a) never reading GPU-written bytes on the CPU unless the CPU actually touches them, (b) doing GPU-to-GPU copies when the source is GPU-modified, and (c) when a readback is unavoidable, waiting only on the fence of the specific submission that wrote the range, on a helper thread, batched.

## 1. Per-range "GPU-modified" lists instead of hashing

Mechanism: a Switch emulator (C#, MIT) keeps, per host buffer, a lazily created list of byte ranges written by the GPU. A range is added when a draw/compute binding is declared writable ("signal modified"), not by comparing contents. Each entry carries the GPU sync number of the submission that wrote it. A CPU touch of the guest pages (tracked by protection handles) triggers a flush of only the overlapping ranges; a write to a range removes it from the list (see idea 4). Buffers over 4 KB get granular per-region handles, small ones a single handle.
Evidence: [R] design; no published speedup for this part alone. A sibling Switch emulator (GPL, ideas only) rewrote its buffer cache around page bitmaps and reported up to 84% (RTX 3070, OpenGL) and 55% (RX 550, Vulkan) frame-rate gains [N], mostly from upload/tracking cost, not readback.
License: MIT for the C# one (reusable with attribution); the sibling is GPL-2.0-or-later as far as I know (unverified), ideas only.
Maps to us: replace on-card hashing with "this range was bound writable in a submission with sync number N". Over-approximation (the whole bound range) is acceptable if it only triggers a GPU-side copy, not a CPU readback (idea 2). Hashing then becomes unnecessary, or runs only to shrink over-approximated ranges asynchronously.

## 2. GPU-to-GPU copy when the source is GPU-modified

Mechanism: in the same MIT Switch emulator's buffer-to-buffer copy path, if the source range is not GPU-modified the destination is written straight from guest memory; if it is modified, the copy is done device-side and the destination inherits the modified flag (and pending sync number) rather than flushing the source to guest memory first. Dependent "virtual" buffers are linked so modifications propagate lazily and are pulled back on demand.
Evidence: [R] code comments describe it as an optimisation to avoid a flush from the GPU. No numbers.
License: MIT.
Maps to us: this directly targets the 56 ms stall. If next-frame copies read shader-written bytes, make the copy a CopyBufferRegion on the GPU queue ordered after the writing work (one UAV/copy barrier), and propagate "GPU-dirty" to the destination range. The CPU copy of those bytes is only produced if the CPU later reads them (idea 3). This removes the CPU from the dependency chain.

## 3. Lazy readback on CPU touch, with a background flush thread

Mechanism: GPU-modified ranges stay write-protected/guarded on the CPU side. On a fault, a handler runs on a background context, waits on the fence for the writing submission only (compared by sync number), then copies overlapping ranges to staging and into guest memory without re-triggering tracking. Several emulators also keep a "readbacks" setting with off / relaxed / precise modes because most titles never read GPU output on the CPU.
Evidence: a PS4 emulator (GPL, ideas only) published measurements for its synchronous readback path (RTX 5090): about 68 microseconds per call, ~106 calls per frame in one heavy title = 7.23 ms/frame of CPU stall; 460-810 page-protection changes per frame; frame-time p50/p99 of 33.33/50 ms with or without readbacks in that title [N]. A tester on an AMD card saw ~30 fps drop to 17-20 with precise readbacks, partly because the GPU power governor read bursty sync-waits as idle and held low clocks [Q]. Its maintainers propose a "batched" mode: coalesce all reads in a fault window into one fence wait, flushed at the next sync packet, with an early flush if the guest reads a pending range [Q, proposal only].
License: ideas only (GPL-2.0 family).
Maps to us: (a) default readback OFF and allowlist ranges that have demonstrably faulted; (b) a fault on a GPU-dirty range waits for the fence value of the writer, not an idle-the-queue wait; (c) do it on a helper thread and block only the faulting guest thread; (d) watch GPU clocks during bursty submit/wait cycles (clock dips are plausible on NVIDIA too; unmeasured).

## 4. Precise-write handling: a CPU write over GPU-modified bytes discards, not flushes

Mechanism: when the CPU writes a range that the GPU also modified, the overlapping span is marked dirty for re-upload and removed from the GPU-modified list; the GPU result is dropped, no flush. Reads pass through. Only reads force a flush.
Evidence: [R] only. Related caveat from the PS4 emulator: a CPU write to an unrelated part of a shared 4 KB page can trash GPU-written bytes on the same page at next use [Q].
License: MIT (Switch) / GPL (PS4, ideas).
Maps to us: track GPU-modified state at finer than page granularity (ranges, as in idea 1) so a CPU write to a page that also holds GPU-written bytes uploads only its written bytes (compare against a CPU shadow or use a byte mask) and never flushes or overwrites the GPU bytes. This also removes a class of read-after-write stalls.

## 5. Page bitmaps and flat page-to-buffer maps

Mechanism: buffers aligned to 4 KB host pages; a flat array maps each page of the guest address space to the buffer holding it (no range-tree lookups); per-buffer bit arrays record CPU-modified and GPU-modified pages; scanning uses count-trailing-zeros/ones to walk dirty runs; only dirty pages are uploaded (a 1-byte change in a 512 KB buffer uploads one 4 KB page). A page belongs to one buffer at a time.
Evidence: [N] numbers in idea 1; no isolated per-frame CPU breakdown published.
License: GPL-2.0 family (ideas only).
Maps to us: our per-frame "find what the CPU wrote" and "find changed pieces" should be bitmap scans over dirty pages plus run-length merging, not per-range objects. Tracking cost then scales with dirty pages.

## 6. Resizable BAR / GPU upload heaps: CPU writes VRAM directly

Mechanism: D3D12 has a GPU-upload heap type (L1 on discrete cards with resizable BAR, write-combined; system memory on integrated GPUs). Check the options-16 feature cap for support (developer mode/preview runtime was required at the time of reading). The CPU maps it and writes; the GPU uses it directly with no upload-then-copy step and no state transition. A Vulkan-based D3D12 layer (LGPL-2.1) places its UPLOAD heap in host-visible VRAM when resizable BAR is on, with an environment switch to disable it because it eats VRAM.
Evidence: [Q] vendor says some titles perform better with resizable BAR; the spec gives no numbers. Vendor guidance says CPU reads from this memory are very slow (uncached, write-combined); do not read back.
License: the spec is documentation (free to apply); the translation layer is LGPL-2.1 (ideas only).
Caveats for us: (1) the spec and the tooling blog say the API write-watch heap flag forces GPU-upload memory into system memory, so do NOT use API write-watch with this heap; use our own tracking on the guest side, not on the VRAM mapping. (2) Write-combined: never read it on the CPU, avoid partial/strided writes. (3) VRAM budget: going over falls back to system memory with a loss.
Maps to us: guest memory stays in ordinary cacheable RAM (fastmem/mirror). The "upload" for dirty runs becomes a CPU memcpy straight into the VRAM-resident mapping, removing the staging copy, the GPU copy command and its fence. Worth testing for bandwidth/latency, but it does not fix the GPU-writes-then-CPU-reads direction.

## 7. Batch and coalesce per-range commands

Mechanism: merge adjacent/near-adjacent dirty runs (gap threshold) into one region; one persistent staging ring (grow ~1.5x to avoid frequent resizes); all copies in one command list with one barrier; one download staging buffer sized to the sum, one fence per frame for all downloads (the readback path in the yuzu-family design copies every modified range into one staging buffer, finishes the work, then writes guest memory). Skip the barrier on ranges whose GPU-modified flag is clear.
Evidence: [N] the PS4 numbers in idea 3 show per-call fence waits dominate (68 us x 106); batching is proposed, not yet measured. [R] for the buffer growth heuristic.
License: GPL ideas / MIT.
Maps to us: collapse thousands of tiny copies to tens; at most one fence wait per frame.

## 8. Reduce protection churn

Mechanism: the PS4 emulator measured 86-809 page-protection changes per frame depending on title [N]. The Switch designs re-arm protection only after a sync point and skip re-protect and re-upload when the sequence number is unchanged.
License: ideas only / MIT.
Maps to us: leave pages that are written every frame unprotected and treat them as always-dirty (or compare small hot ranges against a shadow on the CPU); protect only cold pages. Keep a per-page "written in last N frames" counter to move pages between hot and cold.

## 9. Async copy queue and frame-delayed fences

Mechanism: uploads and downloads on a copy queue with CPU waits deferred one or two frames; results consumed when the fence value has passed. D3D12 readback heaps must start in the copy-dest state; use a ring of readback buffers.
Evidence: [Q] the PS4 data indicates submit-time sync waits are what hurt; no direct number for a frame-delayed scheme.
License: n/a (general API practice).
Maps to us: for predicted-needed readbacks (idea 10), issue the copy at frame N and consume at N+1 or N+2 so no thread waits.

## 10. Predicting which ranges the CPU will read

Mechanism: none of the sources I read implement a true predictor. The closest are (a) off/relaxed/precise readback modes, (b) a "fence-detection prepass" that the PS4 project's maintainers declined to revive, and (c) fault history. The known hard case is a GPU histogram feeding a CPU auto-exposure loop, which only works with readbacks.
Maps to us: record ranges that ever caused a fault on a GPU-dirty page; from then on schedule an async frame-delayed readback for those ranges right after the writing submission (idea 9); everything else stays lazy. A 1-2 frame latency is usually acceptable for such feedback loops.

## 11. Aliasing via placed resources

Mechanism: one big heap with buffers and textures placed over the same bytes, or one buffer covering the guest address space, so texture/buffer reinterpretation is a barrier, not a copy. I found no published numbers; this is general D3D12 practice. The Switch emulator's virtual-buffer dependency links (idea 2) are a software version.
Maps to us: only useful if our stall comes from texture<->buffer copies; lower priority.

## What I could not establish

- No source I could read prints a before/after in ms for GPU-to-GPU copy versus flush. The 138 ms to 33 ms target cannot be justified from published numbers; the 56 ms stall is clearly the biggest single item, and the 7.23 ms/frame figure for ~106 fence waits shows how quickly fence waits add up.
- Xbox 360-class emulator D3D12 write-watch details: searches found no usable source, so nothing is claimed.
- Texture-cache designs (PS3 emulator) were not read in detail (the progress-report page refused fetching); only the search summary says protection faults notify the cache of writes.
- Licenses marked "as far as I know" need a check before anyone reads that code closely.

## Suggested order of attack

1. Make shader-written to next-frame-copy dependencies GPU-to-GPU (idea 2) with GPU-dirty propagation. This should eliminate the ~56 ms wait.
2. Replace hash-on-card with bound-writable range tracking plus sync numbers (idea 1); readback off by default with fault-triggered, fence-scoped, helper-thread flush (idea 3).
3. Bitmap dirty scanning and range coalescing (ideas 5, 7).
4. Hot-page unprotect (idea 8).
5. Resizable-BAR direct write for uploads (idea 6) and frame-delayed readbacks for learned ranges (ideas 9, 10).
Instrument each step with per-phase CPU ms so the remaining ~82 ms can be attributed; I do not know its split.
