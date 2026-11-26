<h1 align="center">🙏 Code we used, and who wrote it</h1>

<p align="center"><i>Every project this fork builds on or learned from, and what came from each.</i></p>

<p align="center"><a href="../README.md">← Back to Triumph</a> · <a href="tooling.md">Tooling</a> · <a href="tasks.md">Task log</a></p>

---

Projects are described rather than named by repository, and credited to their authors. Where a commit or pull request is known, it
is given so the work can be traced in that project's history.

## The base

| project | licence | what Triumph takes from it |
|:--|:--|:--|
| **VIctory** (Marvisak / VIctory) and contributors | GPL-3.0-or-later | **Everything underneath**: the loader, runtime services, the Direct3D 12 back end, the shader recompiler, the memory model. Triumph is a fork of it and keeps its history. |
| **the upstream fork** (fxceless) | GPL-3.0-or-later | Our upstream: its base and its measured switch set took the world from 3.4 to 6.9 fps, and later 7.6, before any of the work below. |

## Code ported in, with credit in the commits

| project / author | what | where it went |
|:--|:--|:--|
| **an AMD memory fork** (myokex), adapting pull request #115 by **Spexivus**, its correction by **joroyt**, and part of VIctory #1122 by **tightfall** | the guard over every view of a memory page | commit `c6dc4d8` in that project |
| **Joshua Odom** | the read-beside fix for kernel reads | commit `10ea38e` |
| **the RDNA3 reference fork's** pull request #6 (**Yoshi-233**, adapted from **davutsimsekk** / an AMD fork `4d291d4`) | decoding operandless VOP1, and the EXEC destination of VOP3 CMPX | `wave64-split` `3edc3e67` |
| upstream pull request #2009 | copy-back writes only the GPU's bytes, so a title write in the same stretch survives | `shadow.rs` `copy_back_with` |
| **a precompiled-pipeline fork** (triippywhale), commit `9e7a6322` | precompiled pipelines through device method 0x218 (the title's FSR passes) | branch `fsr` (works, but 4× the cost per draw, so off by default) |
| **an RX 7900 XTX fork** (avatarmanz), commit `86dc1097` | the title's 32-bit heap used in place | `VICTORY_PORT_CONSTANTS` (ported, off by default) |
| **an RX 7900 XTX fork** (avatarmanz), commit `689df313` | the light-refresh lock-free bit check | `watch.rs` (`VICTORY_LIGHT_REFRESH`) |

## Read for ideas (no code copied)

| project | licence | what we learned from it |
|:--|:--|:--|
| **the X reference layer** (ivall and contributors) | **GPL-2.0-only**, which cannot be combined with GPL-3.0 | Its unified-memory design (title memory as 4 GiB heap windows the GPU uses directly) shaped our [zero-copy design](design/uma.md). Its wave64-on-wave32 write-up (halves meeting through groupshared exchanges and arrival counters) is the basis of our wave64 work. **Ideas reimplemented, never code.** |
| **the RDNA3 reference fork** | GPL-3.0-or-later | A documented cost structure on an AMD RDNA3 card: descriptor records left to the draws, hash passes over card copies on 64 threads, preparing on its own thread, THIN list translation, AVX2 compares. Its RTX 4090 measurements (pull request #13, TaynanGT) showed ~25 fps on the same NVIDIA wave limit as ours, with a much faster CPU. That pointed at our CPU-side memory work as the wall. |
| **an AMD fork** (Belobug) | GPL-3.0-or-later | AMD-specific paths, and the source of the decoder fix above. |

## Tools

RenderDoc, Microsoft PIX, and NVIDIA's Nsight Systems, Nsight Graphics and Nsight Aftermath. How and
why each was used is on the [tooling page](tooling.md). None of them ship in this repository.

## This fork

Developed by the repository owner with [Claude Code](https://claude.com/claude-code) (Claude Opus and
Sonnet agents) and Hermes Agent. Every agent result is treated as a claim and checked on disk before it
counts. See the rules in [CLAUDE.md](../CLAUDE.md).

---

*If you wrote something used here and it isn't credited correctly, please open an issue.*
