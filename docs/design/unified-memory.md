# Unified memory: what XCL does, and what victory needs to do the same

Read `D:\Developer\xcl` beside this. This document records the *design* of that layer's memory model
and maps it onto our files. It is a description, not a copy: the two trees are different licences and
different module layouts, and the point of writing it down is that we reimplement it in our own terms.

## Why this matters

Our frame is dominated by memory work that exists only because we mirror the title's memory:

```
copies        31-48 ms  (83-115 MB, ~5.5 us a range, ~1 us of it bytes)
copy-back     48-54 ms  (60-65 MB, running at about 1/5 of the bus)
guards/faults 13-69 ms  thread time
```

XCL has none of that, because the GPU addresses the title's memory directly. Their README:
*"title graphics memory shared by CPU and GPU without copies, up to 16 GB (4 GiB windows)"*, and a
retail title at *"a steady 30 fps"* with the console FSR 4 upscaler running.

## The three mechanisms

### 1. Sparse windows over the aperture (`xcl/src/gpu/uma.rs`, `hle/d3d12x/sparse.rs`)

The title's reserved-and-committed memory is given addresses in a unified-memory block. On the GPU each
**sparse window** is a reserved (tiled) 4 GiB buffer whose **64 KiB tiles are mapped onto video-memory
heaps as the title commits pages**. Translated shaders reach those addresses through their memory model
(`mem`/`mem2`) exactly as on the console.

Consequence: a compute pass writing an ocean heightmap writes *into the title's address space*. There is
no readback, because nothing was ever elsewhere.

### 2. Write-watch + 64-byte chunk uploads (`sparse.rs`)

The CPU's view stays the title's own memory. What the CPU wrote is noticed with **write-watch**, hashed
against the last bytes sent, and only the **changed 64-byte chunks** are copied into the tiles — before
the title's next `ExecuteCommandLists`, through an **asynchronous ring-buffered staging queue**, with the
title's queues GPU-waiting on the upload fence. No CPU stall.

Note their reason for chunking rather than page-copying: *"the rest of a written page may hold what the
GPU wrote there (counters, indirect arguments) and must not be overwritten with the CPU's stale view."*
That is a real hazard our whole-range model has too.

### 3. Tiled placement at console addresses (`hle/d3d12x/vram.rs`)

GPU-owned textures (targets, depth, UAV textures) are placed at the addresses the title chose, as
**reserved (tiled) resources whose 64 KiB tiles map onto the pages of those addresses**: one 64 KiB page
of video memory per 64 KiB page of title address, shared by every resource covering it. Resources at one
address share memory, as on the console. Untiled cases (unaligned addresses, D32S8, MSAA) fall back to
arenas that are released after their last user.

## Mapping onto victory

```
xcl gpu/uma.rs            -> crates/victory-gpu/src/uma.rs        (new)  block + windows
xcl hle/d3d12x/sparse.rs  -> crates/victory-gpu/src/sparse.rs     (new)  tiles, commit, CPU chunks
xcl hle/d3d12x/vram.rs    -> crates/victory-gpu/src/place.rs      (new)  tiled targets at title addresses
xcl hle/d3d12x/guard.rs   -> crates/victory-gpu/src/watch.rs      (replace the guard-fault path)
xcl hle/d3d12x/real.rs    -> crates/victory-gpu/src/host.rs       (drop the mirror, keep the queue)
crates/victory-hle/xmem.rs -> allocations become sections/windows instead of plain pages
```

The parts of victory that stay untouched — the translator, descriptor model, shader path, pipeline
library — are the parts XCL also has and is not faster at. The change is confined to memory.

## Order of work, smallest useful slice first

1. **Write-watch replacing guards.** Self-contained, testable without the title: allocate with
   `MEM_WRITE_WATCH`, call `GetWriteWatch` per frame, hash the dirty pages, and prove that the set of
   changed chunks matches what the guard path reported on the same workload. Win: removes the fault
   machinery (13-69 ms of thread time) before anything else moves.
2. **Async upload ring.** Dirty chunks onto a ring with a fence, so the submitting thread never copies
   inline. Win: the 68 ms present wait stops being copy time.
3. **Sparse windows for the title's committed pages.** This is the structural one: the GPU starts
   addressing title memory, and copy-back disappears for everything placed there.
4. **Tiled placement for targets.** Targets stop being mirrored entirely; pages are shared by address.

Steps 1 and 2 are worth doing even if 3 and 4 never land — they attack the copy cost from the CPU side
and are measurable on their own.

## The traps, from XCL's own comments

- **Overwriting GPU-written bytes.** Copy only what the CPU changed, at 64-byte granularity, never a
  whole page: the rest may hold counters or indirect arguments the GPU wrote.
- **Arenas are not free.** Their first attempt (a heap per range) turned 3.5 GB of title addresses into
  15 GB of arenas in two seconds and removed the device. Tiled page-mapping is the fix for anything that
  can be tiled.
- **Fraction of a page.** A placement straddling two arenas made a third over both; pages avoid the whole
  class of that bug.
- **Not everything can be tiled** (unaligned addresses, D32S8, MSAA) — that's what the bounded arena
  fallback with a release timer is for.

## What to measure, and the falsifiable claim

```
before   copies 31-48 ms · copy-back 48-54 ms · guards/faults 13-69 ms · frame 131.3 ms
after    copies -> dirty chunks only · copy-back -> gone for placed surfaces
claim    frame lands at the card's own work (~45 ms) plus the thread's remaining work
falsified if   copy-back falls but frame time does not — that would mean the wall is the
               title's own work, and the memory model was never the limit
```

That last line is the useful part: it distinguishes "our memory model is the wall" from "the title's
45 ms of GPU work is the wall", and we have never had a measurement that separates the two.
