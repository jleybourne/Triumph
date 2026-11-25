# The per-lane form: what to change and in what order

The frame is now bound by the card, not the host. Recording is 298-330 ms
after the stack-buffer change; the GPU's work is the critical path. What the
card executes is our lowering, and this file measures how far it is from what
the console's own programs need.

## Before anything else: the gate does not pass

`cargo test -p victory-shader` gives 244 passed, 1 failed:

    emit::tests::a_mask_whose_high_dword_is_rewritten_is_refused

The test builds `s_mov_b32 exec_hi, 0` (and the vcc_hi forms) after a
compare and expects the translator to refuse the program; it translates
instead. So a mask whose upper half is rewritten alone is accepted today,
where lanes 32-63 should take the mask the low dword was first written with.
That is silent wrong per-lane masking, and it is in the exact machinery the
per-lane form replaces.

Nothing else in this file is worth doing until that refusal is back and the
suite is green: a per-lane change measured against a translator that accepts
malformed masks would be measured against a lie.

### What has been tried against it

Three attempts, each reverted, each narrowing the site:

- **The shape pass (`wave.rs:301`)** folds a constant into lanes 32-63 when the
  high dword is written alone. Restricting that to whole 64-bit moves changed
  nothing: `wave.rs` has no refusals at all, so the refusal was never there.
- **A set of masks whose halves are written at different sites**, refused in
  `mask_dword` and in the opaque-mask arm. It stays empty for this program:
  the lone write never becomes a registered pair, so nothing consults it.
- **A program-level refusal** when a wave64 program writes one dword of EXEC or
  VCC alone. It makes the intended test pass and breaks
  `an_exec_high_write_does_not_overflow_the_stack`, which uses the same
  instruction and must translate: the difference is that its mask is never
  used afterwards.

The conclusion those three give: the guard that makes this wrong is the
**implicit EXEC guard of a store**, and the IR does not record EXEC as a read
of such an instruction (`structure.rs:679` only notes instructions that read
it explicitly). A refusal therefore has to be tied to deriving a guard for an
instruction whose EXEC half was written apart, not to the instruction or the
program.

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
