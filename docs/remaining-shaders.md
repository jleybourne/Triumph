# What still does not translate, and what it costs

Measured on the default build (wave split on, bounded to thread groups of
128+, LDS split on, ray-tracing builder on): **0 host pipeline refusals**,
and these 8 programs still fail to translate, each with its own reason. All
eight are things the title uses in the world; none is optional.

| program | reason | what it costs |
|---|---|---|
| `PS_DeferredTextured` (`38bf0814e721a1b3`, `78f3f2fc53a69531`) | the first half of an interpolation | surface texture on deferred-lit geometry |
| `PS_PuddleDetect` (`7d0f14ce44746d7e`, `6b6db9f1a526daeb`) | a descriptor read only as data | puddle detection, so wet-ground reflections |
| `CS_WaterTransmittanceOpticalDepthProcessing` (`a929109552ce1e84`) | `v_permlanex16_b32` | water depth/transmittance |
| `CS_MinTimestepReduce` (`ceb5eb68cd99cb07`) | a descriptor's address or format read as data | shallow-water simulation step |
| `CS_TransmissionVolumetrics_Tiled` (`5dc3b25986622949`) | `v_cvt_f32_f64` | light shafts through water and air |
| `CS_TransmissionVolumetrics_Tiled_LocalLights` (`6f225b641c5b0708`) | a wave reduction where not every lane runs | local lights in the volumetric pass |

Two of these are worth doing first because each is one instruction, not a
feature:

- **`v_cvt_f32_f64`** — a 64-bit source in two VGPRs, truncated to `f32`:
  read the pair as `u64`, bitcast to `f64`, convert, write one `f32`. The
  emitter's ALU table is in `crates/victory-shader/src/emit/lower.rs`
  (the `"v_add_f32" | ...` arm is a model of the shape).
- **`v_permlanex16_b32`** — already handled for the wave form in
  `emit/wave.rs`; the compute path here needs the same treatment.

The other four need real work: an interpolation half (the second half of a
value the console keeps in two registers), and a descriptor's address or
format read as data (the same class the fork lists as 40+40 in its own gap
table).

## How to check a fix, in about twenty seconds

    VICTORY_TRANSLATE_ONLY=<hash> \
    ./target/diag/victory.exe translate --list <shaders.rpf>

`--list` prints the program's name and, when it fails, the reason and the
offset in the container. `--pipelines` additionally creates a real D3D12
pipeline, which is what catches a description the driver refuses even when
the shader validates.

## Where the lighting stands

Against the reference frame for this scene, the primary lighting is right
(sun through the window, rim light on the hair, blown highlights, readable
shadow side). Still missing are the second-order terms: bounce onto interior
walls, the light shafts, and dim detail like the bunk frame. Those come from
the irradiance-cache and volumetric passes — the same family as the
volumetrics entries above.
