# The frame budget, measured

Every number here was measured in the world on the test machine (RTX 4070 Ti
12 GB, i7-10700KF), diagnostics build unless said otherwise, with the room
drawn at 4,700 to 5,400 draws a frame. Numbers are only comparable at the same
draw count; a denser room costs more.

## Where the frame goes

At about 4,800 draws:

```
total          283-294 ms
  recording    about 265 ms   (the title's calls into our lists)
  preparing    about 90 ms    (title memory copies, texture refreshes)
  card         60-70 percent busy, about 270 ms of work, overlapped
```

The card and the host now overlap (asynchronous submission), so the total is
not their sum. Recording is the largest single line, then the card.

## Run to run

The same build and switches vary by about 40 percent between runs: one run of
the README's combo read 295-306 ms and the next read 374-743 ms, with the
room at the same draw count both times. Anything under about 50 ms of
difference is noise here and must not be read as a gain; compare medians over
several runs, not single frames.

## What moved it, in order

```
1,500-3,500 ms   the state this work started from, with 5 s stalls
  425-470 ms     residency capped at 90 percent, async submission on
  316-339 ms     residency 70
  283-294 ms     residency 60
```

Also worth, in the host path: a stack buffer for a stage's root constants
instead of a fresh `Vec` a draw (recording 473-733 ms to 265-285), and copying
small ranges whole rather than asking the write watch about each (preparing
copies 97.8 to 87.3 ms; the world's copies average 18 KB over 12,073 a frame).

## What was measured and rejected

- `VICTORY_TEXTURE_ROOM_PERCENT=50`: no better than 60 (268-304 ms).
- `VICTORY_RECHECK_PER_FRAME=128`: worse, 394-407 ms.
- `VICTORY_RECHECK_PER_FRAME=2048`: no better, 293-303 ms.
- `VICTORY_HEAP_RECHECK=0`: worse, 348-359 ms; the copy count did not change,
  so the copies are the title's own volume, not our re-check.
- `VICTORY_WATCH_MIN_BYTES=524288`: worse, 98.2 ms of copies for 312 MB
  against 87.3 for 212 MB at the 64 KiB default.
- `VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1 VICTORY_CARD_BUFFERS_MIN_MB=1
  VICTORY_CARD_BUFFERS_MB=1024`: 295-306 ms in a denser room, and those are
  already this tree's defaults once `VICTORY_LATE_WAIT` is unset with the
  relay on. No gain to be had there.
- The `fxceless/victory-fork` host rewrite (descriptor-sync fingerprint, view
  guard, heap sync; three commits after this tree's base): 666-1,456 ms and
  never reaching the room, and 1,165-2,071 ms with `VICTORY_HEAP_SYNC=0`. It
  does not compose with the residency cap and the wave64 work. Kept on branch
  `fx-main-attempt`; it needs its switches settled against this tree, not a
  blind merge.

## Traps: faster frames that are not wins

- **`--console series-s`: 195 ms against 290, and a black screen.** The title
  takes a different content path at the lower tier, and what it draws there
  does not come out. The speed is missing geometry, not efficiency. Verified
  from a presented frame: 5 fps, 4,361 draws, nothing on screen.
- **The DXIL optimiser commit from triippywhale's line (`e057a00`, block
  merging, disjoint load caches, exact folding):** applies without conflict
  and passes 248 shader tests, but the world never loads -- 3 draws after
  eleven minutes against 4,900 for this tree. Kept as
  `D:\victory-work\patches\dxil-optimiser-e057a00.patch`.
- **`--console series-x`:** 366-380 ms, correct, slower than the PC profile.
- **The `fxceless/victory-fork` host rewrite:** 666-2,071 ms, already noted.

The rule these give: a frame time that drops without the draw count and the
presented image holding up is not a frame time, it is missing work.

## The remaining floor

Recording, at about 55 microseconds a draw. The card's work is the shader
expansion: 26,084 programs translate into 40.6 M IR instructions, about 1,556
each, with 795,624 branches executed per lane and 791,463 guarded
instructions. That is `docs/per-lane.md`, and it is the only route far below
283 ms.
