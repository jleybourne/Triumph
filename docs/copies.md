# Copies

The frame moves title memory into the card and back: **114.9 MB forward in 11,542 copies (75.4 ms)**
and **64.7 MB back (73.9 ms)**, out of a 141 ms frame. Everything below is measured on the machine the
README names, and every figure has a row in [runs.md](runs.md).

## The cost is per operation, not per byte

A copy costs about **5.5 µs**, of which the bytes account for about **1 µs**. So the block is 11,500
discrete operations, not a bandwidth problem. Four separate attempts to make each copy cheaper have
measured neutral:

| attempt | result |
|---|---|
| join contiguous ranges (`VICTORY_COPY_MERGE`, `VICTORY_MERGE_RUNS`) | count flat: 11,336–11,795 against 11,542 |
| copies through the kernel instead of the guarded path (`VICTORY_COPY_PLAIN=0`) | frame 152.7 ms against 145.9 — no faster |
| coarser change detection (`VICTORY_REFRESH_COARSE`) | count −5%, bytes **+70%** (194.7 MB) — copies padding |
| zero-copy for private buffers (`VICTORY_INPLACE=2 VICTORY_INPLACE_MORE=1`) | count halves (6,460) but bytes rise 63% (187.4 MB); frame 142.6 against 145.9, inside variance |

The finding those four share: **cheapening a copy does not pay. Only removing one does.**

## Why the page pool cannot be zero-copy today

The title does not allocate memory itself; it calls `XMemAllocatePhysicalPages` in
`crates/victory-hle/src/modules/xmem.rs`, and the layer decides what that becomes. The page pool —
**80–90 MB a frame, the bulk of the traffic** — is created as a `SEC_RESERVE` section
(`xmem.rs`, the `CreateFileMappingW` call), and
`OpenExistingHeapFromFileMapping` **refuses** `SEC_RESERVE` sections, so that memory cannot be placed
in a heap and must be copied. This is not the API's limit on the title, it is a flag the layer picks.

`VICTORY_XMEM_COMMIT=1` creates those sections with `SEC_COMMIT` instead, and a unit test proves the
difference is exactly the blocker: with the switch off the heap call is refused, with it on the heaps
open, a write through one view is visible through the other, and untouched pages still read as zero.

Resizable BAR is not a factor here: BAR1 is 16,384 MiB on a 12,282 MiB card and reads as fully mapped
whether or not a run is in progress, which is how a full-VRAM aperture presents.

## What to expect if the page pool is routed

```
copies        114.9 MB → 30-40 MB   (only the private buffers remain)
copy-back      64.7 MB → 10-15 MB
time          ~100-115 ms of the 149 ms host block
frame         141 ms → 40-60 ms if the routing survives the load
```

Those are estimates until a run prints them. The acceptance counter is
`copies of title memory made in place per frame: N, M MB` — **M must fall**, not rise. A change that
lowers the count while raising the bytes is moving work, not removing it, and has been rejected twice
already.
