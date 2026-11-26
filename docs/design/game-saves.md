# Game saves under the layer: diagnosis and design

Scope: read-only research. Source: wt-bisect crates/victory-hle/src/modules/xgamesave.rs (1355 lines), runtime.rs, files.rs, storage.rs. Data: the title's data folder (saves\, storage\, temp\). Logs: D:\victory-work\logs\*.err and the data folder's crashes.log.

## 1. Diagnosis

Verified:
- XGameSave is implemented, not stubbed. The vtable has 30 slots. Implemented: init (async), quota (async), delete (async), container info, enumerate containers, create/close container, enumerate blobs, read (sync and async), create/close update, write/delete blob, submit (async). The README's "saves that survive a kill" is covered by a kill-at-every-step child-process test.
- On disk it works. saves\<xuid>\<user guid>\ holds three containers: Player (v83, last written Nov 26 22:09, 2012-byte data blob plus header 296 bytes), SARC0015 (v1, 250128 bytes, Nov 24 21:40), SettingsProfileSettings (v1, 7344 bytes, Nov 24 14:23). Player version 83 means 82 successful commits across runs, so cross-run persistence and commit work.
- No log line shows a save call failing: no "runtime method not implemented" warning for any area, no 0x8083xxxx code, no save-related line in crashes.log. The only hle warns are DirectStorage page failures and a refused host DLL.
- Unimplemented slots (would log "runtime method not implemented" and return E_NOTIMPL, none seen): 3 (blocking init), 7 (blocking quota), 10 (blocking delete), 15 (containers by prefix), 18 (all blob info), 27 (blocking submit). The title does not call them in any logged run.
- Persistent storage is mounted as drive Z, temp as T (logged each run). No access to S:\ is logged; nothing shows the title uses S:.
- There is no tracing of normal save calls (init, quota result, submit, read), so the logs cannot show which calls the title makes. That is the main observability gap.

Inferred:
- The one save slot with real progress (SARC0015) was last written Nov 24. Runs since Nov 25 ended in GPU device loss "before the title's next save point" (README). So the likely "saves don't work" symptom is no new progress save being reached, not a save failure. Autosave on mission progress was never observed after Nov 24 21:40.
- Silent-loss risks in our code (not seen in logs):
  a. containers() and report() drop a container whose manifest cannot be parsed (errors ignored), so the title sees "no save" and may start fresh; the next submit then returns NO_ACCESS because current() errors, so the title would see a persistent write failure.
  b. current() deletes older versions before validating the newest manifest, so a damaged newest version leaves nothing to fall back on.
  c. No checksum on blobs; a truncated blob (power loss, disk full) is returned as valid data. Directory is not fsynced after the commit rename, so power loss can lose the last commit.
  d. Update is all-or-nothing at submit, but the title's own two-container updates (data and header, or Player plus SARC) are not atomic across containers; a kill between them leaves mismatched containers. The console has the same limit, so this is inferred as acceptable.
  e. Quota is hard-coded 256 MB; the console value comes from the title's package config (runtime title().storage_bytes exists but is not used).

## 2. Design

Layout (keep the current one, already atomic): saves\<xuid>\<config>\<safe name>~<fnv hash>\v<N>\{manifest, 0..n}. Add to the manifest a format version byte and per blob a length and CRC32 (append after the names, parser tolerates the old form).
Atomic write: keep staged v<N>.new, fsync each file, add fsync of the container directory after rename (FlushFileBuffers on a directory handle, or MoveFileEx with WRITE_THROUGH). Delete the old version only after the commit; do not delete older versions in current() until the newest one validates.
Corruption handling: current() validates manifest and blob sizes/CRC; on failure move the bad version to v<N>.bad (never delete), fall back to the next older committed version if one exists, and log a warn with container and reason. If none, report the container as absent but keep the .bad folder. A submit over a quarantined container proceeds with number+1.
Keep two versions: retain v<N-1> until the next successful commit plus one (cost: size), enabling the fallback above.
Quota: report min(title.storage_bytes if set else 256 MB minus used, disk free); keep the two error codes.
Async completion: current helpers complete immediately via xasync::finished*. Matches the console for correctness; also complete after the callback queue dispatch only if a title is seen depending on ordering (not observed). Fill the unimplemented slots: blocking init/quota/delete/submit (reuse same functions), 15 (prefix filter), 18 (blob info for all).
Cross-run persistence: already works; add an INFO line per call (provider open with config, quota result, submit with container, blob names, bytes, version, HRESULT; read with names and bytes; delete) so a run's log proves which calls happen.
Backup before risky runs: a script (not in the game) that copies saves\ to D:\victory-work\save-backups\<timestamp>\ with the data folder's files untouched (robocopy of the saves folder only, and a restore script); in-layer option: VICTORY_SAVE_BACKUP=1 copies the container folder to saves.bak\<ts>\ at provider init, keeping the last 5.

## 3. Staged plan

1. Logging (xgamesave.rs: initialize_async, quota_async, submit_async, read_async/read_blobs, delete_async, container_info, enumerate_*; log HRESULT). Run the title to a save point and read the log to learn the real call set. No behavior change.
2. Missing slots (VTABLE): implement 3, 7, 10, 27 by calling the same cores; 15 via report() with a prefix filter; 18 via enumerate with all names. Unit tests call each vtable slot via method() like the existing tests.
3. Validation and fallback (current(), commit(), manifest()/parse_manifest()): CRC and size check, .bad quarantine, older-version fallback, no early deletion, directory fsync. Tests against a temp dir: truncated blob, garbage manifest, zero-length manifest, missing blob, valid older plus corrupt newest returns older; extend the crash test (a_kill_at_any_step_leaves_old_or_new) with the new steps; old manifests still parse.
4. Quota from title().storage_bytes; test no_room with a small configured quota.
5. Backup tooling: a script under D:\victory-work (outside the repo) and the optional env var in Provider::open; test that the backup holds the old bytes after a submit.

Verify in the game (user or caller runs, not this task): launch, reach an autosave, check the log for "submit ... S_OK" and a new Player or SARC version in saves\; quit (and once kill via taskkill) and relaunch; confirm Continue loads the checkpoint, container versions did not regress, no .bad folders. Back up saves\ first.
