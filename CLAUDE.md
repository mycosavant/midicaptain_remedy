# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Engineering Philosophy

**Be proactive.** Do not write workarounds for old platform limitations when an upgrade is available. If a dependency, runtime, library, or tool has a newer stable version that would eliminate complexity, recommend upgrading immediately. The OEM firmware backup exists in `MIDICAPTAIN_OEM_BACKUP/` — there is always a safety net.

**Production-grade code.** All code should be efficient, safe, stable, and secure. Follow industry best practices. This means:
- No unnecessary abstractions, but no shortcuts that compromise reliability
- Proper error handling at system boundaries
- Memory-conscious patterns appropriate for embedded (RP2040 has 264KB RAM)
- Clean separation of concerns across modules
- Use platform-native features over custom reimplementations (e.g., use `tomllib` instead of a custom TOML parser when available)

**Upgrade-first mindset.** When facing a limitation of the current platform/library version, check if a newer stable version resolves it before writing a workaround. Always prefer removing code over adding compatibility shims.

## Project Overview

MIDICaptain Remedy is configuration-driven CircuitPython firmware for the Paint Audio MIDI Captain (RP2040-based MIDI footswitch controller). It replaces proprietary firmware with a customizable solution supporting arbitrary MIDI devices with deep integration for BOSS Katana amplifiers.

**Target:** CircuitPython 10.0.3 on Raspberry Pi Pico (RP2040)

## Deployment Commands

There is no build system - CircuitPython firmware deploys via direct file copy:

```bash
# 1. Enter Update Mode: Hold Switch1 (GP1) during power-on + USB connection
# 2. Mount the MIDICAPTAIN USB drive
# 3. Copy remedy/ contents to device root:
#    remedy/boot.py    → /boot.py
#    remedy/code.py    → /code.py
#    remedy/main.py    → /main.py
#    remedy/lib/       → /lib/
#    remedy/config/    → /config/
#    remedy/fonts/     → /fonts/
# 4. Disconnect USB, normal boot runs firmware
```

**Serial Debugging:** Use mu editor with CircuitPython support, or `python -m serial.tools.miniterm COM3 115200` from Windows PowerShell (WSL cannot see USB COM ports).

**CRITICAL: boot.py and SPI pins.** In CircuitPython 10 on RP2040, `import storage` in boot.py claims SPI1 pins (GP14/GP15) as a side effect, breaking display initialization. boot.py must NOT import the `storage` module. The same applies to `supervisor.disable_autoreload()` and `storage.remount()`. See `remedy/boot.py` comments for details.

**Test Scripts:** Individual hardware tests in `scripts/` (switch.py, encoder.py, led.py, midi_uart.py, expressionin.py, display_test.py).

## Architecture

```
┌─────────────┐
│  code.py    │ ← Entry point (imports main)
└──────┬──────┘
       ▼
┌─────────────────┐     ┌──────────────┐     ┌──────────────┐
│   main.py       │────►│   Hardware   │────►│   Events     │
│ MidiCaptainApp  │     │ buttons/LEDs │     │ dispatcher   │
│ - init          │     │ encoder/exp  │     │ + actions    │
│ - event loop    │     └──────────────┘     └──────┬───────┘
│ - 30fps display │                                 │
└────────┬────────┘     ┌──────────────┐           │
         │              │   Config     │◄──────────┘
         ├─────────────►│ TOML loader  │
         │              │ profiles/    │
         │              │ pages/       │
         │              └──────────────┘
         ▼
┌──────────────┐        ┌──────────────┐
│   Display    │        │    MIDI      │
│ ST7789 TFT   │        │ USB + DIN    │
│ tuner mode   │        │ Roland SysEx │
└──────────────┘        └──────────────┘
```

## Key Modules (remedy/lib/)

| Module | Purpose |
|--------|---------|
| `pins.py` | GPIO constants, LED mapping (30 NeoPixels, 3 per switch) |
| `config.py` | TOML config loader, hierarchical config (global → profile → page) |
| `hardware.py` | Button debouncing, encoder, LEDs, expression pedals |
| `events.py` | Event dispatcher, action types (CC, PC, SysEx, page nav) |
| `midi.py` | USB+DIN MIDI, Roland SysEx with checksum, Katana helpers |
| `display.py` | DisplayManager, ColorPalette, dirty-flag rendering pattern |
| `tuner.py` | TunerState, TunerController, pitch detection via MIDI |
| `menu.py` | On-device settings menu, expression pedal calibration (NVM) |

## Configuration System

**Hierarchy:** `global.toml` → `profiles/<device>.toml` → `pages/<layout>.toml`

```
remedy/config/
├── global.toml          # MIDI channel, display, LEDs, colors, tuner
├── profiles/
│   ├── katana.toml      # BOSS Katana SysEx (tiered: CC → SysEx → Gen3)
│   └── generic_cc.toml  # Universal CC/PC for any MIDI device
├── pages/
│   ├── default.toml     # Basic MIDI CC/PC controller layout
│   ├── katana-live.toml # Katana live performance (GA-FC CCs)
│   └── daw-control.toml # Generic DAW controller
└── setlists/
    └── example.toml     # Example setlist with song navigation
```

**Button config pattern:**
```toml
[buttons.A]
label = "FX1"
color = "green"
on_press = { type = "midi_cc", cc = 80, value = "toggle" }
on_long_press = { type = "page_next" }
```

## Hardware Constants

- **10 footswitches** + encoder pushbutton
- **30 NeoPixels** (3 per switch, auto-mapped in pins.py)
- **2 expression pedals** (ADC on GP27/GP28)
- **Display:** ST7789 240×240 TFT (SPI on GP12-15)
- **MIDI:** USB-MIDI + DIN UART (GP16/GP17, 31250 baud)

## Roland SysEx Pattern

```python
# Checksum (midi.py)
def roland_checksum(data):
    accum = sum(data) & 0x7F
    return (128 - accum) & 0x7F

# Katana model ID
KATANA_MODEL_ID = [0x00, 0x00, 0x00, 0x33]
```

## Memory Optimization Patterns

- **Dirty-flag rendering:** DisplayElement only updates when marked dirty
- **Object pooling:** Reuse display elements vs creating new ones
- **GC management:** Manual `gc.collect()` every 5 seconds in main loop
- **Lazy color computation:** dim/dark variants cached on first use
- **Display throttling:** Updates capped at ~30fps

## Features

### Page Navigation
Pages are auto-discovered from `config/pages/*.toml`. Buttons with `page_next`/`page_prev` actions cycle through them. Toggle states are cleared on page change.

### LED Toggle Feedback
Buttons with `value = "toggle"` track on/off state. LEDs show full brightness when ON, dimmed (idle_brightness) when OFF. State syncs bidirectionally with incoming MIDI CC.

### Setlist Mode
Configure `setlist = "example"` in `global.toml` `[startup]`. Up/down buttons navigate songs (overrides normal page_next/page_prev). Each song can have `on_enter` MIDI actions. The display title updates to show the current song name.

### Settings Menu
Activated by encoder long-press. Navigate with encoder rotation, select with encoder press. Settings: MIDI Channel, Display Brightness, LED Brightness, Expression Pedal Calibration. Calibration uses a 3-step wizard (set min → set max → confirm) and persists to NVM.

### Bidirectional Device Sync
When `query_device = true` in `[startup]`, the firmware queries the connected device for current effect states on boot (via Roland SysEx RQ1). Incoming CC and SysEx responses update toggle LED states automatically.

## NVM Layout (RP2040)
Expression pedal calibration is stored in non-volatile memory since the filesystem is read-only (CP10 boot.py storage import bug):
- Byte 0: Reserved (SPI reset guard)
- Bytes 1-2: Pedal 1 min (16-bit big-endian)
- Bytes 3-4: Pedal 1 max (16-bit big-endian)
- Bytes 5-6: Pedal 2 min (16-bit big-endian)
- Bytes 7-8: Pedal 2 max (16-bit big-endian)

## Important Directories

- `remedy/` - New refactored firmware (primary development)
- `remedy/fonts/` - PCF bitmap fonts (PTSans variants) for display
- `HKAudio_firmware/` - Reference implementation (tuner ported from here)
- `MIDICAPTAIN_OEM_BACKUP/` - Original Paint Audio firmware backup
- `scripts/` - Individual hardware test scripts
- `docs/dev/KATANA/` - BOSS Katana MIDI/SysEx documentation
