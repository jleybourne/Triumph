# RT settings research (read-only, 2026-11-26)

## Verified
- settings.xml (advancedGraphics) has only two RT keys: rtLODLightShadows=false, rtDynamicShadows=false. Related shadow keys: shadowAlphaTintedShadows, shadowLongShadows, shadowParticleShadows (all false), directionalScreenSpaceShadowQuality=0, worldHeightShadowQuality=0, ssroSampleCount=2 (screen-space AO, not RT), waterReflectionSSR=true (screen-space). No RT/GI/AO/probe key exists anywhere in the file.
- settings.xml.bak (older) had rtDynamicShadows, shadowAlphaTinted, shadowParticle = true, so a run with them on was done once; no log tied to it.
- The 40 ms was measured with the builder ON, not "off by settings": the builder is gated by env VICTORY_BVH_BUILDER=1 (pipeline.rs builder_runs), default off. Logs: 10 runs say "builder runs: 53 pipelines"; others say "dropped N/frame: a stage of the ray-tracing structure builder, which is off". Builder-off runs spend no BVH time.
- Breakdown (the RDNA3 reference fork docs/fork-ray-tracing.md, run light-13): CopyForBVHUpdate 13.3 ms; traces 24 ms = RaytraceVBuffer 7.9, screen probes 6.4, irradiance cache 3.5, primary probes 2.3, shadows 1.6.
- The title builds BVHs and traces in that scene while rtDynamicShadows/rtLODLightShadows are false.
- Census: RT programs seen: CS_RaytracingResolve (4, duplicates, translate), CS_TraceShadowRays_{Eighth,Quarter,Half,Full,FullAdaptive} (5, do NOT translate: descriptor chosen by computed index). The docs say the title never created those 5 in any run. The other trace programs (VBuffer, probes, irradiance) translate (the builder's 53 pipelines all have host pipelines).

## Inferred
- Only shadows are user-toggleable. VBuffer, screen/primary probes and irradiance cache are GI/reflection work that the title (console profile) runs unconditionally; they need the BVH, so builds come with them. Nothing in settings names them.
- CopyForBVHUpdate likely serves all dynamic structures regardless of toggles.

## Feasibility of turning RT ON
- rtDynamicShadows: would dispatch CS_TraceShadowRays_*; 5 programs untranslated, so expect a missing pipeline (drop) or broken shadow mask. Needs a host table of structure addresses by slot. Not feasible today. Also builder-on has an open correctness bug (compacted-size reads, memory overwrite) being fixed in wt-rt; wt-rt has no RT commits yet.
- Without the env var, no RT effect can render at all (traces need the builder).

## Can a setting turn BVH work off?
No known setting. Only VICTORY_BVH_BUILDER unset (default), which already costs 0 ms; the 40 ms is the cost of the experimental ON path. Unknown whether the title skips builds when its GI is off, because that GI key is unfound.

## Test plan (one change per run, builder ON, same scene, per-pass GPU timing)
1. Baseline: all false. Record CopyForBVHUpdate, trace ms.
2. rtDynamicShadows=true. Watch: shadows trace ms, "dropped" log lines for TraceShadowRays, crashes.
3. rtLODLightShadows=true. Watch: same, plus new BVH dispatch count.
4. shadowAlphaTintedShadows=true. Watch: any RT dispatch change (may be raster only).
5. shadowParticleShadows=true. Same.
6. shadowLongShadows=true. Same.
7. graphicsQualityPreset (0.0 -> 1.0), then lodScale. Watch whether VBuffer/probe ms change; this may be the unfound GI lever.
Back up settings.xml first; restore after each run. Compare builder-off vs on for the final baseline.
