---
description: Generate MIDI Captain SuperMode or GeekMode preset files from a plain-English description
allowed-tools: Read, Write, Bash, Glob
---

Invoke the **midicaptain-preset** skill. The user wants to author one or more
pages for their Paint Audio MIDI Captain footswitch controller.

1. Confirm which mode they want: **SuperMode** (text, more flexible, recommended)
   or **GeekMode** (text-record format, supports 6 stacked actions per press).
   If they don't say, default to SuperMode.
2. Confirm the **target device** for each page (Katana, Neural DSP, Helix Native,
   FL Studio, REAPER, or "custom").
3. Confirm the **output directory** (default: `./presets/<mode>setup/`).
4. Use the skill's reference docs to pick the right CCs / PCs and write the files.
5. After writing, print a summary table of which key does what on each page.
