# What a frame costs, measured on this machine

RTX 4070 Ti (12 GB), i7-10700KF, Windows 11. The title's world scene, Series X console profile,
diagnostics build. All figures are frame lines observed in `victory-run-*.log.err`.

## Where the tree is now

| tree | draws | fps | frame | copies | copy-back |
|---|---|---|---|---|---|
| `c76e330` (hand-merged tree, warm room) | 5,801 | 3.4 | 295 ms | 68 ms / 227 MB | 55 ms |
| `006c3530` upstream, default switches | ~1,000 | 1.0 | 738 ms | — | — |
| `006c3530` upstream, measured switch set | 2,966 | **9.1** | **109 ms** | 25.5 ms / 80 MB | 38.2 ms |
| menus, either tree | 8-32 | 30.0 | 33 ms | — | — |

The switch set that produced 9.1 fps is the one listed in `diagnostics/README.md` for run 279-280,
applied as environment variables before launch, and it reproduces upstream's claimed 9-12 fps on
this card.

## The load phase is not the frame

The first minute in the world prints frames of 500-3,300 ms with 200-680 MB of copies and
30-100 ms of texture refreshes. Those are streaming reads of the title's data, not the steady
state: after them the same scene settles at 109 ms. Any measurement taken in the first minute
after the world appears understates the frame rate by 10x or more, and every claim in this
document is taken after the spikes stop.

## What dominates the 109 ms frame

- copies 25-55 ms of 80-110 MB,
- copy-back 38-60 ms,
- preparing 59-90 ms, which includes both,
- the GPU line reads 0.0-0.1 ms because submissions are prepared on a layer thread
  (`VICTORY_ASYNC_SUBMIT`); that number is not a GPU measurement and must not be read as one.

So the host side is the frame at this point, not the card. That is the opposite of the tree
before the merge, where the GPU was 155-175 ms of a 295 ms frame.

## Rules that came out of measuring

- **Never accept a frame-time win that draws less.** A tree that lost the world to menus read
  faster than anything here; a scene with two thirds of its draws missing would too. Compare the
  draw count with the frame line, and the picture with a screenshot.
- **Take the number after the spikes.** See above.
- **One run is not a measurement.** The same tree has measured 109 ms and 508 ms on the same
  scene, depending on where in the load the reading fell.
- **A switch set is per machine.** The full set that gives 9.1 fps here contains card-memory
  sizes (2 GiB of windows, 1280 MB of buffers) that leave a 12 GB card little room; the bake-off
  comparing sets with and without them is the check.
