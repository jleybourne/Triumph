# The world scene stalls on pipeline loads, not compiles

## The measurement

A world frame that reaches the full draw count is preceded by a stall of 2 to 3 seconds. Its
frame line reads, for example:

```
0.3 fps, 2974.8 ms/frame (...), 2966 draws/frame, dropped 3790 without a host pipeline
(0 being made) ... heap buffers 1.0 passes 353.2 ms
0.3 fps, 3272.7 ms/frame (...), 2200 draws/frame, dropped 3186 without a host pipeline
(2 being made) ... heap buffers 0.2 passes 2218.9 ms
```

Note `passes 2218.9 ms` and `0 being made`: the time is not spent compiling. The line that says
where it goes is printed once when the pipelines are settled:

```
host pipelines all made  made=0  loaded=2495  started_early=2504
```

**Nothing was made. Two and a half thousand were loaded.** The stall is the cost of getting the
pipeline library's entries into the device while the scene wants to draw with them.

## Why this matters for frame rate

While those loads are outstanding, draws that need a pipeline are dropped: 3,186 and 3,790 in the
two frames above, 168 to 522 in the frames that look "fast" at 744 to 883 draws. So the frames
that read 9 fps are the frames drawing a third of the scene, and the frames that draw the full
scene read 0.3 fps. Neither is the frame rate of the game; the true figure sits after the loads
finish, which is what a longer run is needed to capture.

## What the code says

`crates/victory-gpu/src/pipeline/library.rs` documents both halves:

- the library is safe to use from several threads, but two loads of the same pipeline are made one
  after the other, and under one lock of ours every call serialised: *"87 to 96 of 110 looks at
  each worker found it waiting for that lock (run 49)"*;
- *"an earlier run's 2,300 pipelines, all in the library, were still being loaded 46 s into the
  opening cutscene."*

So the levers, in order of expected value:

1. **Load the world scene's pipelines before the scene asks for them.** The list already exists:
   `early-pipelines.txt` (68 KB) records which title pipelines the last run created, and the next
   run hands them to the workers during loading. What is missing is the world scene's set being
   in that list early enough.
2. **Keep the workers from serialising** on the library lock (`VICTORY_LIBRARY_LOCK=0`, the
   default, with the same-name exception kept).
3. **Only then** consider `VICTORY_SLOW_PIPELINES`, which drops expensive pipelines to get frames
   back — it makes the number look better by drawing less, so it is a last resort, not a fix.

## The number to watch

`passes ... ms` in the frame line is the pipeline time of that frame, and `dropped N without a host
pipeline` is what the scene lost to it. A fix is real when `passes` falls and `dropped` follows it
down **at the same draw count**; a fix that raises fps by drawing fewer draws is not a fix.
