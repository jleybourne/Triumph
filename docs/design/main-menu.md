# Main menu under the layer

Read-only research. No run was made; no menu capture exists (captures show only the in-world title card, logs show no per-menu data).

## 1. Flow
Verified: run-fx.ps1 sets VICTORY_KEYS to 64 presses (16 rounds from 14 s to 240 s: Enter, Down, Enter, Down at +0/+3/+6/+9 s); gameinput.rs scripted() maps them to the pad (Enter=A, Down=d-pad). Its comment lists the walk: account picker, subtitles, brightness, two legal notices, terms checkboxes, then start screen. Logs: one XUserAddAsync (options=1, offline user), intro video, load, world. README: start screen, profile and saves work.
Inferred: the blind Enter/Down cadence also presses through the main menu, so Continue or another entry is chosen by chance. No log or capture proves a menu was seen or what was picked.

## 2. Options
No "runtime method not implemented" line appears in any of 302 logs for any area, so no unimplemented slot was hit. Failures below are risks, not observed.
- Continue: saves work on disk (see game-saves.md). Risk: an unparseable manifest hides the container and Continue greys out.
- New Game over an existing save: probably asks for confirmation through XGameUi slot 0x18 (message dialog), which is missing (E_NOTIMPL) in xgameui.rs. Slots 0x48 achievements, 0x58 player picker, 0x70 error dialog are also missing.
- Load Game: needs container and blob enumeration (works). Slots 15 (containers by prefix) and 18 (all blob info) are E_NOTIMPL.
- Settings: container SettingsProfileSettings exists; no fault seen. Gamer picture (xuser 0x80) is missing, so profile art may be blank.
- Online entries (store, DLC, multiplayer): store offline calls fail with a network error by design; entitled products are empty; licence valid. Network hint never changes. A "needs sign-in/online" greyed entry is expected, not a bug.
- Privileges: check_privilege grants all with reason 0; resolve-with-UI (0xe8) is missing but never needed while all are granted.

## 3. Design
- Add info-level tracing to every xuser, xstore, xgameui, xgamesave and xnetworking entry, so a run records what the menu asks.
- Implement message dialog (0x18): host MessageBox with title, text and up to three buttons, return the pressed index. Add error dialog (0x70) the same way, player picker (0x58) returning the one user, achievements (0x48) returning S_OK.
- Implement save slots 15 and 18 from existing enumeration code.
- Make containers() report an unparseable container as an error, not an absence.
- Gamer picture: return a small generated image.
- Boot-to-menu option: VICTORY_SKIP_INTRO=1 sends the wizard keys only until the start screen, then stops. Add VICTORY_KEYS_STOP_AT=<seconds> so the walker cannot select a menu entry. Menu entries are then chosen by real keys.

## 4. Stages (verify with one screenshot per screen)
1. Tracing in xuser.rs, xstore.rs, xgameui.rs, xnetworking.rs, xgamesave.rs, plus the run: capture start screen, main menu, Settings, Load, New Game. Log shows which slots are asked.
2. xgameui.rs: add message_dialog and error_dialog to VTABLE, replacing missing::<3> and missing::<14>. Capture the New Game confirm dialog.
3. xgamesave.rs: containers by prefix, blob info; harden containers() and current(). Capture Load Game with the three containers.
4. run-fx.ps1: replace the 240 s walker with the wizard-only sequence; add the stop switch in gameinput.rs scripted(). Capture each wizard screen.
5. xuser.rs gamer picture; capture the profile card.
