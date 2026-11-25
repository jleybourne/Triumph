# The per-lane form: what to change and in what order

The frame is now bound by the card, not the host. Recording is 298-330 ms
after the stack-buffer change; the GPU's work is the critical path. What the
card executes is our lowering, and this file measures how far it is from what
the console's own programs need.

## The census

Translating the whole archive (`translate --list`):

```
containers   26503 (614 duplicates skipped)
programs     26084 translated of 26084        (100 percent)
IR           40597693 instructions
vector       32535818
guarded        791463
branches per lane  795624    (about 30 a program)
uniform branches   360833
```

About 1,556 IR instructions a program. A console program is a few hundred
vector instructions, so the expansion is four to eight times, and it is the
reason a 12 GB card runs the world at 425 ms instead of the tens of
milliseconds another implementation of the same console reaches.

## Where the expansion comes from

1. **Branches executed per lane, not as control flow** (795,624). A branch
   inside a wave is lowered by running both sides under masks. Every
   divergent branch therefore costs the sum of both paths instead of one
   path. This is the largest single multiplier and the first thing the
   per-lane form removes: when each lane is its own thread, control flow is
   the hardware's.

2. **Guarded instructions** (791,463). The same masking, applied to
   individual ops. Removing (1) removes most of these with it.

3. **Lane exchange through LDS.** Where a program reads or writes across
   lanes (`v_readlane`, `v_writelane`, a shuffle), the split to 32 lanes has
   to stage values in shared memory. `WAVE64-WHY` names these programs.

## Order of work

1. Lower divergent branches as control flow per lane. Target: the 795,624
   per-lane branches and the 791,463 guarded instructions fall out of the
   census entirely.
2. Lane exchanges without LDS where the hardware can do it in one
   instruction (`v_readlane` of a uniform index, a broadcast).
3. Only then look at instruction selection inside a lane.

Each step is measured against the census, and against the world frame time,
before it is kept.

## What not to do

Do not copy code from another implementation of this console. Its licence is
not compatible with this tree's, and its compiler is written against its own
IR. Read it for the shape of the solution, write ours here.

Do not change the wave size to reduce the branch count. `VICTORY_WAVE_WIDE`
was measured: refusals fall, but the title aborts, and it is not the same
thing as per-lane control flow.
