MIDI Captain SuperMode preset pack (5 pages)
=============================================

DEPLOYING
---------
1. Hold Switch1 during USB power-on to enter UPDATE mode.
2. Mount the MIDICAPTAIN USB drive.
3. Copy these page*.txt files into the device's /supersetup/ folder,
   replacing the existing page0-page4 files (or backing them up first).
4. Eject + power cycle. The device should boot into SuperMode showing KTNA.

PAGE LAYOUT (use bank +/- on key4/key9 to cycle)
------------------------------------------------
  page0  KTNA  -- BOSS Katana (GA-FC compatible CC + PC channels)
  page1  NDSP  -- Neural DSP Archetype (CC 100 chain selector + 3 learn slots)
  page2  HXLP  -- Helix Native + custom looper
                 (snapshots top, looper transport bottom)
  page3  FLSO  -- FL Studio transport (CC 102-110, learn in FL)
  page4  RPER  -- REAPER transport (CC 111-119, learn in REAPER)

MIDI CHANNEL
------------
All pages send on MIDI channel 1. To change, edit the channel in every
[CC]/[PC] tuple (the first number in `short_dw1 = [1][CC][...]`).

KEY POSITIONS
-------------
   key0   key1   key2   key3   key4
   key5   key6   key7   key8   key9
key4 = bank+ / page+ (or tuner on HXLP, with long-press for bank+)
key9 = bank- / page-
key8 = TAP TEMPO (CC 64) on every page

EXPRESSION + ENCODER
--------------------
EXP1 / EXP2 / encoder are page-specific. Defaults:
  Katana:  EXP1=CC1 (wah), EXP2=CC7 (volume), Enc=CC7
  Neural:  EXP1=CC11, EXP2=CC7, Enc=CC7
  Helix:   EXP1=CC1, EXP2=CC11, Enc=CC7
  FL/RPR:  EXP1=CC11, EXP2=CC7, Enc=CC7

CC REFERENCE TABLE (for your own MIDI-Learn sessions)
-----------------------------------------------------
Katana (built-in on the amp, no learn needed):
  CC 16 Boost, CC 17 Mod, CC 18 FX, CC 19 Delay, CC 20 Reverb, CC 21 FX Loop
  CC 80 GA-FC CTL1, CC 81 GA-FC CTL2
  CC 1 wah, CC 7 volume, CC 11 expression
  PC 0-3 channels CH1-CH4 (bank-able for stored patches 5+)

Helix (Native uses the same MIDI chart as the hardware):
  CC 49-53 footswitch FS1-FS5
  CC 54-58 footswitch FS7-FS11 (FS6 not mapped)
  CC 69 snapshot select (val 0-7)
  CC 68 tuner on/off
  CC 64 tap tempo (val 64-127)
  CC 60 looper rec/ovd (0-63 ovd, 64-127 rec)
  CC 61 looper play/stop (0-63 stop, 64-127 play)
  CC 62 looper play-once (64-127)
  CC 63 looper undo (64-127)
  CC 65 looper fwd/rev (0-63 fwd, 64-127 rev)
  CC 66 looper full/half-speed (0-63 full, 64-127 half)

Neural DSP (all Archetypes):
  CC 100 Device Activator + chain selector
    val 0  = bypass
    val 1-7 = chain 1-7
  Everything else: right-click parameter in plugin -> Enable MIDI Learn,
  press footswitch. Save mappings via the MIDI Mapping window (port icon
  at bottom-left of the plugin).

FL Studio / REAPER:
  No factory CC defaults -- everything is MIDI Learn. The pages send on
  channel 1 CCs 102-110 (FL) and 111-119 (REAPER) so the two pages can
  share the same DAW session without colliding.

EDITING TIPS
------------
- `keytimes` controls how many tap-states the key cycles through (1 = momentary,
  2 = on/off toggle, 3 = three-state).
- `ledmode = select` makes the key act radio-style within the page (use for
  patch/snapshot/chain buttons).
- `ledmode = tap` makes the LED blink at the tap tempo.
- LED colors are RGB hex per pixel (3 NeoPixels per switch).
- See MIDICAPTAIN_OEM_BACKUP/setup/FW-SuperMode-4.0-BriefGuide.txt for the
  full SuperMode syntax reference.
