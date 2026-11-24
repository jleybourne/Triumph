# Descriptor words a program reads as numbers

Some programs read a descriptor's *fields* (an image's width, height, levels; a
buffer's record count or stride; occasionally the base address) and compute from
them. Nothing binds those fields today, so `descriptor_data` in
`crates/victory-shader/src/emit/lower.rs` refuses such a program and the title
never gets a pipeline for it. That was 940 compute programs before the work
below, which is why every world dispatch had no pipeline.

## What is implemented

- `DESC_DATA_SPACE` (3) and `SCALAR_DATA_SPACE` (4): root SRVs the host fills,
  eight dwords per descriptor entry and sixty-four per scalar entry.
- The translator records the root parameter, declares the table in the program's
  own resource list (`PSV0`), and reads the fields where the console read the
  descriptor's memory.
- The host publishes per draw (`draw.rs`), driven by the root parameter's user
  value.

Both paths are **opt-in** (`VICTORY_DESCTABLE=1`) because turning them on still
draws nothing.

## The finding that matters (do not repeat this mistake)

A title "descriptor" as VIctory sees it is **not** a GPU descriptor block. The
three dwords at the address a program loads from are `{tag, i, generation}`: a
handle onto a host view, decoded by `HostHeap::mirror`. The console descriptor
dwords (base address, size, format, tiling) do not exist at that address in
title memory.

So the table cannot be filled by reading title memory at the address in the root
parameter. Doing that yields one zero word block at a fixed low address (visible
in the `descriptor words published` log), which is what blackens the frame.

## What to do instead

1. Fill the table from the *host view* the record resolves to: dims, level
   count, depth, record count, stride — the same fields `descriptor_data`'s
   synthesised path already builds in the GFX10 layout.
2. For a base-address read, publish the host buffer's own GPU virtual address
   for the tails of that resource we have copied, and translate the loads that
   arithmetic feeds into as host reads. A console address cannot be passed
   through.
3. Validate with `translate --pipelines` (a real DXIL container and a real D3D12
   PSO per program) before running the title. Two of the faults found here — a
   table missing from `PSV0`, and a byte offset used as a slot index — were both
   invisible in the frame and obvious to that check.

## Measurements (corpus `shaders.rpf`, 26,084 programs)

| stage | before | with the table paths emitting |
|---|---|---|
| vertex + pixel | 12,751 | 13,207 |
| compute | 1,444 | 2,396 |
| mesh | 8,215 | 8,610 |

Those counts are the translator accepting the programs; the runtime still needs
the publish above before any of them draw.
