# SuperMode preset format

A SuperMode preset lives in the device's `/supersetup/` folder as a set of
`page0.txt` through `page19.txt` files (up to 20 pages). Each file is INI-ish
text with bracketed `[section]` headers and `key = [value]` lines.

## File-level structure

```
[globalsetup]
... (global hardware settings; only page0's is honored)

[PAGE]
... (per-page settings: name, encoder, expression pedals, display mode)

[key0]
... (10 key blocks, key0..key9, each with up to 3 states)
[key1]
...
[key9]
```

## [globalsetup]

```
ledbright         = [0-100]              LED master brightness
screenbright      = [0-100]              screen backlight
dark_fonts        = [on | off]           font color (off=light, for dark wp)
wallpaper         = [wp1]                only wp1 ships by default
long_press_timing = [1 | 1.5 | 2 | 2.5]  seconds
WIRELESS_2.4G     = [on | off]
WIRELESS_ID       = [1-99]
WIRELESS_dB       = [0-14]               see table in OEM docs
```

Only the `[globalsetup]` in `page0.txt` is applied at boot. Including it in
later pages is harmless (some users keep it as a self-contained template).

## [PAGE]

```
page_name           = [XXXX]      <= 4 chars uppercase, shown on display
exp1_CH             = [1-16]
exp1_CC             = [0-127]
exp2_CH             = [1-16]
exp2_CC             = [0-127]
encoder_CC          = [0-127]
encoder_NAME        = [XXXX]      <= 4 chars, shown above encoder value
midithrough         = [on | off]
display_number_ABC  = [123 | abc3 | abc4 | abc5 | abc8]
group_number        = [3 | 4 | 5 | 8]
display_pc_offset   = [0 | 1]
display_bank_offset = [0 | 1]
```

## [keyN]

A key has 1, 2, or 3 *states*, each with its own LED colors and action set.
Pressing the key cycles to the next state.

```
[keyN]
keytimes  = [1 | 2 | 3]                    how many states this key has
ledmode   = [normal | select | tap]        select=radio-style within page;
                                           tap=blinks at tap tempo
ledcolor1 = [0xRRGGBB][0xRRGGBB][0xRRGGBB] three RGB hex for the 3 NeoPixels
short_dw1 = [CH][TYPE][P1][P2]             action on press (state 1)
short_up1 = [CH][TYPE][P1][P2]             action on release (optional)
long1     = [CH][TYPE][P1][P2]             action on long press (optional)

# Repeat for state 2 if keytimes >= 2:
ledcolor2 = ...
short_dw2 = ...
short_up2 = ...
long2     = ...

# Repeat for state 3 if keytimes == 3:
ledcolor3 = ...
short_dw3 = ...
short_up3 = ...
long3     = ...
```

### Action tuple `[CH][TYPE][P1][P2]`

| TYPE  | P1                  | P2                  |
|-------|---------------------|---------------------|
| `CC`  | CC number 0-127     | CC value 0-127      |
| `CCT` | CC number 0-127     | toggle target value |
| `PC`  | `auto` or 0-127     | bank action or PC#  |
| `NT`  | note number 0-127   | velocity 0-127      |

For `PC`:
- P1 `auto` means "use current bank+group accounting"
- P2 can be a fixed program number, `bank_inc`, `bank_dec`, `inc1`, `dec1`

### LED color triplet

Each switch has 3 NeoPixels. `ledcolor1 = [0xff0000][0x00ff00][0x0000ff]`
lights the left LED red, middle green, right blue. Use the same hex three
times for a solid color: `[0xff0000][0xff0000][0xff0000]`.

For "dim when off" toggle feedback, set state-2 colors to a low-intensity
version of state-1, e.g. `0xff0000` -> `0x220000`.

## Worked example: BOOST toggle

```
[key0]
keytimes = [2]
ledmode  = [normal]
ledcolor1 = [0x00ff00][0x00ff00][0x00ff00]
short_dw1 = [1][CC][16][127]

ledcolor2 = [0x002200][0x002200][0x002200]
short_dw2 = [1][CC][16][0]
```

State 1: bright green LEDs, press sends CC#16 val 127 (BOOST on).
State 2: dim green LEDs, press sends CC#16 val 0 (BOOST off).
Pressing the key cycles between the two.
