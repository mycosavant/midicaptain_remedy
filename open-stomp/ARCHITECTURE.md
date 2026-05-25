# Open Stomp — Architecture & Research Document

## 1. Vision

A DIY, open-source guitar effects processor and MIDI foot controller that
rivals commercial units (MOD Dwarf, Neural DSP Quad Cortex, Tonex Pedal)
while remaining fully hackable and extensible.

**Core capabilities:**
- Neural amp modeling (load .nam and .aidax models)
- Stereo impulse response / cabinet simulation
- Full LV2 plugin effects chain (100s of available plugins)
- Programmable MIDI control (USB-C, DIN, Bluetooth)
- Web-based pedalboard designer (accessible from phone/laptop over WiFi)
- Physical controls: footswitches, expression pedals, encoder, LEDs
- Display(s) showing current preset, signal chain, tuner, etc.
- Setlist / preset management for live performance

**Design principles:**
- Open source everything (hardware designs, firmware, software)
- Built from off-the-shelf components where possible
- Hackable — users can modify any layer of the stack
- Prototype-first — prove the concept, then refine the hardware
- Leverage existing open-source projects rather than reinventing


## 2. Prior Art & Open-Source Ecosystem

### 2.1 Commercial Products

| Product | Price | Key Features | Limitations |
|---------|-------|-------------|-------------|
| MOD Dwarf | ~$500 | LV2 plugins, web UI, MIDI, WiFi | Dual A53 CPU (limited for heavy NAM models) |
| Neural DSP Quad Cortex | ~$1900 | Neural captures, touchscreen | Closed source, expensive |
| Tonex Pedal | ~$400 | Neural tone models | Limited effects, closed ecosystem |
| Line 6 HX Stomp | ~$600 | Full modeler + effects | Closed source |
| Sonulab Stompstation | ~$600 | Pi-based, NAM, plugins | Closed source software |

### 2.2 Open-Source Projects to Build On

**Audio Processing:**
- **AIDA-X** — Neural amp modeler, LV2 plugin, designed for embedded ARM.
  Runs on MOD Dwarf. Loads .aidax and .nam models. Uses RTNeural for
  inference. https://github.com/AidaDSP/AIDA-X
- **RTNeural** — Real-time neural network inference library optimized for
  audio. Used by AIDA-X and others. C++ with ARM NEON optimizations.
  https://github.com/jatinchowdhury18/RTNeural
- **GuitarML / NeuralPi** — Proved neural amp modeling on Raspberry Pi 4.
  https://github.com/GuitarML/NeuralPi
- **Guitarix** — Open-source virtual guitar amplifier for Linux. Amp models,
  cabinet sims, and effects. LV2 plugin versions available.
  https://guitarix.org/
- **NAM (Neural Amp Modeler)** — Steven Atkinson's neural amp modeling
  project. Training + inference. The .nam model format is widely used.
  https://github.com/sdatkinson/neural-amp-modeler

**Plugin Hosting & Audio Routing:**
- **mod-host** — LV2 plugin host from MOD. Lightweight, designed for
  embedded. Controllable via socket API.
  https://github.com/mod-audio/mod-host
- **mod-ui** — Web-based pedalboard designer from MOD. Drag-and-drop
  signal chain building. https://github.com/mod-audio/mod-ui
- **JACK** — Low-latency audio server. Industry standard for pro audio
  on Linux. Required by mod-host.
  https://jackaudio.org/
- **MODEP** — Full MOD software stack packaged for Raspberry Pi.
  https://blokas.io/modep/

**Hardware Platforms:**
- **Bela** — Ultra-low-latency audio platform based on BeagleBone.
  Sub-millisecond round-trip. Open source hardware + software.
  https://bela.io/
- **Zynthian** — Pi-based open-source synth/effects platform.
  https://zynthian.org/
- **PiStomp** — Pi-based guitar pedal, open hardware/software.
  https://github.com/TreeFallSound/pi-stomp
- **Elk Audio OS** — Ultra-low-latency Linux distro for audio.
  https://www.elk.audio/


## 3. Hardware Platform Comparison

### 3.1 Raspberry Pi 5 (Standard Board)

| Spec | Details |
|------|---------|
| CPU | Quad-core Arm Cortex-A76 @ 2.4GHz (64-bit) |
| RAM | 2GB / 4GB / 8GB / 16GB LPDDR4X |
| Storage | microSD (no eMMC) |
| Connectivity | WiFi 5, Bluetooth 5.0/BLE, GbE, 2x USB 3.0, 2x USB 2.0 |
| GPIO | 40-pin header (SPI, I2C, UART, PWM) |
| Display | 2x 4Kp60 HDMI, 2x 4-lane MIPI DSI |
| PCIe | 1x PCIe 2.0 (via M.2 HAT) |
| Power | USB-C, 5V/5A (27W PD) |
| Price | ~$60 (4GB) to ~$120 (16GB), kits ~$225 |

**Pros:** Massive ecosystem, best community support, most software
compatibility, powerful CPU, readily available, familiar platform.

**Cons:** No analog audio I/O (needs HAT), no real-time guarantees without
RT kernel, higher power draw, no onboard eMMC (SD card reliability concern),
boot time ~15-30s.

**Best for:** Prototype development, maximum software compatibility.

### 3.2 Raspberry Pi Compute Module 5 (CM5)

| Spec | Details |
|------|---------|
| CPU | Same as Pi 5: Quad-core Cortex-A76 @ 2.4GHz |
| RAM | 2GB / 4GB / 8GB LPDDR4X |
| Storage | None (Lite) / 16GB / 32GB / 64GB eMMC |
| Connectivity | Optional WiFi 5 + BT 5.0, 2x USB 3.0, GbE |
| Form Factor | 100-pin connector pair (same as CM4) |
| Carrier Board | Required — official CM5IO ($20) or third-party |
| Price | From $45 (2GB Lite) to ~$90 (8GB/32GB/WiFi) |
| Production | Guaranteed until at least January 2036 |

**Pros:** Same Pi 5 CPU in module form. eMMC for reliable storage (no SD
card corruption). Custom carrier board possible for final product. Compact.
Drop-in upgrade path from CM4.

**Cons:** Needs carrier board. More complex development setup. No 16GB
variant currently. Not as easy to prototype with as a full Pi 5.

**Best for:** Final product / boutique production. Design custom carrier
board with exactly the I/O needed.

### 3.3 BeagleBone AI-64

| Spec | Details |
|------|---------|
| SoC | TI TDA4VM (Jacinto) |
| CPU | Dual-core Arm Cortex-A72 @ 2GHz (64-bit) |
| DSP | C7x @ 1GHz (80 GFLOPS) + 2x C66x @ 1.35GHz (40 GFLOPS) |
| AI | 8-bit MMA deep learning accelerator (8 TOPS) |
| MCU | 6x Cortex-R5F cores (real-time capable) |
| GPU | PowerVR 8XE @ 750MHz |
| RAM | 4GB LPDDR4 |
| Storage | 16GB eMMC + microSD |
| Connectivity | 3x USB 3.0, GbE, M.2 E-key (PCIe + USB + SDIO) |
| Audio | 12 multichannel audio serial port modules on-chip |
| Price | ~$190-250 (availability varies) |

**Pros:** Dedicated DSP cores could offload neural network inference from
CPU entirely. 8 TOPS AI accelerator. Real-time R5F cores for deterministic
I/O. On-chip audio serial ports. Bela cape compatibility. More capable SoC
architecture overall.

**Cons:** Smaller ecosystem than Pi. Less community support. More expensive.
TDA4VM DSP programming is specialized (TI's SDK, OpenCL). Higher learning
curve. Availability has been spotty. Only 4GB RAM.

**Best for:** Maximum audio performance if the DSP toolchain investment is
worthwhile. A potential upgrade path after proving the concept on Pi 5.

### 3.4 Recommendation

**Prototype on Pi 5**, target **CM5 for production**.

The Pi 5 has the largest software ecosystem, the most community support, and
the most straightforward path to getting AIDA-X + mod-host + JACK running.
MODEP and NeuralPi have already proven the stack works on Pi hardware.

The CM5 is the natural path to a product — same software, module form factor,
eMMC storage, custom carrier board with exactly the connectors needed.

The BeagleBone AI-64 is interesting for its DSP and AI accelerator, but the
software ecosystem gap is significant. Worth revisiting if the Pi 5's CPU
can't handle the target neural models, but Pi 5's Cortex-A76 cores are
actually faster per-core than the AI-64's Cortex-A72 cores. The DSP would
only win if AIDA-X/RTNeural were ported to use TI's C7x — a major effort.

### 3.5 Bela + CTAG

The Bela platform with CTAG face2|4 cape offers remarkable audio I/O
(8 in / 16 out, <1ms round-trip). However:

- Based on BeagleBone Black/AI — less CPU power than Pi 5
- CTAG + Bela cape is expensive (~$200+ for the audio alone)
- Bela's ultra-low latency matters for synthesis and experimental audio;
  for guitar effects, 3-5ms round-trip (achievable on Pi 5) is indistinguishable
- Could be revisited as an audio I/O solution if paired with a more powerful
  compute board

**Verdict:** Impressive tech, but overkill for this application given the
cost and compute tradeoff.


## 4. MIDICaptain as Prototype GPIO Controller

A key insight: the existing MIDICaptain hardware can serve as the real-time
GPIO subsystem for the prototype, eliminating the need to design and wire
physical controls from scratch.

### 4.1 What the MIDICaptain Already Has

| Feature | Details | Reuse Potential |
|---------|---------|-----------------|
| 10 footswitches | Debounced, tested, working | Direct reuse |
| Encoder + pushbutton | With click detent | Direct reuse |
| 30 NeoPixels | 3 per switch, auto-mapped | Direct reuse |
| MIDI DIN I/O | UART at 31250 baud (GP16/GP17) | Direct reuse |
| USB | RP2040 native USB | Serial link to Pi |
| 2x 1/4" TRS jacks | Expression pedal inputs (ADC GP27/GP28) | Direct reuse as expression |
| ST7789 240x240 TFT | SPI display | Secondary status display |
| RP2040 MCU | 133MHz dual-core, 264KB RAM | Runs custom firmware |

### 4.2 What it CANNOT Do

- **Audio I/O**: The 1/4" jacks are wired to ADC pins for expression pedals
  (voltage divider circuit). They are not audio jacks — no codec, no DAC,
  no anti-aliasing filters, not balanced. Cannot be repurposed for audio.
- **High-speed data**: RP2040 USB is 12Mbps (USB 1.1/2.0 Full Speed).
  Fine for control data, not for audio streaming.

### 4.3 Prototype Architecture Using MIDICaptain

```
┌─────────────────────────────────────────────┐
│  Raspberry Pi 5                             │
│  ┌────────────────────────────────────────┐ │
│  │ Linux + RT kernel                      │ │
│  │ JACK → mod-host → AIDA-X + LV2 plugins│ │
│  │ Web UI (mod-ui) over WiFi              │ │
│  │ Control daemon (Python)                │ │
│  └───────┬─────────────────────┬──────────┘ │
│          │ I2S                 │ USB serial  │
│  ┌───────▼───────┐            │             │
│  │ Audio Codec   │            │             │
│  │ HAT           │            │             │
│  │ 1/4" In / Out │            │             │
│  └───────────────┘            │             │
│  ┌────────────────────┐       │             │
│  │ Main Display       │       │             │
│  │ (HDMI or DSI TFT)  │       │             │
│  └────────────────────┘       │             │
└───────────────────────────────┼─────────────┘
                                │
                    USB cable   │
                                │
┌───────────────────────────────▼─────────────┐
│  MIDICaptain (RP2040)                       │
│  ┌──────────────────────────────────────┐   │
│  │ CircuitPython firmware               │   │
│  │ - Footswitch scanning + debounce     │   │
│  │ - NeoPixel LED control               │   │
│  │ - Expression pedal ADC               │   │
│  │ - Encoder input                      │   │
│  │ - MIDI DIN I/O passthrough           │   │
│  │ - USB serial protocol to Pi          │   │
│  │ - ST7789 secondary display           │   │
│  └──────────────────────────────────────┘   │
│  Physical: 10 switches, 30 LEDs, 2 exp,    │
│            encoder, MIDI DIN, 240x240 TFT   │
└─────────────────────────────────────────────┘
```

### 4.4 Communication Protocol (USB Serial)

Simple text-based protocol between MIDICaptain and Pi:

```
MIDICaptain → Pi (events):
  BTN:A:press
  BTN:A:release
  BTN:up:long_press
  ENC:+3
  EXP:1:87
  MIDI_IN:<hex bytes>

Pi → MIDICaptain (commands):
  LED:A:00FF00         (set LED color)
  LED:A:00FF00:dim     (set LED dimmed)
  LED:ALL:000000       (clear all)
  DISP:title:CLEAN     (update display title)
  DISP:btn:A:BOOST     (update button label)
  MIDI_OUT:<hex bytes>  (send MIDI DIN)
```

### 4.5 Benefits of This Approach

- **Saves weeks of hardware work** — no designing switch matrix, LED wiring,
  expression pedal circuits, or MIDI I/O from scratch
- **Already tested** — the MIDICaptain Remedy firmware has been debugged on
  real hardware through iterative testing
- **Clean separation** — Pi handles compute/audio, RP2040 handles real-time
  GPIO. This is the same architecture used by commercial products
- **Incremental migration** — start with MIDICaptain as-is, later design
  a custom GPIO board if needed for production

### 4.6 Required Firmware Changes

The current MIDICaptain Remedy firmware would need a new operating mode:
"bridge mode" where it acts as a USB serial peripheral to the Pi rather than
running its own MIDI logic. This could be:

- A separate firmware image, OR
- A mode selected at boot (e.g., hold a specific button combination), OR
- Detected automatically when USB serial is connected to a host running
  the control daemon


## 5. Software Stack

### 5.1 Audio Layer

```
Guitar In ──► Audio Codec (I2S) ──► JACK ──► mod-host ──► JACK ──► Audio Codec ──► Amp Out
                                                │
                                      ┌─────────▼─────────┐
                                      │   LV2 Plugin Chain  │
                                      │                     │
                                      │  ┌───────────────┐  │
                                      │  │ Noise Gate     │  │
                                      │  └───────┬───────┘  │
                                      │  ┌───────▼───────┐  │
                                      │  │ AIDA-X (amp)   │  │
                                      │  └───────┬───────┘  │
                                      │  ┌───────▼───────┐  │
                                      │  │ IR Loader (cab)│  │
                                      │  └───────┬───────┘  │
                                      │  ┌───────▼───────┐  │
                                      │  │ Delay          │  │
                                      │  └───────┬───────┘  │
                                      │  ┌───────▼───────┐  │
                                      │  │ Reverb         │  │
                                      │  └───────────────┘  │
                                      └─────────────────────┘
```

**JACK** handles audio routing with low-latency configuration:
- Sample rate: 48kHz
- Buffer size: 64-128 frames
- Target latency: 3-5ms round-trip (1.3-2.7ms buffer + codec)

**mod-host** loads and connects LV2 plugins via a socket API:
```
# Example mod-host commands
add lv2_uri instance_number
connect effect_instance:output effect_instance:input
param_set instance_number param_index value
bypass instance_number bypass_state
```

### 5.2 Plugin Ecosystem

| Category | LV2 Plugins Available |
|----------|----------------------|
| Amp Modeling | AIDA-X, Guitarix amps, GxAmps |
| Cabinet / IR | AIDA-X (built-in), HiFi LoFi, Calf plugins |
| Overdrive/Distortion | Guitarix drives, Calf, ZamTube |
| Delay | Calf Vintage Delay, MDA, Guitarix delays |
| Reverb | Calf, DragonflyReverb, MDA, Guitarix |
| Modulation | Calf Flanger/Phaser/Chorus, MDA, Guitarix |
| Dynamics | Calf Compressor/Gate, ZamComp, LSP |
| EQ | Calf Parametric EQ, LSP, Guitarix |
| Looper | SooperLooper, ALO |
| Tuner | GxTuner (internal, no MIDI needed) |
| Utility | Calf Mono/Stereo, gain, mixer |

### 5.3 Control Layer

Python daemon running on the Pi that bridges everything:

```
┌─────────────────────────────────────────┐
│  Control Daemon (Python)                │
│                                         │
│  ┌─────────────┐  ┌─────────────────┐   │
│  │ USB Serial   │  │ mod-host socket │   │
│  │ (MIDICaptain)│  │ (plugin control)│   │
│  └──────┬──────┘  └───────┬─────────┘   │
│         │                 │             │
│  ┌──────▼─────────────────▼──────────┐  │
│  │ Preset Manager                    │  │
│  │ - Load/save pedalboard presets    │  │
│  │ - Map footswitches to actions     │  │
│  │ - Expression pedal → param CC     │  │
│  │ - Setlist management              │  │
│  │ - LED state management            │  │
│  └──────┬────────────────────────────┘  │
│         │                               │
│  ┌──────▼──────┐  ┌─────────────────┐   │
│  │ MIDI engine  │  │ Web UI server   │   │
│  │ (USB + BLE)  │  │ (mod-ui / Flask)│   │
│  └─────────────┘  └─────────────────┘   │
└─────────────────────────────────────────┘
```

### 5.4 Tuner (Solved Differently)

With audio processing on-board, the tuner problem from MIDICaptain Remedy
disappears. Instead of relying on the amp to send MIDI tuner data:

- Use **GxTuner** (LV2 plugin) — does pitch detection directly from the
  guitar audio signal already flowing through JACK
- Display the result on the Pi's screen
- No external device cooperation needed
- Works with any amp or direct-to-PA setup

This is a fundamental advantage of having audio I/O on the device.


## 6. Display Strategy

### 6.1 Options

| Approach | Pros | Cons |
|----------|------|------|
| Single 5-7" HDMI/DSI touchscreen | Simple wiring, large UI, touch editing | Single point of info, bulky |
| Single 3.5" DSI TFT + MIDICaptain 240x240 | Two displays, Pi main + MC status | Small main display |
| Multiple small OLEDs per switch | Per-switch labels (like Quad Cortex) | Complex wiring, SPI mux needed |
| 5" main + MIDICaptain secondary | Best of both — large main, switch status on MC | Two display drivers |

### 6.2 Recommendation for Prototype

**5" DSI touchscreen on Pi** (main UI — preset name, signal chain, tuner) +
**MIDICaptain's existing 240x240 TFT** (switch status, button labels, LED
complement).

The Pi drives the main display natively. The MIDICaptain drives its own
display as it already does. No additional hardware needed for prototype.


## 7. Prototype BOM

### 7.1 Phase 1 — Audio Proof of Concept (Desk Setup)

| Component | Source | Est. Cost |
|-----------|--------|-----------|
| Raspberry Pi 5 (8GB) kit w/ SD, PSU, case | Amazon | ~$120-150 |
| HiFiBerry DAC+ADC Pro (or Fe-Pi Audio Z V2) | HiFiBerry / Amazon | $45-65 |
| **Subtotal** | | **~$165-215** |

This gets audio in → processing → audio out working on a desk.

### 7.2 Phase 2 — Add Physical Controls

| Component | Source | Est. Cost |
|-----------|--------|-----------|
| MIDICaptain (already owned) | On hand | $0 |
| USB-A to USB-C cable | On hand / Amazon | $5 |
| **Subtotal** | | **~$5** |

Wire MIDICaptain to Pi via USB. Write bridge firmware + control daemon.

### 7.3 Phase 3 — Display + Enclosure

| Component | Source | Est. Cost |
|-----------|--------|-----------|
| 5" DSI touchscreen | Amazon / Pi Hut | $40-60 |
| Enclosure + hardware | Amazon / Hammond | $30-60 |
| 1/4" panel-mount jacks (4x) | Amazon / Mouser | $10-15 |
| Standoffs, screws, misc | Amazon | $10 |
| **Subtotal** | | **~$90-145** |

### 7.4 Total Prototype Cost

| Phase | Cost |
|-------|------|
| Phase 1 (audio PoC) | ~$165-215 |
| Phase 2 (controls) | ~$5 |
| Phase 3 (display + enclosure) | ~$90-145 |
| **Total** | **~$260-365** |

Well under the $500 target, with room for a Pi 5 16GB upgrade if needed.


## 8. Development Plan

### Phase 1 — Audio Proof of Concept

**Goal:** Guitar signal in, processed signal out, under 10ms latency.

- [ ] Acquire Pi 5 + audio HAT
- [ ] Install Raspberry Pi OS Lite (64-bit)
- [ ] Build/install RT-patched kernel (PREEMPT_RT)
- [ ] Install JACK, configure for low-latency (48kHz, 64-128 frames)
- [ ] Install mod-host, verify LV2 plugin loading
- [ ] Build/install AIDA-X LV2 plugin from source for aarch64
- [ ] Load a .nam model, run guitar through it
- [ ] Measure round-trip latency (JACK latency tools)
- [ ] Test with various plugin chains (amp + cab + delay + reverb)
- [ ] Document CPU usage under different plugin loads
- [ ] Install GxTuner, verify pitch detection works

### Phase 2 — Physical Controls via MIDICaptain

**Goal:** Footswitches control the plugin chain, LEDs reflect state.

- [ ] Write MIDICaptain "bridge mode" firmware (USB serial protocol)
- [ ] Write Pi-side control daemon (Python, asyncio)
- [ ] Implement USB serial communication protocol
- [ ] Map footswitches to mod-host actions (bypass, preset switch)
- [ ] Map expression pedals to plugin parameters
- [ ] LED feedback for bypass state (on/off colors)
- [ ] MIDI DIN passthrough (MIDICaptain ↔ external gear)

### Phase 3 — Display + Web UI

**Goal:** Visual feedback on device, remote configuration via WiFi.

- [ ] Set up main display (DSI TFT or HDMI)
- [ ] Display current preset / plugin chain info
- [ ] Tuner display (GxTuner output)
- [ ] Install mod-ui or build custom web UI
- [ ] WiFi access point mode for configuration
- [ ] Preset save/load from web UI
- [ ] MIDICaptain secondary display (switch labels/status)

### Phase 4 — Integration + Enclosure

**Goal:** Everything in a giggable enclosure.

- [ ] Enclosure layout design (footswitch spacing, display cutout, jacks)
- [ ] Power supply integration (5V/5A for Pi)
- [ ] Internal wiring (audio jacks → HAT, USB, power)
- [ ] Boot optimization (target <10s to audio-ready)
- [ ] Read-only root filesystem (prevent SD corruption on power loss)
- [ ] Stress testing (extended play sessions, heat monitoring)

### Phase 5 — Advanced Features + Production Path

**Goal:** Feature parity with commercial units, production readiness.

- [ ] Bluetooth MIDI (Pi 5 built-in BT + bluez-alsa or similar)
- [ ] USB-C MIDI host/device (via Pi USB)
- [ ] Setlist management with per-song presets
- [ ] Multiple display support (if desired)
- [ ] Snapshot / scene system within a preset
- [ ] CM5 migration (custom carrier board design)
- [ ] Custom PCB for GPIO (replace MIDICaptain with integrated solution)
- [ ] Enclosure manufacturing (CNC aluminum or custom mold)


## 9. Open Questions

1. **Audio HAT selection** — HiFiBerry DAC+ADC Pro vs Fe-Pi Audio Z V2 vs
   others. Need to verify I2S compatibility with Pi 5 and driver support
   under RT kernel. The Fe-Pi is cheaper but less documented.

2. **AIDA-X on Pi 5** — Has anyone built AIDA-X for aarch64 Pi 5 yet?
   NeuralPi proved Pi 4 works. Pi 5 should be better, but need to verify
   build process and performance.

3. **Latency budget** — What's actually achievable? Theoretical minimum
   with 64-frame buffer at 48kHz is ~2.7ms (2 x 64/48000 + codec). Real
   world with plugins in the chain? Need to measure.

4. **Power consumption** — Pi 5 under audio DSP load. How hot does it get?
   Does it need active cooling in an enclosed pedal? Almost certainly yes.

5. **Boot time** — Can we get to audio-ready in under 10 seconds? Possible
   with systemd optimization, initramfs, and skip unnecessary services.
   Some people use suspend-to-RAM for instant wake.

6. **BeagleBone AI-64 as future path** — If the TDA4VM's C7x DSP could
   run RTNeural inference, it could handle heavier models than the Pi 5's
   CPU. But this requires porting RTNeural to TI's DSP toolchain. Worth
   investigating after the Pi 5 prototype proves the concept.

7. **MIDICaptain expression jack reuse** — The TRS jacks are wired to RP2040
   ADC pins with voltage dividers for expression pedals. Could they be
   rewired or used as control voltage inputs for other purposes? Not for
   audio (no codec), but potentially for CV or additional analog sensors.


## 10. References & Resources

### Hardware
- Raspberry Pi 5: https://www.raspberrypi.com/products/raspberry-pi-5/
- Raspberry Pi CM5: https://www.raspberrypi.com/products/compute-module-5/
- CM5 IO Board: https://www.raspberrypi.com/products/compute-module-5-io-board/
- BeagleBone AI-64: https://www.beagleboard.org/boards/beaglebone-ai-64
- Bela: https://bela.io/
- HiFiBerry DAC+ADC Pro: https://www.hifiberry.com/shop/boards/hifiberry-dac-adc-pro/

### Software
- AIDA-X: https://github.com/AidaDSP/AIDA-X
- RTNeural: https://github.com/jatinchowdhury18/RTNeural
- NeuralPi: https://github.com/GuitarML/NeuralPi
- mod-host: https://github.com/mod-audio/mod-host
- mod-ui: https://github.com/mod-audio/mod-ui
- MODEP: https://blokas.io/modep/
- Guitarix: https://guitarix.org/
- JACK: https://jackaudio.org/
- PiStomp: https://github.com/TreeFallSound/pi-stomp
- Zynthian: https://zynthian.org/
- Elk Audio OS: https://www.elk.audio/

### Documentation
- LV2 plugin standard: https://lv2plug.in/
- JACK API: https://jackaudio.org/api/
- Pi 5 RT kernel: https://github.com/raspberrypi/linux (rpi-6.x.y branch)
- Jeff Geerling CM5 review: https://www.jeffgeerling.com/blog/2024/raspberry-pi-cm5-2-3x-faster-drop-upgrade-mostly
