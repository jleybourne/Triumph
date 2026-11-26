<h1 align="center">🔬 Advanced research</h1>

<p align="center"><i>Where the frame time goes, what could remove it, and how far this machine can go.</i></p>

<p align="center"><a href="../../README.md">← Back to Triumph</a> · <a href="../design/README.md">Designs</a> · <a href="../tasks.md">Task log</a></p>

---

Each report was written by a read-only research agent against the code and the measured runs in
[runs.md](../runs.md). They were not checked line by line, so treat a report's estimates as
estimates. Every report marks what it verified apart from what it guessed. Where two reports
disagree, the disagreement is listed below rather than smoothed over.

## Reports

| | report | question it answers | headline |
|:--:|:--|:--|:--|
| 🎯 | [**Path to 60 and 120 fps**](path-to-60-and-120.md) | What beyond the current plan gets us to 60, then 120? | Overlapping the title's frame with ours (`VICTORY_ASYNC_PRESENT`) is the biggest untested lever. On this card, the GPU is the wall at ~45 ms even after all planned work. |
| 🧮 | [**Native 60 fps on the GPU**](native-60-gpu.md) | Why does the title's GPU work cost ~45 ms here, and how is it recovered without upscaling? | The "3× less efficient" premise is soft (closer to 1.65× for this integer-heavy code). Cost is a flat tail of ~1,290 kinds of work. The biggest card-side cost is the structure around indirect draws and dispatches. |
| 🧩 | [**Running parts natively**](native-paths.md) | What could bypass emulation instead of speeding it up? | No PC shaders or PC build ship with the title, so translation stays. The real bypasses are memory living on the card, the upscaler hook, and GPU-side texture detiling. |
| 💾 | [**Caching**](caching.md) | Which caches cut load time, stutter and frame time? | Cheapest wins: warm the shader cache after each translator change, preload the working set into RAM. The NVIDIA driver cache is 32 GB and worth investigating. |
| 🔁 | [**Passthrough audit**](passthrough.md) | Where do we translate what D3D12 could take 1:1? | Mostly passed through already. Best left: constant-buffer tables rebuilt per draw (10–30 ms of recording time). |
| ☀️ | [**Ray tracing**](raytracing.md) | How to make RT fast on NVIDIA? | Emulated BVH tests are ~20–100× below hardware; faster emulation first, native DXR only if loop shapes are few. |
| 🔦 | [**RT settings**](rt-settings.md) | What do the RT settings do? | The ~40 ms BVH figure is from builder-on runs only; with it off, ray-traced lighting is **dropped**, so correct lighting costs time. |
| 🎚️ | [**Settings**](settings.md) · [**Presets**](presets.md) | Which title settings cut GPU work? | Already near the floor; a few ms at most. Presets aren't readable; one test of the preset slider decides if it's live. |
| 🖥️ | [**Resolution**](resolution.md) | How does the title pick its resolution? | Output follows the mode we report (4K); the scene size follows `fsrQuality`. Plan: report 1080p, then step fsrQuality. |
| 🎮 | [**Input**](input.md) | Control mapping, native keyboard/mouse | The title registers for keyboard devices; mouse unseen. Configurable key map first, then native or emulated mouse. |
| 🛠️ | [**Developer menu**](devmenu.md) | How would in-world tools work? | Feasible via the title's native table; discovery first; 6–8 weeks for world tools. |
| 🎨 | [**Visual fidelity**](fidelity.md) | Why doesn't it look like the release? | ~336 of ~339 dropped draws are the RT builder stage (off): the missing GI is the dark look. Texture slot budget is full. A release-look settings profile to apply one item per run. |
| 🕹️ | [**Console techniques**](console-tricks.md) | What does the title lean on, and how should each map to PC? | Ranked: unified memory → GPU-driven indirect → GPU detile → async compute (after zero-copy) → DLSS; mesh shaders already native. |
| 🔧 | [**Fork renderer ports**](fork-renderer-ports.md) | Which of the RX 7900 XTX fork's CPU-side ports apply to us? | Port 6 (guard lock + parallel refresh planning) first, then port 5 (constant buffers in place), 2, 7. Port 6 is merged. |

## What the reports agree on

```mermaid
flowchart LR
    A["Today<br/>~127 ms · 7.9 fps"] --> B["Zero-copy memory<br/>−35 to −65 ms submit thread<br/>−20 ms card"]
    B --> C["Card-bound<br/>~45–55 ms · 18–22 fps"]
    C --> D["Card work cut<br/>indirect/dispatch structure,<br/>memory() emulation, scalar work"]
    D --> E["30 fps native"]
    E --> F["60 fps displayed<br/>with Frame Generation"]
```

- **Our layer's memory mirroring is the wall today**, not the title or the GPU.
- **After that, the GPU is the wall**, and no single shader fix recovers the gap.
- **Native 60 fps is a stretch on this hardware.** 60 fps *displayed* via Frame Generation on a real ~30 is plausible. Real 120 is not.

## Open disagreements

| topic | one view | other view | settles it |
|:--|:--|:--|:--|
| Cost of masked lanes | `per-lane.md`: 4–8× on hot shaders | census: 16.5 % of instructions, 2.4 % guarded | a per-shader card-time join (cardmeter + census) |
| Title's GPU efficiency | "~3× worse than console" | ~1.65× once integer work is counted | PIX / Nsight timing of the top pipelines |
| Value of THIN | 75 → 68 ms on another fork | 0.0 ms of recording on our render thread | a run with THIN that shows *present* shrinking |
