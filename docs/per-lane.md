# The per-lane question, and why the last two refusals are not a patch

## Where the translator stands

Every program in the archive translates except a handful, and at runtime the refusal count is
two: compute programs `5dc3b2598662` and `6f225b641c5b`, both
`a wave reduction where not every lane runs`.

The cost problem is not refusals. It is that a console shader written for one lane's path per
wave is executed here with every lane running every path, which multiplies the instruction count
of the shaders that matter by four to eight.

## Why the refusal at `lower.rs:1945` cannot be fixed in place

The refusal is raised where a console wave64 reduction has to cross the two host waves
(`exchange_halves`) and the block is not all-lane. The obvious fix -- reconverge for the
reduction, run it, restore the mask -- does not work on D3D12:

- a DXIL wave op (`waveActiveOp`, `waveActiveAllTrue`) is defined over the lanes that are
  **active at that point**; a lane that branched away is not merely masked, it is not executing,
  and nothing in DXIL sets EXEC back to include it;
- so the reduction would combine only the lanes on this path, which is not the console's answer,
  and a shader that accepted it would be quietly wrong rather than visibly broken.

This is why the reference implementation carries each lane's EXEC bit as an `i1` value in
ordinary code instead of relying on the hardware's EXEC: with EXEC as data, a reduction becomes a
combine over values every lane can compute, and lanes that are "off" contribute their identity
element. Any fix for those two programs is therefore part of the per-lane model, not a patch
above it.

Measured cost of leaving it: two programs, and `VICTORY_WAVE_MASKED=1` already runs them, at the
cost of the masked reduction being approximate.

## What the tree already has

The translator is not naive. It already carries:

- `lane_regions` / `all_lane_blocks` (`lower.rs:336-460`): per-lane branch regions and the set of
  blocks that every lane reaches;
- `MaskData::Console` and `wave_wide` (`lower.rs:3821`): whole-wave branches where the wave's own
  answer is enough;
- split-wave64 halves with LDS exchange at barriers (`lower.rs:3723-3760`);
- `VICTORY_WAVE_WIDE` / `VICTORY_WAVE_MASKED` switches that trade exactness for coverage.

So the remaining work is the model change, not the scaffolding: an EXEC value per lane, a live
mask analysis over the CFG to avoid over-guarding, and branch emission that refines the mask
instead of widening to the wave.
