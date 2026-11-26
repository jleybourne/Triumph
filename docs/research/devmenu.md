# Dev menu research (read-only). V = verified in code, I = inference.

## 1. Part B feasibility
- I: natives sit in a hash -> function table; find it first, then a harmless call, a per-tick point, the player handle, the name hash, streaming.
- I: log by overwriting a table slot; V: inline jmp lacks a length decoder here.
- V: victory-pe `ImageInfo` parses sections, exports, .pdata presence, load-config presence and CodeView (identity only). The exe is mapped as a real module (copy named by hash), so scanning is plain reads of mapped sections. `KNOWN_BUILDS` is empty; nothing is hooked. The loader already changes page protections (IAT) and emits stubs (`thunks.rs`).
- I: console builds are usually stripped: expect no export names or symbols. RTTI unknown; one header and .rdata string dump through existing parse code answers it.
- Table finder (I): once the title idles at its menu, a worker scans .rdata for (hash, pointer) pairs whose pointers land on .pdata function starts in .text, hashes distinct, count in the thousands; confirm with a hash of a name from the title's own data. Read-only, section-bounded.
- Estimate: table 3-7 days, first call 1 week, teleport 1, time/weather 1, spawn 2-3 (design's 6-8 weeks holds).
- Ranking (value/effort): 1 teleport (high/medium); 2 time and weather (medium/low); 3 feature toggles (medium/low each); 4 spawn (high/highest, needs model streaming). Cheapest start: a native-call logger yields the catalogue.

## 2. Hooking
- I: prefer calling. Queue requests from the menu and drain them in an HLE entry the title's game thread calls each frame (gameinput reading is a candidate; no inline patch). V (per design): `present_now` runs on the submit thread, so no native calls there. Thread per HLE entry unchecked.
- V: `crash.rs` logs fatal first-chance faults and the unhandled filter writes crashes.log; watch.rs handles write faults only. A bad call kills the process. I: validate pointers with VirtualQuery; arm a thread-local "in world call" flag so a vectored handler logs, disables Part B and resets RIP/RSP to a recovery point.

## 3. Part A
- I: no heavy dependency needed. About 400 lines: glyph atlas and rect quads in one dynamic vertex buffer, small VS/PS, own root signature, drawn on the back buffer in the present list after `CopyResource`. Skip imgui (C++ build). Keep `fpsbox` as fallback.
- V: `keys()` polls GetAsyncKeyState when `focused()`; no consume path, so design A1 (shared atomic gating keys and pad) fits. F11 is taken by `gpucapture.rs`; F10 is free. Polling ignores window messages, so text entry needs own key-edge polling.

## 4. Version keying
- V: `identify` matches the BLAKE3 hex printed as "build unknown (<hex>)"; PE timestamp and CodeView GUID/age are already parsed.
- I: per-build table (name, rva, pattern, check bytes) in a sidecar under the data dir, not the repo. Verify each pattern at load; on mismatch rescan, then disable Part B. Unknown build means discovery mode only.

## Not checked
Real exe contents (RTTI, strings, CFG) and the thread behind each HLE call.
