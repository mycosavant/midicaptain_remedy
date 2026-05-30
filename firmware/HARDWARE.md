# Hardware — Paint Audio MIDI Captain

RP2040 (Cortex-M0+, dual-core, 264 KB SRAM, 2 MB QSPI flash). Single
board; OEM PCB labelled "MIDICaptain". The pin map below mirrors
[`src/pins.rs`](src/pins.rs) — keep them in sync. Authoritative source
is the reverse-engineered map from
[`../remedy/lib/pins.py`](../remedy/lib/pins.py).

## Pin map

### Footswitches (active LOW, internal pull-up)

| Switch | GPIO | Notes |
|---|---|---|
| 1 | GP1  | Also used by OEM as boot-mode select at power-on |
| 2 | GP25 | |
| 3 | GP24 | |
| 4 | GP23 | |
| A | GP9  | |
| B | GP10 | |
| C | GP11 | |
| D | GP18 | |
| up   | GP20 | |
| down | GP19 | |

### Rotary encoder

| Signal | GPIO | Notes |
|---|---|---|
| Phase A | GP2 | Quadrature |
| Phase B | GP3 | Quadrature |
| Push    | GP0 | Active LOW |

### NeoPixel chain (WS2812B)

| Signal | GPIO |
|---|---|
| Data in | GP7 |

30 LEDs total, 3 per footswitch. **Chain order** (matters because the
PIO driver streams them in physical order):

```
[ 1 ][ 2 ][ 3 ][ 4 ][ up ][ A ][ B ][ C ][ D ][ down ]
 0–2  3–5  6–8  9–11 12–14 15–17 18–20 21–23 24–26 27–29
```

### ST7789 240×240 TFT (SPI1)

| Signal | GPIO |
|---|---|
| SCK | GP14 |
| MOSI | GP15 |
| CS | GP13 |
| DC | GP12 |
| Backlight (PWM) | GP8 |

Panel is physically mounted inverted in the chassis. **Verified on
hardware:** under `mipidsi` 0.10 the upright, centred result is
`Rotation::Deg0` + `display_offset(0, 0)` — *not* the CircuitPython
recipe (`rotation=180` + `rowstart=80`). mipidsi keeps the offset
constant across rotation, so the naive CP translation (`Deg180` +
offset 80) renders inverted with an 80-row stale band. SPI baud: 24 MHz.
Note: the case bezel obscures the outermost ~1–2px ring — keep a small
safe margin in UI layouts.

### DIN MIDI (UART0 alt 2)

| Signal | GPIO |
|---|---|
| TX (to DIN OUT)  | GP16 |
| RX (from DIN IN) | GP17 |

Baud: 31250. Standard MIDI current-loop circuit on the OEM board
(220 Ω current limit, 6N138 / equivalent opto-isolator on RX).

### Expression pedals (TRS, tip = wiper)

| Signal | GPIO | ADC channel |
|---|---|---|
| Expression 1 | GP27 | ADC1 |
| Expression 2 | GP28 | ADC2 |
| Battery voltage (optional) | GP29 | ADC3 |

ADC reference is the internal 3.3 V rail.

### Reserved / unused

GP4, GP5, GP6, GP21, GP22, GP26. Available for future expansion.

## USB identity

- **VID**: `0x2E8A` (Raspberry Pi vendor ID — KEEP THIS, see below)
- **PID**: `0x102D` (development; revisit before shipping)
- **Manufacturer**: "Paint Audio"
- **Product**: "MIDICaptain Remedy (Rust)"

The VID is load-bearing: `../scripts/bootsel_hammer.py` matches devices
by VID `0x2E8A` to know when to issue the 1200-baud BOOTSEL touch.
Change the VID and the recovery workflow breaks. Change the PID freely.

## SWD debug pads — VERIFIED

**Verified on hardware 2026-05-30** with a Raspberry Pi Debug Probe +
`probe-rs`: the RP2040 enumerates over SWD (DPv2, Designer "Raspberry Pi
Trading Ltd", Part `0x1002`), with **both** Cortex-M0+ cores visible as
multidrop instances `0x00`/`0x01`, each exposing a MemoryAP and the ARM
ROM Table at `0xe00ff000`.

No public teardown labels these pads. The two RP2040 reverse-engineering
projects ([nicola-lunghi/hiper-midicaptain](https://github.com/nicola-lunghi/hiper-midicaptain),
[paulhamsh/PaintaudioMidiCaptain](https://github.com/paulhamsh/PaintaudioMidiCaptain))
and the Kemper "PySwitch" community map GPIO/peripherals but not the
debug interface — so this map was established first-hand.

### Location (bottom / solder side)

The board is solid black solder-mask with **no white silkscreen**, but
the copper pad shapes are legible. Both headers are **plated
through-holes on the bottom side** — directly accessible once the chassis
bottom slides off; no board extraction needed.

- **SWD header — 3-pad inline group, board centre** (through-holes
  opposite the top-side RP2040). Pad shapes `■ ● ●`: one **square**
  (pin 1) + two round.
- **BOOTSEL jumper — 2-pad group `■ ●` just below/right.** Round pad is
  GND; **short the two at power-on → UF2 recovery.** Independent of the
  firmware's Switch-1-hold path — keep it as a hardware recovery option.
- There is also a 4-pad strip nearer the top edge (likely a UART/serial
  header — GND/TX/RX/3V3). Not characterised; not needed for SWD.

### Pinout (continuity + probe handshake confirmed)

| Pad (3-group)    | Signal | RPi Debug Probe "D" |
|---|---|---|
| **square** (pin 1) | **SWCLK** | SC |
| **middle**         | **GND**   | GND |
| **far**            | **SWDIO** | SD |

GND on the middle pad confirmed by continuity to the DC-jack sleeve / USB
shell. SWCLK↔SWDIO confirmed by the chip responding — a swap yields
"target did not respond", so the successful enumeration proves the
orientation.

### Wiring & power

- The 3-pin debug connector carries **NO VBUS**. Power the MIDI Captain
  from its own USB; the probe powers from the host. **Common GND only —
  do not bridge VBUS.**
- The target does **not** need bootloader mode for SWD — a normally
  running RP2040 answers the debug port.

### Bring-up gotchas (each cost time; recorded so they don't recur)

1. **Probe firmware ≥ 2.2.0 required.** A factory-fresh Debug Probe
   shipped with older firmware; `probe-rs` refused with *"firmware on the
   probe is outdated … minimum supported … 2.2.0."* Fix: hold the
   probe's **BOOTSEL**, plug in (mounts `RPI-RP2`), drop the latest
   `debugprobe.uf2` from
   <https://github.com/raspberrypi/debugprobe/releases>.
2. **"JTAG protocol could not be selected" is normal.** The Debug Probe
   is **SWD-only**; `probe-rs` tries JTAG first, fails, falls through to
   SWD. Ignore that line.
3. **Flipped probe PCB in the housing.** On at least one unit the
   internal board was rotated, so the case's **"D" (debug) and "U"
   (UART) labels were swapped** — cabling to the labelled "D" port
   actually drove UART and the target stayed silent ("did not respond").
   If wiring checks out but nothing answers, **try the other port.**
4. **Marginal contact.** Dupont pins resting in bare through-holes are
   flaky; `--speed 100` (100 kHz) helps, or tack thin wires for
   reliability.

### Enabling the probe-rs runner

[`.cargo/config.toml`](.cargo/config.toml) ships **UF2 as the default
runner** (not every contributor has a probe — leave it the default).
To use the probe locally: comment the `elf2uf2-rs -d` runner line and
uncomment `probe-rs run --chip RP2040`. Then
`cargo run --release --example display_widgets` flashes **and** streams
`defmt`/RTT live — far faster than the UF2 reflash loop. The UF2 /
1200-baud-touch path remains available as a fallback.

## Memory layout

```
0x10000000  BOOT2     0x100 bytes   (rp2040-boot2 stage 2, supplied by embassy-rp)
0x10000100  FLASH     2 MB − 0x100  (firmware)
0x20000000  SRAM      264 KB
```

See [`memory.x`](memory.x).
