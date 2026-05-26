# midicaptain-presets

A Claude Code plugin that authors **Paint Audio MIDI Captain** footswitch
preset files (SuperMode or GeekMode) from plain-English descriptions.

## What it does

Given a brief like
> "Page 1 = BOSS Katana effect toggles + 4 channels. Page 2 = Helix Native
> snapshots + looper. Use the safe CC blocks for FL Studio and REAPER."

…Claude reads the bundled format specs and CC reference tables, then writes
working preset files into your project. No more wrangling brackets, RGB
hex, or 5-field record layouts by hand.

## Install

```bash
# From any Claude Code session in any repo:
/plugin install midicaptain-presets

# Or, if installing locally from this folder:
/plugin install ./midicaptain-presets
```

## Use

```
/midicaptain-preset

Claude will ask:
  - SuperMode (default) or GeekMode?
  - Which targets per page?
  - Output directory? (default ./presets/)
```

Or just describe what you want in plain English and Claude will pick up
the skill automatically:

> "I need a Helix Native page with snapshots on the top row and the looper
> on the bottom, and a separate page for REAPER transport."

## What ships in the box

| File | Purpose |
|------|---------|
| `commands/midicaptain-preset.md` | Slash-command entry point |
| `skills/midicaptain-preset/SKILL.md` | Skill prompt + workflow |
| `skills/midicaptain-preset/references/supermode-format.md` | SuperMode INI-style spec |
| `skills/midicaptain-preset/references/geekmode-format.md` | GeekMode 5-field record spec |
| `skills/midicaptain-preset/references/cc-reference.md` | Verified CC/PC for Katana, Helix, NDSP, FL, REAPER |
| `skills/midicaptain-preset/references/sources.md` | Where the spec/CCs came from |
| `skills/midicaptain-preset/scripts/generate.py` | Batch generator (both modes, one Python file) |

## Standalone use of the generator

The generator works without Claude. Edit the `PAGES` list at the bottom of
[generate.py](skills/midicaptain-preset/scripts/generate.py), then:

```bash
python generate.py --mode super --out ./presets/supersetup
python generate.py --mode geek  --out ./presets/geeksetup
```

## What's a MIDI Captain?

Paint Audio's RP2040-based MIDI footswitch controller with 10 switches +
encoder + ST7789 display. Ships with two custom firmware modes:

- **SuperMode** -- 20 pages × 10 keys, text INI config, 24-bit RGB LEDs,
  long-press + multi-tap state cycling.
- **GeekMode** -- 16 pages × 8 keys + 2 page-nav, 5-field text records,
  22-color palette, **6 stacked actions per press + 6 per release** (the
  one feature SuperMode can't match).

Both modes are configured by USB mass-storage drop-in -- no proprietary
editor needed once you know the format.

## Sources

- [Helix Native MIDI implementation chart](https://helixhelp.com/tips-and-guides/universal/midi)
- [Neural DSP MIDI control guide](https://neuraldsp.com/getting-started/controlling-plugins-with-midi)
- [REAPER MIDI mapping how-to](https://harmonicbuzz.com/mapping-a-midi-controller-to-reapers-transport-controls/)
- [FL Studio MIDI settings reference](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/envsettings_midi.htm)
