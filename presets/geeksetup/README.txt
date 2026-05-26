MIDI Captain GEEK MODE preset pack (5 pages)
============================================

5 pages matching the SuperMode pack at ../supersetup/, in geek mode's
text-record format.

DEPLOYING
---------
1. Hold Switch1 during USB power-on to enter UPDATE mode.
2. Mount the MIDICAPTAIN USB drive.
3. Copy these page* folders + GeekSetup.txt into the device's /geeksetup/
   folder (rename existing first if you want to keep them).
4. Power-on holding Switch3 (key3) to boot into GEEK mode.

GEEK MODE FILE FORMAT (reverse-engineered)
------------------------------------------
gekey<N>.dat -- one per key, 12 records of 5 lines each (60 lines total):

  Line 1: MIDI channel (1-16)
  Line 2: type -- one of:
            CC  = control change
            NT  = note on
            PC  = program change
            UP  = page UP (no MIDI sent)
            DW  = page DOWN
            --  = slot disabled (params ignored)
  Line 3: param1 -- CC#, note#, or PC#
  Line 4: param2 -- CC value, velocity, or unused
  Line 5: literal "-"  (separator)

12 records = 6 press-down actions + 6 release-up actions, in order:
  records 1-6  -> on press
  records 7-12 -> on release

keyled.dat -- 10 lines, one palette index 0-21 per key (key0-key9).
Convention: keys 4 and 9 are page-nav; their LED is index 0 (dim).
Inferred palette: 0=off/dim, 1=red, 2=green, 3=blue, 4=yellow,
5=magenta, 6=orange, 7=purple, 8=cyan, 9=pink, 10-21=mixed shades.

PAGE LAYOUT
-----------
  page1  Katana   -- GA-FC effect toggles + PC channels
  page2  NDSP     -- Neural DSP chain select + learn stomps
  page3  Helix    -- Helix snapshots + custom looper
  page4  FL       -- FL Studio transport (CC 102-110, learn in FL)
  page5  REAPER   -- REAPER transport (CC 111-119, learn in REAPER)

  Key positions:    key0  key1  key2  key3  key4 (=UP/PAGE+)
                    key5  key6  key7  key8  key9 (=DOWN/PAGE-)
  Key 8 on every page = TAP TEMPO (CC 64 val 64)

REGENERATING
------------
Run `python _generate.py` from this folder to regenerate all 55 .dat files
from the Python definitions. Edit the page dicts in _generate.py to remap
any button, then rerun.

WHY YOU MIGHT WANT SUPERMODE INSTEAD
------------------------------------
SuperMode (../supersetup/) is more configurable than geek mode for most
single-button uses: more pages (20 vs 16), more keys per page (10 vs 8),
long-press actions, multi-tap state cycling, 24-bit RGB LEDs.

Geek mode wins when you need 6 stacked MIDI messages on a single press
(e.g. broadcasting CC + PC + Note across a daisy-chained rig). Both
packs ship the same five target configs so you can compare on-device.
