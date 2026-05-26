# CC / PC reference

Verified MIDI mappings for common targets. Use these without MIDI-Learn --
the target understands them natively.

## BOSS Katana (any generation)

GA-FC compatible CC scheme. The amp listens on its set MIDI channel
(default 1) on both DIN MIDI IN and USB.

| CC  | Function          | Value behavior |
|-----|-------------------|----------------|
| 1   | EXP / Wah         | continuous 0-127 |
| 7   | Volume            | continuous 0-127 |
| 11  | Expression        | continuous 0-127 |
| 16  | Booster on/off    | 0 = off, 127 = on |
| 17  | Mod on/off        | 0 = off, 127 = on |
| 18  | FX on/off         | 0 = off, 127 = on |
| 19  | Delay on/off      | 0 = off, 127 = on |
| 20  | Reverb on/off     | 0 = off, 127 = on |
| 21  | FX Loop on/off    | 0 = off, 127 = on |
| 64  | Tap tempo         | val 64 (any > 63) |
| 80  | GA-FC CTL1        | momentary |
| 81  | GA-FC CTL2        | momentary |

Program changes select stored channels/patches:

| PC  | Channel slot |
|-----|--------------|
| 0   | CH1          |
| 1   | CH2          |
| 2   | CH3          |
| 3   | CH4          |
| 4-7 | extended slots / panel toggle (varies by gen) |

For deep parameter access (gain, tone stack, individual effect type/depth)
use SysEx -- see `remedy/config/profiles/katana.toml` for the address book.

## Line 6 Helix Native (= Helix hardware MIDI chart)

Helix Native is software wrapper around the Helix DSP engine. It accepts
the **same MIDI implementation chart** as the hardware Helix / Helix Rack /
Helix LT / HX Stomp.

| CC    | Function         | Value behavior |
|-------|------------------|----------------|
| 49-53 | Footswitch FS1-FS5 | 0-127 |
| 54-58 | Footswitch FS7-FS11 | 0-127 (FS6 not mapped) |
| 60    | Looper rec/ovd   | 0-63 = overdub, 64-127 = record |
| 61    | Looper play/stop | 0-63 = stop, 64-127 = play |
| 62    | Looper play-once | 64-127 |
| 63    | Looper undo      | 64-127 |
| 64    | Tap tempo        | 64-127 |
| 65    | Looper fwd/rev   | 0-63 = forward, 64-127 = reverse |
| 66    | Looper full/half | 0-63 = full, 64-127 = half speed |
| 68    | Tuner            | 0 = off, 127 = on |
| 69    | Snapshot select  | value 0-7 = snapshot 1-8 |

PC messages 0-127 select Helix presets. The Helix transmits a PC of its
own when the user changes preset on the device (can be disabled in Global
Settings).

## Neural DSP Archetype (any plugin in the line)

Neural DSP plugins have **one** documented default CC -- everything else
is MIDI-Learn.

| CC  | Function | Value behavior |
|-----|----------|----------------|
| 100 | Device Activator + chain selector | 0 = bypass, 1-7 = chain 1-7 |

For anything else (specific stomp on/off, doubler toggle, cab IR pick,
output level): right-click the parameter in the plugin -> "Enable MIDI
Learn" -> press your footswitch -> save the mapping via the MIDI Mapping
window (port icon, bottom-left of the plugin window).

Recommended Learn-target CC block: **102-104** (won't collide with track
plugins and matches the convention used by these presets).

## FL Studio

**No factory CC defaults.** Every binding must be MIDI-Learn-ed.

To map: right-click the FL Studio button (Play, Stop, etc.) ->
"Link to controller..." -> press the footswitch on the device.

Recommended CC block for this skill's presets: **CC 102-110** on channel 1:

| CC  | Suggested function |
|-----|---|
| 102 | Play / Pause |
| 103 | Stop |
| 104 | Record toggle |
| 105 | Metronome toggle |
| 106 | Pattern next |
| 107 | Save project |
| 108 | Undo |
| 109 | Redo |
| 110 | Pattern prev |

## REAPER

**No factory CC defaults** -- use Actions list to learn.

Setup:
1. Options -> Preferences -> MIDI Devices -> enable input + "Enable input
   for control messages" for your MIDI Captain device.
2. Actions -> Show Action List -> filter for the action you want.
3. Click `Add` in Shortcuts section -> press the footswitch.

Recommended CC block for this skill's presets: **CC 111-119** on channel 1:

| CC  | Suggested action |
|-----|---|
| 111 | Transport: Play |
| 112 | Transport: Stop |
| 113 | Transport: Record |
| 114 | Loop: toggle |
| 115 | Marker: go to next marker |
| 116 | File: Save project |
| 117 | Edit: Undo |
| 118 | Edit: Redo |
| 119 | Marker: go to previous marker |

Using **separate CC blocks** for FL (102-110) and REAPER (111-119) means
one session in either DAW can run both pages without conflicts.

## Tap tempo

CC 64 with value > 63 is the de facto industry standard for tap tempo.
Most tempo-aware plugins and devices (Helix, Katana, Strymon, Eventide,
Source Audio, etc.) accept it.

## Macro launcher patterns

For OS automation (Stream Deck, Keyboard Maestro, Hammerspoon, AHK,
BetterTouchTool, MIDIBerry, OSCRouter):

- Prefer **Note On + Note Off** pairs over CCs -- most macro tools handle
  them more reliably and can distinguish "trigger" from "release".
- Use a dedicated **channel** (e.g. 14, 15, or 16) so your DAW/amp don't
  accidentally respond.
- Pick **notes outside the playing range** (e.g. C-1 = note 0 through B0 =
  note 23) so you don't accidentally trigger a connected synth.

Example: macro to launch your DAW with a template

```
Page key:  press = Note On  ch=15 note=12 (C0) vel=127
           release = Note Off ch=15 note=12

Stream Deck rule:
  When: receive Note On ch=15 note=12
  Then: run shell command "open -a 'REAPER' ~/templates/live-session.rpp"
```
