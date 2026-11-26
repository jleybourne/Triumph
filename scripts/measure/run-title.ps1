# Launch the title under VIctory and log the run (diagnostics build for measurements).
#   powershell -ExecutionPolicy Bypass -File run-title.ps1 [-Diag] [-Seconds 90] [-NoMove] [-WindowWait 180]
#     -Diag       : target\diag build (debug logs, frame capture, DRED, stack dumps)
#     -Seconds    : stop after N seconds and report the exit code (omit = run freely)
#     -NoMove     : leave the title's window where it opens
#     -WindowWait : how long to look for the window before giving up (default 180 s)
#     -Dev        : development only - open victory-debugger.ps1 in its own window,
#                   following this run's log (never started without the flag)
#
# By default the title's window is moved to the RIGHT half of the primary screen,
# so it does not cover a window on the left.

param(
    [switch]$Diag,
    [int]$Seconds = 0,
    [switch]$NoMove,
    [int]$WindowWait = 180,
    [switch]$NoAutoAdvance,
    [switch]$Dev,         # development only: open the live text debugger window
    [string]$Console = 'series-x',
    # 0.5 is half again as small, beside another window
    [double]$WindowScale = 1.0,
    # testing: after loading, hold W for RoamMs so the character walks out
    # of the start room and into the world
    [switch]$Roam,
    [int]$RoamAfter = 150,
    [int]$RoamMs = 8000,
    # the title's executable (or set VICTORY_GAME_EXE)
    [string]$Game = $env:VICTORY_GAME_EXE,
    # where run logs go (default: logs\ beside the repository)
    [string]$LogDir = ''
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$exe  = if ($Diag) { Join-Path $repo 'target\diag\victory.exe' } else { Join-Path $repo 'target\release\victory.exe' }
$game = $Game
if (-not $game) { throw 'set -Game or VICTORY_GAME_EXE to the title''s executable' }
if (-not $LogDir) { $LogDir = Join-Path $repo 'logs' }
New-Item -ItemType Directory -Force $LogDir | Out-Null

if (-not (Test-Path $exe))  { throw "no $exe  -  build it: cargo build --release -p victory" }
if (-not (Test-Path $game)) { throw "no $game  -  is the dump unpacked?" }

Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class XclWin {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
}
"@

# physical pixels, so the geometry below is not DPI-scaled
[XclWin]::SetProcessDPIAware() | Out-Null

Push-Location $repo
try {
    Write-Host '== config ==' -ForegroundColor Cyan
    & $exe config

    Write-Host "`n== run ==" -ForegroundColor Cyan
    $log = Join-Path $LogDir ("victory-run-" + (Get-Date -f yyyyMMdd-HHmmss) + ".log")
    Write-Host "log: $log" -ForegroundColor DarkGray

    # First-boot wizard walker: the account picker, subtitles, brightness, the
    # two legal notices and the terms checkboxes all advance with A (Enter),
    # and the lists move with the d-pad. This cadence presses both, so a launch
    # lands in the game without anyone touching the keyboard.
    if (-not $NoAutoAdvance -and -not $env:VICTORY_KEYS) {
        $seq = @()
        foreach ($base in 14,26,38,50,64,78,92,106,120,136,152,168,186,204,222,240) {
            $seq += "{0}:Enter" -f $base
            $seq += "{0}:Down"  -f ($base + 3)
            $seq += "{0}:Enter" -f ($base + 6)
            $seq += "{0}:Down"  -f ($base + 9)
        }
        $env:VICTORY_KEYS = $seq -join ','
        Write-Host ("auto-advance: {0} key presses" -f $seq.Count) -ForegroundColor DarkGray
    }

    if ($Dev) {
        # development only: a live text debugger in its own console, following this
        # run's log. Created before the game so the game never writes to a
        # half-open file, and placed on the left half so it does not cover the game.
        # tracing goes to stderr: that is the file worth following
        $trace = "$log.err"
        if (-not (Test-Path $trace)) { New-Item -ItemType File -Path $trace -Force | Out-Null }

        # one debugger at a time: close the previous launch's window
        Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" -ErrorAction SilentlyContinue |
            Where-Object { $_.CommandLine -and $_.CommandLine -match 'victory-debugger\.ps1' } |
            ForEach-Object { try { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }

        $dbg = Join-Path $PSScriptRoot 'victory-debugger.ps1'
        if (Test-Path $dbg) {
            # no -NoExit: the debugger writes a summary and closes when the game ends
            $console = Start-Process powershell -WindowStyle Normal -ArgumentList @(
                '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $dbg, '-Log', $trace
            ) -PassThru
            Start-Sleep -Milliseconds 1200
            try {
                $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
                $cw = [int](($b.Width / 2) - 24); $ch = [int]($cw * 9 / 20)
                [XclWin]::MoveWindow($console.MainWindowHandle, $b.X + 12, $b.Y + 12, $cw, $ch, $true) | Out-Null
            } catch {}
            Write-Host 'debugger: open in its own window (left half)' -ForegroundColor DarkGray
        } else {
            Write-Host "debugger: $dbg not found" -ForegroundColor Yellow
        }
    }

    $p = Start-Process -FilePath $exe -ArgumentList @('run', "`"$game`"", '--execute', '--console', $Console) -NoNewWindow -PassThru `
         -RedirectStandardOutput $log -RedirectStandardError "$log.err"
    $null = $p.Handle
    if ($Roam) {
        Write-Host ("roam: W held for {0} ms after {1} s" -f $RoamMs, $RoamAfter) -ForegroundColor DarkGray
        Start-Job -ScriptBlock {
            param($scripts, $after, $ms)
            Start-Sleep -Seconds $after
            & (Join-Path $scripts 'holdkey.ps1') -Key W -HoldMs $ms
            Start-Sleep -Milliseconds 400
            & (Join-Path $scripts 'holdkey.ps1') -Key W -HoldMs ([Math]::Max(1500, $ms / 2))
        } -ArgumentList $PSScriptRoot, $RoamAfter, $RoamMs | Out-Null
    }

    if (-not $NoMove) {
        $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $half = [int]($b.Width / 2)
        $moved = $false
        $t0 = Get-Date
        while (-not $p.HasExited -and ((Get-Date) - $t0).TotalSeconds -lt $WindowWait) {
            $p.Refresh()
            $h = $p.MainWindowHandle
            if ($h -ne [IntPtr]::Zero) {
                # right side of the primary screen, 16:9, vertically centred
                $m = 24
                $w = [int](((($b.Width / 2) - $m)) * $WindowScale)
                $ht = [int]($w * 9 / 16)
                if ($ht -gt ($b.Height - 2 * $m)) { $ht = $b.Height - 2 * $m; $w = [int]($ht * 16 / 9) }
                $x = $b.X + $b.Width - $w - $m
                $y = $b.Y + [int](($b.Height - $ht) / 2)
                [XclWin]::ShowWindow($h, 1) | Out-Null
                [XclWin]::MoveWindow($h, $x, $y, $w, $ht, $true) | Out-Null
                Write-Host ("window -> {0},{1} {2}x{3} (16:9, right side)" -f $x, $y, $w, $ht) -ForegroundColor Green
                $moved = $true
                break
            }
            Start-Sleep -Milliseconds 500
        }
        if (-not $moved) { Write-Host 'no window appeared (yet) to move' -ForegroundColor Yellow }
    }

    if ($Seconds -gt 0) {
        if (-not $p.WaitForExit($Seconds * 1000)) {
            Write-Host "still running after ${Seconds}s -> stopping" -ForegroundColor Yellow
            Stop-Process -Id $p.Id -Force
        }
        Write-Host ('exit: 0x{0:x8}' -f $p.ExitCode)
    } else {
        $p.WaitForExit()
        Write-Host ('exit: 0x{0:x8}' -f $p.ExitCode)
    }

    Write-Host "`n== crashes.log (tail) ==" -ForegroundColor Cyan
    $dataDir = if ($env:VICTORY_DATA_DIR) { $env:VICTORY_DATA_DIR } else { Join-Path $env:LOCALAPPDATA 'victory\data' }
    $crash = Get-ChildItem -Path $dataDir -Filter crashes.log -Recurse -ErrorAction SilentlyContinue |
             Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($crash) { Get-Content $crash.FullName -Tail 40 } else { Write-Host "no crashes.log yet" -ForegroundColor DarkGray }
} finally {
    Pop-Location
}
