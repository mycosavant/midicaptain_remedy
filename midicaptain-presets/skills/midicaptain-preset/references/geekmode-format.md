# GeekMode preset format

A GeekMode preset lives in the device's `/geeksetup/` folder. Layout:

```
/geeksetup/
  GeekSetup.txt           global hardware config
  page1/                  16 page folders, page1 .. page16
    gekey0.dat            one .dat per key (key0..key9)
    gekey1.dat
    ...
    gekey9.dat
    keyled.dat            10 LED color indices for the 10 keys
  page2/
    ...
```

Files have `.dat` extension but are **ASCII text with CRLF line endings**.

## gekey<N>.dat -- the key file

12 records, each record = 5 lines:

```
line 1: MIDI channel (1-16)
line 2: action type   (see table below)
line 3: param1        (CC#, note#, PC#, or 0)
line 4: param2        (CC value, velocity, or 0)
line 5: literal "-"   (separator)
```

12 records total:
- Records 1-6  -> press-down actions (sent in order on key press)
- Records 7-12 -> release-up actions (sent in order on key release)

### Action types

| Type | P1                 | P2          | Notes |
|------|--------------------|-----------  |-------|
| `CC` | CC number 0-127    | value 0-127 | standard control change |
| `NT` | note number 0-127  | velocity    | note on; P2=0 sends note off |
| `PC` | program 0-127      | (unused, 0) | program change |
| `UP` | (unused, 0)        | (unused, 0) | page up -- key acts as page+ |
| `DW` | (unused, 0)        | (unused, 0) | page down -- key acts as page- |
| `--` | (ignored)          | (ignored)   | slot disabled |

By convention, **key 4 holds the `UP` action and key 9 holds `DW`** so that
each page has the top-right and bottom-right corners as page nav. You can
violate this if the user wants a different layout (e.g. page nav on key 0
and key 5 for left-side navigation).

### Worked example: BOOST toggle on key 0

`page1/gekey0.dat`:
```
1
CC
16
127
-
1
--
0
0
-
1
--
0
0
-
1
--
0
0
-
1
--
0
0
-
1
--
0
0
-
1
CC
16
0
-
1
--
0
0
-
1
--
0
0
-
1
--
0
0
-
1
--
0
0
-
1
--
0
0
-
```

Press: sends CC#16 value 127 on channel 1 (BOOST on).
Release: sends CC#16 value 0 on channel 1 (BOOST off).

### Stacking actions

For a "broadcast to 3 amps" footswitch, fill press slots 1, 2, 3 with the
same PC on channels 1, 2, 3 respectively:

```
1\nPC\n5\n0\n-     ch1 PC5
2\nPC\n5\n0\n-     ch2 PC5
3\nPC\n5\n0\n-     ch3 PC5
1\n--\n0\n0\n-     (rest disabled)
... etc.
```

This is the unique strength of GeekMode -- SuperMode can't do this without
multi-tap state cycling, which is a worse UX for a single live-performance press.

## keyled.dat -- LED colors

10 lines, one number per line, one per key (key0..key9):

```
1
2
3
4
0
5
6
7
8
0
```

Values 0-21 are palette indices. **Index 0 is the "page-nav" dim color**
(used for keys 4 and 9 by convention). Inferred palette from OEM defaults:

| Idx | Color           | Idx | Color           |
|-----|-----------------|-----|-----------------|
| 0   | off/dim         | 11  | mixed shade A   |
| 1   | red             | 12  | mixed shade B   |
| 2   | green           | 13  | mixed shade C   |
| 3   | blue            | 14  | mixed shade D   |
| 4   | yellow          | 15  | warm white      |
| 5   | magenta         | 16  | mixed shade F   |
| 6   | orange          | 17  | mixed shade G   |
| 7   | purple          | 18  | cool white      |
| 8   | cyan            | 19  | mixed shade I   |
| 9   | pink            | 20  | mixed shade J   |
| 10  | mixed shade     | 21  | mixed shade K   |

(Palette table is best-effort; verify on-device with a test page.)

## GeekSetup.txt -- global config

```
SCREEN_LIGHT  = [1-100]
LED_LIGHT     = [0-100]
BATTERY_CHARGE= [ON | OFF]
WIRELESS_2.4G = [ON | OFF]
WIRELESS_ID   = [1-100]
MIDI_THROUGH  = [ON | OFF]
EXP1_CC#      = [0-127]
EXP2_CC#      = [0-127]
WHEEL_MANUAL  = [ON | OFF]    use encoder to page +/-
WIRELESS_dB   = [0-14]        see OEM docs for dBm mapping
```

Format note: each line ends with comments (`# ...`) followed by `.` filler
characters out to column ~110, then a newline. The filler isn't required
for parsing but matches the OEM-shipped style. Trailing blank line and
`! Do not change anything other than inside the []` footer are likewise
cosmetic but conventional.

## Mode entry

Hold a specific key during USB power-on to set the boot mode:

| Hold key  | Boots into |
|-----------|------------|
| Switch 1  | Update mode (USB drive mounts for file copy) |
| Switch 2  | Normal mode (OEM preset 1-10 selection) |
| Switch 3  | GeekMode |
| (nothing) | Last-used mode (remembered) |

Once selected, the mode is remembered for next normal power-on.
