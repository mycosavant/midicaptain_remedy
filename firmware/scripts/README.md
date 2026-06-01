# Host-side dev / instrumentation tools

Small host utilities for developing and validating the MIDI Captain Remedy
firmware. They make the device observable from the command line — readable by a
human, and (deliberately) parseable by tooling or an agent driving the hardware.

For the end-to-end validation playbook (gate, self-tests, manual checks, feature
matrix, pre-merge checklist) see [`../TESTING.md`](../TESTING.md). This file
documents the host tools themselves.

The three observation surfaces, and the tool for each:

| Surface | What you see | Tool |
|---|---|---|
| Internal state | `defmt` logs over SWD (boot, router decisions, errors) | `probe-rs run` (RTT) |
| Config in/out | the device's live `RuntimeConfig` (read/write/round-trip) | `cdc_config_client.py` |
| MIDI it emits | the actual MIDI messages on USB/DIN out | `midimon.ps1` (+ `sysex_decode.py` to read Roland/Katana SysEx) |

> **Windows only for anything touching USB.** WSL can't see USB serial or USB
> MIDI; run the device-facing commands from Windows PowerShell.

## `cdc_config_client.py` — config sync over USB-CDC

Host client for the device's USB-CDC config-sync link (COBS + CRC-16 framing,
mirrored from `src/proto.rs`; postcard `RuntimeConfig` codec mirrored from
`src/config/mod.rs`). It is the **reference implementation** the web editor's
serializer is ported from — keep it in lockstep with the firmware config model.

```bash
pip install pyserial
python cdc_config_client.py COM9 hello        # probe the link / proto version
python cdc_config_client.py COM9 get          # read + decode the live config
python cdc_config_client.py COM9 get --json cfg.json --raw cfg.bin
python cdc_config_client.py COM9 set cfg.json # push (.json = encode, .bin = raw)
python cdc_config_client.py COM9 tweak        # rename page 1 + recolor a button
python cdc_config_client.py COM9 roundtrip    # GET->SET->GET, assert unchanged
```

Find the port with `python -m serial.tools.list_ports -q 102D` (the device is
VID:PID `2E8A:102D`, serial `RMDY-DEV-0001`).

## `midimon.ps1` — watch the MIDI the device sends

Wrapper around [`midimon`](https://github.com/sourcebox/midimon), a cross-platform
Rust CLI MIDI monitor (built on `midir`). The output-side complement to RTT: push
a config over CDC, then watch the MIDI the device actually emits.

```powershell
cargo install --git https://github.com/sourcebox/midimon   # one-time

./midimon.ps1 -List              # list input ports
./midimon.ps1                    # stream the device as hex (banner suppressed)
./midimon.ps1 -Format min        # human-ish, one line per message
```

It looks the device up *by name* ("MIDICaptain Remedy (Rust)") so a shifting
port id doesn't matter. `-q -f min-hex` (the default invocation) emits nothing
but messages on stdout, so it pipes/redirects cleanly for capture or automated
assertions.

> Pipe its hex output into `sysex_decode.py` (below) to read the Roland/Katana
> SysEx — the boot RQ1 sweep and the amp's DT1 replies — as human lines.

## `sysex_decode.py` — decode Roland / Katana SysEx off the wire

The decode-side companion to `midimon.ps1`, and the lens for validating
**device sync**: it turns the boot RQ1 sweep and the amp's DT1 replies into
human lines — operation, the parameter the address names, the value, and a
checksum check. A pure-Python mirror of `src/midi/katana.rs` (no dependencies);
keep the address map in lockstep with the firmware.

```bash
# Live: pipe a monitor capture in (frames are reassembled across lines)
./midimon.ps1 -Format min-hex | python sysex_decode.py

# Decode bytes directly (with or without 0x / commas), or a saved capture:
python sysex_decode.py F0 41 00 00 00 00 33 12 00 00 04 20 02 5A F7
python sysex_decode.py --file capture.hex

# Verify the decoder itself against known-good vectors:
python sysex_decode.py --selftest        # -> "9/9 ALL PASS"
```

Example, watching a Katana boot sync (`device_query_task` → amp replies):

```text
DT1 set   EditorMode = ENTER  [01]
RQ1 read  AmpType  len=1
DT1 set   AmpType = 3 (Lead)  [03]
RQ1 read  RecallPreset  len=2
DT1 set   RecallPreset = 2 (CH2)  [00 02]
```

A bad checksum is flagged (`!! CHECKSUM 0x.., expected 0x..`) rather than
silently accepted, and non-Roland SysEx is labelled by manufacturer id.
