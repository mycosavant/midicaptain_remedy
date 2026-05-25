# Architecture — Rust + Embassy firmware

The CircuitPython implementation in [`../remedy/`](../remedy/) is built
around one giant polling loop in `main.py`: read every input, dispatch
events, draw the display, sleep, repeat. That's idiomatic for CP and it
works for a 10-switch foot controller, but it bakes worst-case latency
into every subsystem. A slow display redraw delays button detection;
expression-pedal smoothing burns CPU even when nothing is touching the
pedal.

The Rust port discards that loop entirely. Embassy gives us cooperative
async tasks driven by hardware interrupts, plus typed compile-time
ownership of every peripheral. So:

- Each subsystem runs as its own task, woken only when something
  actually happens (button edge, UART RX, USB packet, encoder click).
- Tasks communicate through `embassy_sync::channel::Channel<…, RAW, N>`
  — bounded MPSC queues with explicit back-pressure semantics.
- No shared mutable state. The few things that ARE shared (e.g. current
  page index, settings) live in single-owner state machines that the
  rest of the system reads through query messages.

This file is the contract for future sessions. If you're adding a
subsystem, place it on the task graph below and document its channel(s).

## Project layout

```
firmware/
├── Cargo.toml
├── memory.x                ← RP2040 flash + RAM layout
├── build.rs                ← stages memory.x, links cortex-m-rt + boot2
├── rust-toolchain.toml     ← pins stable + thumbv6m-none-eabi
├── .cargo/config.toml      ← dual runner (UF2 default, probe-rs alt)
├── src/
│   ├── lib.rs              ← `pub mod pins`
│   └── pins.rs             ← board pin map (GPIO numbers, NeoPixel chain order, USB IDs)
├── examples/               ← runnable PoC binaries (this session)
│   ├── blink.rs
│   ├── serial_echo.rs
│   └── midi_passthrough.rs
├── README.md               ← build/flash quickstart
├── ARCHITECTURE.md         ← this file
└── HARDWARE.md             ← pin map, SWD pad location (TBD)
```

Future modules (rough plan, lands one per follow-up session):

```
src/
├── lib.rs
├── pins.rs                 ← (today)
├── hal/                    ← thin wrappers over embassy-rp peripherals
│   ├── buttons.rs          ← debounced edge detector → Channel<ButtonEvent>
│   ├── encoder.rs          ← quadrature decoder → Channel<EncoderEvent>
│   ├── leds.rs             ← per-switch RGB state → driven WS2812 frames
│   └── expression.rs       ← ADC + calibration → Channel<ExprEvent>
├── midi/
│   ├── mux.rs              ← USB + DIN combined I/O
│   ├── sysex.rs            ← parse / build streaming SysEx
│   └── katana.rs           ← Roland model-ID + helpers (port from remedy/lib/midi.py)
├── display/                ← mipidsi + embedded-graphics scene graph
├── config/                 ← serde-toml load from flash KV, or binary fmt
├── storage/                ← sequential-storage over embassy_rp::flash
├── sync/                   ← COBS+CRC16 wire protocol for webapp sync
├── app.rs                  ← top-level state machine wiring tasks
└── bin/
    └── midicaptain.rs      ← application binary (replaces examples/)
```

## Task graph (target)

```
                  ┌──────────────┐
   GPIO IRQs ────▶│ buttons task │──ButtonEvent──┐
                  └──────────────┘               │
                  ┌──────────────┐               │
   GPIO IRQs ────▶│ encoder task │──EncEvent─────┤
                  └──────────────┘               │
                  ┌──────────────┐               │
   ADC DMA ──────▶│ expr.   task │──ExprEvent────┤
                  └──────────────┘               │
                                                 ▼
                                         ┌───────────────┐
                                         │ event router  │
                                         │  (app state)  │
                                         └───┬───┬───┬───┘
                                MidiCmd  ◀───┘   │   └───▶ LedFrame
                                                 │            │
                                            DisplayCmd        │
                                                 ▼            ▼
                                         ┌───────────────┐  ┌───────────┐
                                         │ display task  │  │ leds task │
                                         │   (30 fps)    │  │ (WS2812)  │
                                         └───────────────┘  └───────────┘
                                                                          
   USB IRQ ──────▶┌──────────────┐                 ┌─────────────────┐
                  │ usb device   │◀────MidiCmd─────│ midi mux task   │──▶ UART0 TX
   USB MIDI ─────▶│   task       │──UsbMidiRx──────│                 │◀── UART0 RX
   USB CDC  ─────▶│              │──CdcRx──────────│                 │
                  └──────────────┘                 └─────────────────┘
                                                            │
                                                       (router input)
```

Concretely:

| Task | Wakes on | Sends | Receives |
|---|---|---|---|
| `buttons` | GPIO IRQ (all 10 footswitches + encoder push) | `ButtonEvent` → router | — |
| `encoder` | GPIO IRQ on quadrature edges | `EncoderEvent` → router | — |
| `expression` | ADC DMA done (period polled @ ~100 Hz) | `ExprEvent` → router | — |
| `usb_device` | USB CTRL IRQ | raw USB-MIDI / CDC bytes → mux | `MidiCmd`, `CdcResp` |
| `midi_mux` | UART RX IRQ or channel msg | router input | `MidiCmd` from router |
| `router` | Any input channel msg | `MidiCmd`, `DisplayCmd`, `LedFrame` | All event channels |
| `display` | `DisplayCmd` or 30 Hz ticker | SPI frames to ST7789 | `DisplayCmd` |
| `leds` | `LedFrame` | WS2812 DMA writes | `LedFrame` |
| `storage` | settings save/load requests | KV blobs | `StorageReq` |

Channel capacities are 8–16 in the PoC; tune per real-world load.

## Channel design rules

1. **Bounded, never unbounded.** Out-of-memory is a worse failure than
   dropping one MIDI event. All channels have a compile-time depth.
2. **Drop-newest on overflow** for time-sensitive events (MIDI, LED
   frames). Use `try_send` and ignore the error. The next event
   overwrites the lost one's intent anyway.
3. **Block-the-producer for state changes** (config writes, page
   navigation). Use `send().await`; the queue depth is set so this
   never deadlocks under realistic load.
4. **One owner per peripheral.** A task that holds `UART0` is the only
   thing that can talk to UART0. To send DIN MIDI from elsewhere, send
   a `MidiCmd` to the mux task. This eliminates locking entirely.

## Why these crate choices

The PoC pulls in the minimum to prove the transports. Where alternatives
exist, here's why these:

- **embassy-rp** vs. `rp-hal`: embassy-rp is the only one with first-
  class async DMA + interrupt-driven USB + interrupt-driven UART out of
  the box. `rp-hal` is fine for sync code; we want async.
- **embassy-usb** vs. `usb-device`: embassy-usb is the only async USB
  stack that fits cleanly with embassy-executor. Has both `cdc_acm` and
  `midi` classes already; we don't reimplement either.
- **smart-leds** (used implicitly via PIO WS2812 driver): standard color
  type. The PIO program ships in embassy-rp under
  `pio_programs::ws2812`.
- **mipidsi** for the ST7789 (planned, not in this PoC): widely adopted,
  works with `embedded-graphics`, supports the 180° rotation we need.
- **sequential-storage** for flash KV (planned): purpose-built for
  flash wear-leveling. Replaces the CP NVM hack for expression pedal
  calibration; same store can hold all settings.

Notably absent: **no USB MSC**. This is intentional. The whole point of
the rewrite is that the device owns its flash exclusively — no host /
device write races that could corrupt the filesystem. The webapp sync
protocol (next-but-one session) rides on USB CDC instead.

## Decisions that may need revisiting

- **Examples-only library crate.** Today there's no `src/bin/main.rs`.
  The first non-PoC session should add `src/bin/midicaptain.rs` (the
  real application) and demote the examples to "transport tests."
- **All-defmt logging, no panic redirect.** If we never get a probe,
  defmt-rtt is just bytes shouted into the void during UF2 boot. The
  USB CDC logger task is a reasonable backup; bring it in only when
  needed because it inflates the binary by ~40 KB.
- **`portable-atomic` with critical-section.** Required by
  `static_cell` on Cortex-M0+. If we move to RP2350 (Cortex-M33) later,
  this dependency becomes superfluous.
- **USB PID is 0x102D**, picked unilaterally. If we ever ship publicly,
  apply for a real PID (or use a sub-VID partner range).
- **Channel capacities (8 / 16).** Educated guess. Instrument with
  `defmt` once on hardware and adjust.
- **No SysEx in `midi_passthrough` PoC.** Real implementation must
  handle SysEx fragmentation across USB-MIDI 4-byte packets (CIN
  0x4/0x5/0x6/0x7). Owns the next-session MIDI mux task.

## Read this if you're starting the next session

1. `firmware/src/pins.rs` is the single source of truth for the board
   layout. If a peripheral comes up missing, check there first.
2. `examples/` is for transport sanity-checks. Don't pile features into
   them — add a new module under `src/` instead.
3. The CP firmware in `../remedy/lib/` is the behavioural reference for
   anything user-visible (LED brightness curves, encoder acceleration,
   tuner pitch detection, Katana SysEx addresses). Read it first, port
   the behaviour, do not copy the polling architecture.
4. `cargo build --examples --release` and
   `cargo clippy --examples -- -D warnings` should always pass. If
   they don't, fix that before pushing.
