# Input research (read-only)

## Verified
- gameinput.rs (HLE, wt-bisect) exposes 6 devices: 4 XInput pads, a keyboard stand-in pad (used only while no XInput pad is connected), and a real keyboard device (kind 0x10). Gamepad kind is 0x40000. No mouse kind exists.
- Run logs (8 of them, victory-run-20261125-*): the title calls GameInputCreate, then registers device callbacks for kind=0x40000 AND kind=0x10 (keyboard), each seeing 1 device. So the title itself asks for keyboard devices. No mouse registration appears. Reading calls are only logged at trace level, so which kinds it reads is not in the logs.
- Keyboard source: GetAsyncKeyState over VK 0x08..0xFE (mouse buttons excluded), gated by focused() (foreground window belongs to our process). It emits scan code + virtual key only; code_point is 0 (a documented shortcut). Only keys held now are reported; no dead keys.
- Mouse-related slots in the device/object tables are `unsupported`/E_NOTIMPL and log a warning; no such warnings appear in the logs.
- Pad fallback map is hardcoded (FALLBACK_BUTTONS plus fallback()), keys give full +-1 stick deflection, no ramp.
- Config: victory-core config.rs parses victory.toml with deny_unknown_fields, [paths] only. A new section needs a struct there. Env overrides exist; VICTORY_KEYS (diagnostics build) scripts key presses.
- settings.xml holds only graphics/video/audio/physics (no input, sensitivity, inversion or layout keys).
- Save profile "SettingsProfileSettings" (JSON, hashed keys like "$0x..."; version 2.0) holds many integer 0-10 values, bools and small enums. Names are hashed, so I cannot say which are sensitivity/invert/layout.

## Inferred
- The 0-10 values and bools in the profile are plausibly look sensitivity, deadzones, invert Y, aim assist, vibration, i.e. the title's own control settings, set in its menu with a pad. Confirm by changing one setting in-game and diffing the file.
- Because the title enumerates the keyboard kind, it likely supports KBM natively (console titles often do), probably reading keyboard for menus/typing/bindings. Whether gameplay uses it, and whether it ever asks for mouse (GameInputKindMouse 0x100000), is unknown: no mouse registration appeared in a menu/early run.

## A. Mapping plan
1. Add [input] to victory.toml: `keymap = { W="LeftStickUp", Space="A", ... }` plus `stick_ramp_ms`, `stick_release_ms`, `deadzone`, `invert_y`. Env switch VICTORY_INPUT_MAP=path. Effort ~0.5 day (serde struct, parse VK names).
2. Replace FALLBACK_BUTTONS with a table built from config; default equals today's. Effort 0.5 day. Unit-test via the existing Source trait fake.
3. Analog ramp: in poll thread (8 ms) per-axis value moves toward target at rate (e.g. 0 to 1 in 120 ms, back in 60 ms); optional "walk" modifier key (Shift scales to 0.5). Effort 0.5 day. Risk: ramps add latency; keep default instant.
4. Per-title control tuning is the title's own menu; our layer need not change it.

## B. KBM plan
- B(1) result: keyboard natively requested (verified); mouse not (inferred, as not registered).
- Stage 1 (verify, 0.5 day): enable trace for gameinput, log reading kinds and any E_NOTIMPL slot hits during gameplay, not just menus. Decide on evidence.
- Stage 2, if the title reads keyboard in gameplay: the keyboard device already works; only add code_point via ToUnicode and key-up/repeat fidelity (0.5-1 day). Then the title's own bindings apply.
- Stage 3, if the title requests mouse: implement the mouse reading struct (buttons, position, wheel, deltas), kind 0x100000, mouse device in the table. Use WM_INPUT raw input (RegisterRawInputDevices, window-bound, no global hook) for relative deltas; ClipCursor to the window plus hidden cursor on capture (~2-3 days).
- Stage 4, if no mouse support: mouse-to-right-stick: accumulate WM_INPUT deltas per 8 ms tick, scale by sensitivity, apply a power curve (v = k*d^1.3, optional acceleration cap), clamp to +-1, decay to 0 when still. Limits: sticks are rate controls, not position, so there is no 1:1 aim; the title's response curve and deadzone distort small motions (counter with inverse-deadzone offset); frame-rate-dependent feel; mouse buttons map to triggers (optional). ~2 days.

## Switches (proposed)
`[input] keymap`, `stick_ramp_ms`, `mouse = "off|native|stick"` (default off), `mouse_sensitivity`, `mouse_curve`, `mouse_capture = true`, hotkey (e.g. F10) to release capture. Env: VICTORY_INPUT_MAP, VICTORY_MOUSE.

## Risks
- Focus: keep the focused() gate for keyboard and mouse; release capture and ClipCursor on focus loss and on window minimise/destroy, or the cursor stays trapped.
- Leakage: GetAsyncKeyState is global, so the gate matters; raw input with no RIDEV_INPUTSINK avoids background mouse capture. Avoid global hooks.
- Keyboard and pad may fight: the stand-in pad is disabled when an XInput pad exists; a native keyboard plus the stand-in pad could double-send (e.g. Enter as A and Enter key). Require a switch to disable the stand-in when the title reads kind 0x10.
- Unknown hashed profile keys: do not edit by hand.
