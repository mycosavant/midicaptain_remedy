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

Display mounted upside-down: rotate 180° in software when initializing
the `mipidsi` driver. SPI baud target: 24 MHz.

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

## SWD debug pads

**TBD** — locate from the reverse-engineering docs in this repo (not
checked in to this branch yet). When found, document here as:

```
SWCLK pad: <location on PCB, e.g. "top-side, U1 pin 24 via 2.54 mm test point near power input">
SWDIO pad: <location>
GND pad:   <location>
RUN/RESET: <location, optional but useful for hard reset>
```

Until that's known, only the UF2 / 1200-baud-touch flash path works.
That path is fine for active iteration — it just doesn't give us
breakpoints or live RTT log streaming.

### Pi Debug Probe wiring (for reference)

Pinout once the SWD pads are identified:

| Probe | Target |
|---|---|
| GND | GND |
| SC (SWCLK) | SWCLK pad |
| SD (SWDIO) | SWDIO pad |

Probe powers itself off the host USB; the MIDI Captain stays powered
from its own USB cable. Don't bridge VBUS between them.

## Memory layout

```
0x10000000  BOOT2     0x100 bytes   (rp2040-boot2 stage 2, supplied by embassy-rp)
0x10000100  FLASH     2 MB − 0x100  (firmware)
0x20000000  SRAM      264 KB
```

See [`memory.x`](memory.x).
