---
name: midicaptain-preset
description: Generate Paint Audio MIDI Captain preset files (SuperMode or GeekMode) from plain-English descriptions. Use when the user wants to create, edit, or migrate MIDI footswitch presets for the MIDI Captain device, especially for BOSS Katana, Neural DSP Archetype, Line 6 Helix Native, FL Studio, REAPER, or custom MIDI macro setups. Handles button layout, LED colors, expression pedals, encoder, and the file-format details for both modes.
tools: Read, Write, Bash, Glob
---

# MIDI Captain Preset Author

Generate working preset files for the Paint Audio MIDI Captain footswitch
controller (RP2040, 10 switches + encoder, ST7789 display, ships with two
custom firmware modes: SuperMode and GeekMode).

## When to use

Trigger this skill when the user wants to:

- Create or edit a footswitch page for a specific software/hardware target
  (Katana, Helix Native, Neural DSP Archetype, FL Studio, REAPER, looper, etc).
- Migrate a preset from one mode to the other.
- Set up a macro launcher (e.g. footswitch to open DAW + load template +
  configure audio routing -- usually via Note messages caught by Stream Deck,
  Keyboard Maestro, Hammerspoon, AutoHotKey, BetterTouchTool).
- Debug an existing preset (CCs not landing, LEDs wrong, page nav broken).

## Workflow

### 1. Pick the mode

| | SuperMode | GeekMode |
|---|---|---|
| Pages | 20 | 16 |
| Keys per page | 10 (all customizable) | 8 + 2 reserved for page nav |
| Actions per press | 1-3 via state cycling | **6 stacked** |
| Actions per release | yes | **6 stacked** |
| Long-press | yes | no |
| LED color | 24-bit RGB per pixel | 22-color palette |
| File format | hand-editable text (`page0.txt` to `page19.txt`) | text records (`gekey0-9.dat` + `keyled.dat` per page folder) |

**Default to SuperMode** unless the user needs >1 message per press (e.g.
broadcasting bank+PC across a daisy chain).

### 2. Pick the target(s) and layout

For each page, decide:
- **page_name** (max 4 chars, uppercase) -- shown on the display
- **encoder_CC** -- usually 7 (volume) or 11 (expression)
- **EXP1_CC / EXP2_CC** -- pedal CCs (often 1=wah, 7=vol, 11=expr)
- **10 keys** -- their action, label, LED color

Standard layout convention (top row first):

```
  key0   key1   key2   key3   key4
  key5   key6   key7   key8   key9
```

Key 4 is normally bank+/page+; key 9 is bank-/page-. Key 8 is normally TAP
TEMPO (CC 64 val 64). This is a strong convention -- don't break it unless
the user explicitly asks.

### 3. Read the format spec for the chosen mode

Before writing files, read:
- [references/supermode-format.md](references/supermode-format.md)
- [references/geekmode-format.md](references/geekmode-format.md)

### 4. Pick the right CCs / PCs

Read [references/cc-reference.md](references/cc-reference.md) for verified
CC mappings for Katana / Neural DSP / Helix / FL / REAPER, plus the safe
CC blocks for MIDI-Learn (102-110 for FL, 111-119 for REAPER).

For Katana SysEx (beyond plain CC toggles), the runtime profile in this
repo's `remedy/config/profiles/katana.toml` is the authoritative source.

### 5. Generate the files

Two paths:

**A. Direct write** -- best when the user wants 1-2 pages. Write the
`page*.txt` files (SuperMode) or the `gekey*.dat` + `keyled.dat` files
(GeekMode) directly using the Write tool, following the format specs.

**B. Use the generator script** -- best for ≥3 pages. Edit the page dicts
in `scripts/generate.py` (already includes Katana/NDSP/Helix/FL/REAPER
defaults), then run `python scripts/generate.py --mode {super,geek} --out <dir>`.

### 6. Print a summary

End with a markdown table showing what each key does on each page. Include
deployment steps:

```
1. Hold Switch1 during USB power-on to enter Update mode.
2. Copy the generated files to the MIDICAPTAIN drive.
   - SuperMode -> /supersetup/page*.txt
   - GeekMode  -> /geeksetup/page*/  +  /geeksetup/GeekSetup.txt
3. Eject. Hold Switch3 at power-on for GeekMode, else default SuperMode boot.
```

## Common patterns

### Effect toggle (with LED feedback)

SuperMode: `keytimes = [2]`, two `ledcolor`/`short_dw` pairs cycling on/off.
GeekMode: press slot 1 = `CC <num> 127`, release slot 1 = `CC <num> 0`.

### Macro launcher (DAW / audio routing / mixer scenes)

Send MIDI Notes on a dedicated channel that your OS automation tool listens
to. Recommended channel layout:

| Channel | Purpose |
|---------|---------|
| 1 | Main audio gear (amp, plugin) |
| 14 | Stream Deck / Keyboard Maestro / Hammerspoon listener |
| 15 | OS shortcut macros (window mgmt, app launch) |
| 16 | Reserved / dev experiments |

A Note On + Note Off pair on a known number is more reliable than CC for
macro tools because most can distinguish "trigger" from "release" cleanly.

### Expression pedal hookups

- Wah: CC 1 (most plugins)
- Volume: CC 7
- Expression: CC 11
- Filter/cutoff sweeps: learn anything in the 102-119 range

## Reference files

- [references/supermode-format.md](references/supermode-format.md) -- full SuperMode INI-style spec
- [references/geekmode-format.md](references/geekmode-format.md) -- GeekMode 5-field record spec
- [references/cc-reference.md](references/cc-reference.md) -- verified CC/PC mappings per target
- [references/sources.md](references/sources.md) -- citations / further reading
- [scripts/generate.py](scripts/generate.py) -- batch generator (reads a spec, writes all files)
