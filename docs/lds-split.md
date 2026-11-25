# Why 27 pipelines are refused, and what the light needs

A run with the extra passes enabled reports `27 host pipeline refused`,
`error=The parameter is incorrect. (0x80070057)`, and the ray-tracing
structure builder stays off, so nothing in shadow is lit.

## The reason, from the D3D12 debug layer

    ID3D12Device::CreateComputeShader: Shader wants wave lane count of 64
    but maximum wave lane count for device is 32.

The programs are emitted with `WaveSize(64)`. The card's maximum is 32.
Every such pipeline is refused. This has nothing to do with the wave split
being enabled or disabled: the same two programs are refused on the default
path, before any switch is set, which is why enabling switches never moved
the count.

## Why they are left at 64 lanes

`VICTORY_WAVE_WHY=1` names it, per program:

    WAVE64-WHY group=256 lds=true [LDS in a group over 32, an LDS read and
                                   write in one lane-branch region, v_readlane]
    WAVE64-WHY group=64  lds=true [LDS in a group over 32, an LDS read and
                                   write in one lane-branch region]

So the translator declines to split a program whose shared memory (LDS)
traffic spans the whole 64-lane console wave, or which reads and writes the
same LDS region inside a lane-branch region, or which reads another lane
with `v_readlane_b32`. Leaving it entire is the safe answer; it is not a
working one on a 32-lane card.

## The place in the code

`crates/victory-shader/src/emit/lower.rs`, in the compute case of the
lowering: `let mut wave = if x.native.wave64 { 64 } else { 32 };` and the
conditions below it that set `wave = 32`. The diagnostics that name each
refusal reason are a few lines further down (`why.push(...)`,
`WAVE64-WHY`). The pieces that decide whether a split is safe already exist
and are what a fix must use or extend:

- `all_lane_blocks(st)`: `None` for a program with an indirect exit.
- `lds_meetings(program, st, &all)`: `None` when an LDS read and write meet
  inside a lane-branch region.
- `v_readlane_b32` / `v_writelane_b32`, `v_mbcnt`, `ds_permute` /
  `ds_bpermute`, and a mask read as data: each keeps the program whole.

## What a fix has to do

Run LDS for a split program the way the exchange already runs: each console
wave is two host waves, and the two halves must see one another's shared
memory. Today the split gives each half its own dwords for the exchange
(`halves = Some(s.lds_dwords)`); the LDS the title itself uses needs the
same treatment plus a group barrier wherever a half reads what the other
wrote, including inside the lane-branch regions `lds_meetings` cannot prove
safe. `v_readlane_b32` across the halves is the same problem in a register.

## How to test it, in about twenty seconds

No game run is needed to see whether a program can be pipelined:

    VICTORY_HOST_MAX_WAVE=32 VICTORY_WAVE_WIDE=1 \
    VICTORY_TRANSLATE_ONLY=<hash prefix> \
    ./target/diag/victory.exe translate --pipelines --list <shaders.rpf>

`--pipelines` creates a real D3D12 pipeline for each program. `pipeline:
The parameter is incorrect.` is the refusal; `pipeline created` is the goal.
Adding `VICTORY_D3D12_DEBUG=1` prints the driver's own explanation.

Two programs worth starting with, both refused today and both validating:
`alphaResolve:CS_AlphTransmissionPrefilterMip` (`#2da4beb386509b77`) and
`GpuParticleUpdate:CS_Visibility` (`#369cbe906ea72764`).
