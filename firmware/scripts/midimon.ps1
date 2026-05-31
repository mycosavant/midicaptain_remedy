<#
.SYNOPSIS
  Thin wrapper around `midimon` (https://github.com/sourcebox/midimon) pinned for
  the MIDI Captain Remedy firmware. Standing instrumentation for observing the
  MIDI the device *emits* — the output-side complement to RTT/defmt (internal
  state) and cdc_config_client.py (config in/out).

.DESCRIPTION
  midimon is a small cross-platform Rust CLI MIDI monitor built on `midir`.
  Install it once:

      cargo install --git https://github.com/sourcebox/midimon

  The firmware's USB-MIDI input port enumerates as "MIDICaptain Remedy (Rust)".
  This wrapper looks the port up *by name* (so a shifting port id doesn't matter)
  and streams its messages in a parse-friendly format — readable by eye, or piped
  into tooling / an agent's log tail for automated behavioural checks (e.g. "push
  a config over CDC, then assert the MIDI it sends").

  WSL can't see USB MIDI — run this from Windows PowerShell.

.PARAMETER Match
  Substring/regex matched against the input port name. Default: "MIDICaptain".
.PARAMETER Format
  Output format: min-hex (default, raw hex bytes) | min (bare, one line/msg) |
  raw (uninterpreted list) | interpreted (midimon's human-readable default).
.PARAMETER List
  List available input ports and exit.

.EXAMPLE
  ./midimon.ps1                 # stream the device as hex, banner suppressed
.EXAMPLE
  ./midimon.ps1 -Format min     # human-ish, one line per message
.EXAMPLE
  ./midimon.ps1 -List
#>
[CmdletBinding()]
param(
    [string]$Match = "MIDICaptain",
    [ValidateSet("min-hex", "min", "raw", "interpreted")]
    [string]$Format = "min-hex",
    [switch]$List
)
$ErrorActionPreference = "Stop"

if (-not (Get-Command midimon -ErrorAction SilentlyContinue)) {
    throw "midimon not on PATH. Install: cargo install --git https://github.com/sourcebox/midimon"
}

if ($List) { midimon list; return }

# `midimon list` prints lines like "  (0) MIDICaptain Remedy (Rust)". Pick the
# id of the first port whose name matches $Match. Capture the id/name groups
# *before* the inner `-match`, which would otherwise clobber $Matches.
$id = $null
$name = $null
foreach ($line in (midimon list)) {
    if ($line -match '^\s*\((\d+)\)\s*(.+?)\s*$') {
        $portId, $portName = $Matches[1], $Matches[2]
        if ($portName -match $Match) {
            $id = $portId
            $name = $portName
            break
        }
    }
}
if ($null -eq $id) {
    throw "No MIDI input port matching '$Match'. Run with -List to see ports."
}

Write-Host "midimon: port $id ($name), format=$Format  -  Ctrl-C to stop"
# `interpreted` is midimon's default (no -f); the machine formats add -q so stdout
# is nothing but messages (clean to redirect/pipe).
if ($Format -eq "interpreted") {
    midimon -p $id
} else {
    midimon -p $id -q -f $Format
}
