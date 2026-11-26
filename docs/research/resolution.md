# Resolution research

## Verified (code, logs, prior docs)
- Our DXGI HLE (dxgi.rs) implements only: adapter description, factory/adapter enumeration, and the console output's mode list (slot 0xa8). It returns ONE mode: `display_size()` at 60/1, 4K default. Read once via OnceLock, so only the first call counts. Nothing else (SetDisplayMode, ResizeTarget) is implemented.
- `VICTORY_DISPLAY_MODE=WxH` only replaces that mode (range 640-7680 x 360-4320). Logged runs exist at 426x240, 960x540, 1280x720.
- Our presenter (host.rs present_now) sizes the swap chain to the texture the title presents and calls ResizeBuffers if that changes. lifecycle.rs only shrinks the title's window to the screen work area.
- Earlier tree (frame-budget.md, CLAUDE.md blocker 2): with 426x240 or 1280x720 reported, the only picture target made stayed 2560x1440 and GPU cost was unchanged. Scaling targets ourselves in create_committed made the title assert (0x80000003) while loading. fsrQuality 4 gives a black screen; 2 to 3 was a small gain. design/dlss.md says the scene is 2560x1440 at fsr 3 and 4.
- Doc claims "tied to fsrQuality" (settings.md); the research-presets.md ratio (0.667 = FSR Quality) is arithmetic, not observed.

## Inference
- The title likely reads the mode list at boot, but the evidence says it does not use it for scene size: render size follows fsrQuality (or its own dynamic-res floor), output size follows the mode. fsr 3 and 4 giving the same 2560x1440 suggests a clamp, so the ratio may not be a clean function of the level.
- 960x540 gaining 4% fits: only the final upscale/present shrank.
- resolutionFullscreen and uiResolution (0x0 = auto) appear unused on this profile (log shows no effect of 854x480). UI scale presumably follows the output size or the mode.

## Getting 1080p/1440p output with a smaller scene (untested)
1. Output: VICTORY_DISPLAY_MODE=1920x1080 (or 2560x1440). Expect the output buffer to shrink, scene still 2560x1440. Watch that the mode is also what lifecycle's window fit uses.
2. Internal: only fsrQuality moves it. Try 1 and 2 (lower), one level per run, reading the picture-target size in the logs (add a log of each created target >=1280x720 if absent). Do not use 4 (black screen). Test 5 and 6 only as separate runs.
3. Do not scale targets ourselves (asserts).
Watch: UI size relative to mode, black screen, ResizeBuffers log lines, hang at menus.

## Runtime change
Unknown. We never answer a new mode, and the mode list is cached once; ResizeBuffers is host-side only. Test: after boot, change nothing but resize the window; check logs for the title querying modes again.

## Test plan (one change per run)
1. Control: current settings, record target sizes.
2. VICTORY_DISPLAY_MODE=1920x1080 only.
3. Same, 2560x1440.
4. fsrQuality 2 with default mode.
5. fsrQuality 1.
6. Combine the best mode with the best fsr level.
7. Window resize at runtime.
