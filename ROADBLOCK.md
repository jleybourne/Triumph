# Roadblocks: why this layer, as it stands, cannot reach a steady 30 fps

Written from measured runs on one machine (i7-10700KF, RTX 4070 Ti 12 GB, 64 GB, 16 GB aperture fully
mapped). Every figure below came from a run of this tree; the doc it came from is named where it exists.
This is not a judgement of the work — it is a list of the walls, in the order they bind.

## The frame we actually have

```
131.3 ms · 7.6 fps median · ~4,500 draws        (best seen: 111.3 ms / 9.00 fps at 3,962 draws)
  preparing        85-110 ms   copies 31-48 ms (83-115 MB), textures ~6 ms
  copying back     48-54 ms    (60-65 MB)
  on the GPU       ~71-90 ms   ~26 ms of it our own hashing, ~45 ms the title's
  list calls       20,293 a frame = 243.2 ms on the submitting thread
  present wait     ~68 ms while the submit thread copies ~72 MB up
```

## 1. The memory model is the architecture, and it is a mirror

The layer gives the title plain CPU memory and keeps a host copy: data is copied in before the GPU reads
and copied back after it writes. That is always correct, and it is why the frame contains 85 ms of copy
work that no amount of tuning removes.

The cost is **per operation, not per byte**: a copy costs **5.5 µs of which ~1 µs is bytes**. Four
separate attempts to cheapen them (merging runs, coarse refreshes, in-place routing, kernel-bypass
`rep movsb`) all measured neutral or worse — `docs/copies.md`.

**Roadblock:** reducing copy cost means removing copies, and removal needs the memory to be the GPU's
memory. That is a different model, not a faster version of this one.

## 2. Placement — the mechanism that would remove the copies — cannot work in this allocation model

The zero-copy port asks whether a range is one contiguous run of one section. The title's heap arrives
**page by page** through `VirtualAlloc`, so the honest answer is always no:

```
port line: 2 not one stretch of a section, 0 not committed, 0 by the host, 0 no buffer — 0 regions placed
```

The sibling mechanisms that make placement work (sparse windows, 64 KiB page mapping at console
addresses) require the allocator to produce what the GPU can consume **from the first call**. In this
tree that is a retrofit into the allocation path, guard bookkeeping and shadows at once.

**Roadblock:** the port is not one bug away from working; it is one memory model away.

## 3. List translation is serial and is the largest single number in the frame

```
20,293 list calls per frame = 243.2 ms on the submitting thread
```

That is larger than the frame we are trying to reach. The sibling mechanism (record the calls as data,
translate them on 8 threads) is not present: `list/thin.rs` is staged on branch `thin-port` but
undeclared, because it needs `crates::cores`, `queue::line_on`, `queue::line_moved` and a `boxed()`
constructor this tree does not have.

**Roadblock:** worth 200+ ms if it lands, but it is a submission-path change, and the tree has no
compiled path for it yet.

## 4. The card's own work is 45 ms and nothing here can lower it

```
the title's GPU work   ~45 ms   (the rest of the ~71 ms is our hashing)
all wave64 divergence  16.5 % of 4.64 M instructions
everything removed     90.8 -> 75.8 ms = 13.2 fps ceiling
```

**Roadblock:** even perfect shader translation cannot reach 30 fps. Only fewer shaded pixels can, which
means the title's own upscaler — and that is blocked (5).

## 5. The upscaler does not run

```
fsrQuality 4            black screen — no picture
display mode 426x240     ignored; the card's work did not change
device method 0x218      works, but 0.115 ms/draw vs 0.029 from the library — 4x worse
```

The title derives its internal resolution from its own settings, and the one level that would help does
not present a picture here.

**Roadblock:** the single largest available win on the card is behind a black screen.

## 6. The remaining costs are spread thin and resist tuning

```
descriptor sync   203,127 slots examined to mirror 12,027 records
hashing           ~26 ms a frame on the card
guards/faults     ~13-69 ms of thread time
```

Nineteen A/B experiments across this area came back neutral or worse: copy coalescing, double buffering,
barrier de-duplication, GPU upload heaps (2x slower, -1,370 draws), guard epochs, present tickets,
ordered copies, wide prepare (ratio 1.00). `docs/runs.md`, rows 20-60.

**Roadblock:** there is no cheap lever left. What remains is index-level work worth a few ms each.

## 7. The process around the code costs as much as the code

- A live game process makes the next launch exit instantly, and the parser then reads a **stale log**.
  Three false conclusions in one night came from that alone.
- An unmeasured switch batch crashed the title at startup and could not be attributed.
- A rebuilt binary asserted inside D3D12 at startup (`0x80000003` at `D3D12Core+0x11632`) and cost three
  runs to isolate to one commit.
- Copy-back numbers vary 48-54 ms and copies 31-48 ms between runs on the *same build*, because draws and
  scenes differ; every comparison has to be normalised or it lies.

**Roadblock:** measurement is expensive here (~10 minutes per world load), so a five-switch question
costs an hour and a wrong conclusion costs a night.

## 8. The arithmetic, stated once

```
now                                    131 ms   7.6 fps
all of our overhead gone, best case    ~45 ms   ~22 fps   <- needs 1, 2 and 3 solved
and the title's card work halved        ~30 ms   ~30 fps   <- needs the upscaler too
```

There is no arrangement of the existing parts that reaches 33 ms.

## What this tree is still good for

- **Shader translation at 100 % coverage** — 25,636 of 25,694 programs, verified by census.
- **The pipeline library** — 8,169 pipelines loaded, `made=0` at runtime.
- **Title knowledge** — every console API this title touches, with the evidence for how it behaves.
- **The descriptor and translation model** — the parts a new layer would otherwise re-derive.

## What would change the outcome

A layer where, from the first allocation, **the title's memory is the GPU's memory**: sparse windows,
64 KiB page mapping at console addresses, write-watch with 64-byte chunk uploads, tiled placement for
GPU-owned resources. That is what removes copy-back rather than cheapening it, and it is why
`triumph-remux` exists as a separate tree rather than a branch of this one.
