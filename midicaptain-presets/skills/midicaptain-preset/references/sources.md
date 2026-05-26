# Sources

Where the CC/PC numbers and format specs came from. Cite these in summaries
to users so they can verify.

## Format specs

- **SuperMode** -- documented in the OEM-shipped guide at
  `setup/FW-SuperMode-4.0-BriefGuide.txt` on the MIDI Captain flash.
  Mirror page0 sample from the OEM `MIDICAPTAIN_OEM_BACKUP/supersetup/page0.txt`.
- **GeekMode** -- format reverse-engineered from the OEM-shipped
  `MIDICAPTAIN_OEM_BACKUP/geeksetup/page*/gekey*.dat` files.
  Validated against the manufacturer's FW 3.0 introduction video:
  https://www.pantheraudio.com/ -- "Wilson" walkthrough of geek mode
  (16 pages × 8 customizable buttons × 12 commands per button = 1536
  commands total).
- **MIDI Captain hardware reference** -- this skill's parent repo,
  `remedy/lib/pins.py` for the 10-switch / 30-NeoPixel / ST7789 layout.

## CC / PC mappings

- **BOSS Katana** -- GA-FC compatible CC table from BOSS's GA-FC user
  manual; SysEx parameter addresses from BOSS Katana Mk1/Mk2/Gen3 MIDI
  implementation chart. Mirrored in `remedy/config/profiles/katana.toml`.
- **Line 6 Helix Native** -- official Helix MIDI implementation chart.
  Helix Native uses the same chart as the hardware.
  https://helixhelp.com/tips-and-guides/universal/midi
- **Neural DSP Archetype** -- official setup guide.
  https://neuraldsp.com/getting-started/controlling-plugins-with-midi
  -- documents only CC 100 as a default; everything else is MIDI-Learn.
- **FL Studio** -- no factory CC defaults; MIDI-Learn via right-click ->
  "Link to controller..." in the FL Studio MIDI settings docs.
  https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/envsettings_midi.htm
- **REAPER** -- no factory CC defaults; use Actions list to learn.
  https://harmonicbuzz.com/mapping-a-midi-controller-to-reapers-transport-controls/

## MIDI spec

- CC numbers 102-119 are listed as **undefined** in the MIDI 1.0 spec --
  safe for custom assignments that won't collide with most plugins or
  controllers.
- CC 64 (sustain) is universally used for **tap tempo** in guitar gear
  by convention; not part of the MIDI spec but observed across BOSS,
  Line 6, Strymon, Eventide.
