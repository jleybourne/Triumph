# What a frame costs, measured on this machine

RTX 4070 Ti (12 GB), i7-10700KF, Windows 11. The title's world scene, Series X console profile,
diagnostics build. All figures are frame lines observed in `victory-run-*.log.err`.

## Where the tree is now

| tree | draws | fps | frame | copies | copy-back |
|---|---|---|---|---|---|
| `c76e330` (hand-merged tree, warm room) | 5,801 | 3.4 | 295 ms | 68 ms / 227 MB | 55 ms |
| `006c3530` upstream, default switches | ~1,000 | 1.0 | 738 ms | — | — |
| `006c3530` upstream, measured switch set | **744** | 9.1 | 109 ms | 25.5 ms / 80 MB | 38.2 ms |
| `006c3530` upstream, pipeline stall frame | 2,966 | 0.3 | 2,975 ms | 110.8 ms / 177 MB | 70.0 ms |
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

## The 9 fps reading is not a like-for-like frame

The frame that prints 9.1 fps draws **744 draws and drops 522 without a host pipeline**; the one
that prints 2,966 draws is a 2.9-second stall while pipelines are made (`passes 2218.9 ms`).
Neither is a complete scene, so neither can be compared with the older tree's 5,801-draw frame:

| tree | draws | ms | ms per draw |
|---|---|---|---|
| older tree, warm room | 5,801 | 295 | 0.051 |
| upstream tree, 9.1 fps frame | 744 | 109 | 0.147 |

Per draw the upstream tree is about three times worse; it looks faster because it is drawing a
third of the scene. The thing to fix before any frame-rate claim is the pipeline creation that
makes it drop those draws -- `VICTORY_SLOW_PIPELINES` and the pipeline-library cache exist for
that reason, and the run that matters is the one whose frame line reads a full draw count.

A measurement rule that follows: when a frame line carries a draw count and a dropped count,
compare the pair, never the frame rate alone. A frame that drops draws is not a faster frame,
it is a smaller one.

## The first full-scene measurement (600 s run)

A 300-second run is too short: the world spends its first minutes in pipeline loads, and the frames
it prints then are either tiny scenes (744 draws) or stalls (2,975 ms). A 600-second run settles:

| frames (draws >= 1,200) | draws | fps | ms | dropped |
|---|---|---|---|---|
| best | 4,525 | 6.9 | 145.9 | 356 |
| | 4,501 | 6.7 | 149.2 | 298 |
| | 4,839 | 6.7 | 149.2 | 347 |
| | 4,717 | 6.6 | 152.2 | 296 |
| max seen | 6,550 | | | |

Against the older tree at a comparable scene this is a real gain, per draw even more than per frame:

| tree | draws | ms | fps | ms per draw |
|---|---|---|---|---|
| older tree (c76e330) | 5,801 | 295 | 3.4 | 0.051 |
| upstream (006c3530) + switch set | 4,525 | 146 | 6.9 | 0.032 |

**6.9 fps at 4,525 draws, 146 ms, with 356 draws still dropped.** That is the honest baseline: the
full scene, after the pipeline loads, on this machine.
