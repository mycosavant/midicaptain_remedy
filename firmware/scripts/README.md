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
| MIDI it emits | the actual MIDI messages on USB/DIN out | `midimon.ps1` |

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

> Rust + open source by design: fork it and extend the decoder (e.g. pretty-print
> Roland/Katana SysEx) directly for this project.
