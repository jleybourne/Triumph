# The 348 dropped draws

Every world frame loses about **350 draws** the layer reports as `dropped N without a host pipeline`.
This page says exactly which pipelines they are, why they are missing, and what fixing them is worth.

## It is not a loading race

For a long time the drops were assumed to be pipelines still compiling. They are not:

```
dropped 355 without a host pipeline (0 being made)
host pipelines all made made=0 loaded=8169
```

**Nothing is being made.** Every pipeline comes from the library (8,169 loaded, 0 compiled), and the
draws that drop name pipelines that will never exist. That is why a bounded start pump
(`VICTORY_PIPELINE_PUMP`) measured neutral: it was built for a race that does not happen.

## Six compute programs account for all of it

```
02384a60fda6f2d0    87.9 draws a frame
369cbe906ea72764    87.3
2cc4c197ee29e291    87.2
2da4beb386509b77    86.3
5dc3b25986622949     0.2      the two wave-masked programs everyone chased
6f225b641c5b0708     0.2
```

**The four at ~87 are not the wave reductions**, and together they are 8% of the scene.

## Why they are missing

The shaders translate successfully — the log says `shader translated stage="compute" hash=…` for each.
What fails is the pipeline state:

```
host pipeline refused  error=The parameter is incorrect. (0x80070057)
```

`E_INVALIDARG` from pipeline creation, not from the translator. The layer's root signature declares

```rust
Flags: input
    | D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED
    | D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED,
```

unconditionally (`crates/victory-gpu/src/pipeline/host.rs`), while these four programs report
`samplers=0` and small `user_data` counts — their DXIL does not carry the matching declaration, and
D3D12 rejects that pairing at creation. A root signature whose flags do not match what the shader was
compiled with is invalid, and the error surfaces as a parameter error rather than a shader error.

## What fixing it is worth

- **~350 draws a frame, 8% of the scene**, restored with no extra cost per draw.
- The scene becomes complete, which also makes every other measurement honest — a frame that drops 8%
  of its draws is a smaller scene, and every fps figure in `runs.md` should be read with that in mind.

The change is not a shader project: it is creating the compute pipeline with a root signature whose
`DIRECTLY_INDEXED` flags match the shader's own declaration, cached as a second variant.
