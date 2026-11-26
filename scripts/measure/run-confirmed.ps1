# The confirmed set, nothing else. Every line here either measured a win in
# docs/runs.md or is the measured submission/card set of the README baseline.
# Unconfirmed switches (LIGHT_REFRESH, EARLY_WORLD, PORT_CONSTANTS,
# HEAP_SYNC_TABLES, LATE_WAITS, DEDUPE_BARRIERS, PRECOMPILED) are deliberately
# absent: PRECOMPILED measured 4x worse per draw, safecopy degraded a run 3x,
# and the all-on set crashed the title at startup.
$ErrorActionPreference = 'Continue'
Get-Process victory -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 10

# Memory work (docs/copies.md): copies 75.4 -> 40.0 ms, copy-back 73.9 -> 46.6.
$env:VICTORY_XMEM_COMMIT        = '1'
$env:VICTORY_INPLACE            = '2'
$env:VICTORY_INPLACE_MORE       = '1'
$env:VICTORY_INPLACE_VIEW       = '1'

# The measured submission and card set (README baseline).
$env:VICTORY_ASYNC_SUBMIT       = '1'
$env:VICTORY_QUEUE2             = '1'
$env:VICTORY_INDIRECT_WINDOW    = '1'
$env:VICTORY_TEXTURE_NO_WAIT    = '1'
$env:VICTORY_MERGE_GAP          = '64'
$env:VICTORY_MERGE_RUNS         = '4096'
$env:VICTORY_STAGING_POOL       = '0'
$env:VICTORY_COPY_PLAIN         = '1'
$env:VICTORY_JOIN_PARTS         = '1'
$env:VICTORY_BACK_OWN_VIEW      = '1'
$env:VICTORY_INDIRECT_GATHER    = '1'
$env:VICTORY_CARD_REHASH_RUNS   = '1'
$env:VICTORY_CARD_WINDOWS_MB    = '2048'
$env:VICTORY_CARD_BUFFERS_MIN_KB= '64'
$env:VICTORY_CARD_BUFFERS_MB    = '1280'
$env:VICTORY_TEXTURE_BACK       = '1'
$env:VICTORY_CARD_HASH_LEN      = '1'
$env:VICTORY_FILLS_PLAIN        = '1'
$env:VICTORY_STEP_INDEX         = '1'

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'run-title.ps1') -Diag -Seconds 480
"run done (read it with: python scripts/measure/frames.py <log>.err)"
