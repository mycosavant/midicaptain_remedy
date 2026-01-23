# MIDICaptain Remedy - Project Roadmap

An open, extensible MIDI controller firmware platform for the Paint Audio MIDI Captain.

## Vision

Replace proprietary footswitch solutions (like BOSS GA-FC-EX) with a highly customizable,
configuration-driven firmware that gives users complete control over their MIDI gear without
writing code. Support deep integration with specific devices (starting with BOSS Katana) while
remaining generic enough for any MIDI-capable gear.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     USER CONFIGURATION                           │
│  /config/                                                        │
│    global.toml           - MIDI channel, display, LED settings   │
│    expression.toml       - Pedal curves, ranges, mappings        │
│    /profiles/                                                    │
│      katana.toml         - BOSS Katana SysEx/CC definitions      │
│      neural-dsp.toml     - Neural DSP plugin mappings            │
│      generic-cc.toml     - Simple CC/PC mode                     │
│    /pages/                                                       │
│      default.toml        - Default button layout                 │
│      katana-live.toml    - Katana performance page               │
│      daw-control.toml    - DAW/plugin control page               │
│    /setlists/                                                    │
│      my_setlist.toml     - Song list with page overrides         │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                       CORE ENGINE                                │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │ Config       │  │ Event        │  │ State        │           │
│  │ Manager      │  │ Dispatcher   │  │ Manager      │           │
│  │              │  │              │  │              │           │
│  │ - TOML parse │  │ - Input events│ │ - Parameter  │           │
│  │ - Validation │  │ - MIDI in    │  │   values     │           │
│  │ - Hot reload │  │ - Timers     │  │ - Page state │           │
│  └──────────────┘  └──────────────┘  └──────────────┘           │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │ Action       │  │ Profile      │  │ Expression   │           │
│  │ Executor     │  │ Engine       │  │ Processor    │           │
│  │              │  │              │  │              │           │
│  │ - Send MIDI  │  │ - Device-    │  │ - Curves     │           │
│  │ - Page nav   │  │   specific   │  │ - Scaling    │           │
│  │ - LED/display│  │   handlers   │  │ - Splitting  │           │
│  └──────────────┘  └──────────────┘  └──────────────┘           │
└─────────────────────────────────────────────────────────────────┘
                                │
          ┌─────────────────────┼─────────────────────┐
          ▼                     ▼                     ▼
┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐
│ DEVICE DRIVERS  │   │   UI RENDERER   │   │  HARDWARE I/O   │
│                 │   │                 │   │                 │
│ - Katana SysEx  │   │ - Widget system │   │ - 10 buttons    │
│ - Generic MIDI  │   │ - Layout engine │   │ - 30 RGB LEDs   │
│ - USB HID       │   │ - Animations    │   │ - 240x240 TFT   │
│                 │   │ - Tuner view    │   │ - Encoder       │
│                 │   │                 │   │ - 2x expression │
└─────────────────┘   └─────────────────┘   └─────────────────┘
```

---

## Phase 1: Foundation (Core Framework)

### 1.1 Project Structure & Build System
- [ ] Create clean project structure separating:
  - `/src/` - Main firmware code
  - `/lib/` - Reusable modules
  - `/config/` - Default configuration files
  - `/profiles/` - Device profile definitions
- [ ] Set up development workflow (copy to device, serial debugging)
- [ ] Document CircuitPython version requirements and dependencies

### 1.2 Configuration System
- [ ] Implement TOML parser integration (use `tomli` or similar)
- [ ] Design configuration schema with validation
- [ ] Implement `global.toml` for system-wide settings:
  ```toml
  [midi]
  channel = 1
  usb_enabled = true
  din_enabled = true

  [display]
  brightness = 80
  timeout = 0  # 0 = never off
  rotation = 180

  [leds]
  brightness = 50
  idle_mode = "dim"  # dim, off, rainbow
  ```
- [ ] Implement hot-reload detection (optional, for development)

### 1.3 Hardware Abstraction Layer
- [ ] Create clean interfaces for:
  - Button input (with debouncing, press/release/long-press/double-tap)
  - Rotary encoder (with acceleration)
  - Expression pedals (with deadzone, calibration)
  - NeoPixel LEDs (with color palette system)
  - Display (with basic drawing primitives)
  - MIDI I/O (USB and DIN)
- [ ] Implement async event loop for responsive I/O

### 1.4 Event System
- [ ] Design event types:
  ```python
  # Input events
  ButtonPress(button_id, press_type)  # press_type: tap, long, double
  EncoderTurn(direction, velocity)
  EncoderPress(press_type)
  ExpressionChange(pedal_id, value)  # 0-127

  # MIDI events (incoming)
  MidiCC(channel, cc, value)
  MidiPC(channel, program)
  MidiSysEx(data)
  MidiClock()

  # System events
  PageChange(page_id)
  ProfileLoad(profile_id)
  ```
- [ ] Implement event dispatcher with handler registration

---

## Phase 2: Action System & Basic MIDI

### 2.1 Action Definitions
- [ ] Define action types in TOML:
  ```toml
  [[actions]]
  type = "midi_cc"
  channel = 1
  cc = 16
  value = 127  # or "toggle" or "expression"

  [[actions]]
  type = "midi_pc"
  channel = 1
  program = 5

  [[actions]]
  type = "page_change"
  page = "effects"

  [[actions]]
  type = "sysex"
  template = "katana_param"
  address = [0x60, 0x00, 0x04, 0x20]
  value = 3
  ```
- [ ] Implement action executor with queuing

### 2.2 Button Mapping
- [ ] Implement page-based button mapping:
  ```toml
  # /config/pages/default.toml
  [page]
  name = "Main"
  profile = "katana"

  [buttons.A]
  label = "BOOST"
  color = "green"
  on_press = { type = "midi_cc", cc = 16, value = "toggle" }
  led_feedback = { type = "midi_cc", cc = 16 }  # LED follows CC value

  [buttons.B]
  label = "MOD"
  color = "blue"
  on_press = { type = "midi_cc", cc = 17, value = "toggle" }

  [buttons.up]
  label = "PAGE+"
  on_press = { type = "page_next" }

  [buttons.down]
  label = "PAGE-"
  on_press = { type = "page_prev" }
  ```

### 2.3 Basic MIDI Implementation
- [ ] USB-MIDI send/receive
- [ ] DIN-MIDI send/receive (UART at 31250 baud)
- [ ] CC, PC, Note On/Off message handling
- [ ] SysEx send/receive with checksum calculation
- [ ] MIDI clock receive and BPM detection

### 2.4 LED Feedback System
- [ ] Color palette definition:
  ```toml
  [colors]
  off = [0, 0, 0]
  red = [255, 0, 0]
  green = [0, 255, 0]
  blue = [0, 0, 255]
  amber = [255, 128, 0]
  # ... etc
  ```
- [ ] LED-to-MIDI binding (LED reflects parameter state)
- [ ] Animation support (pulse, flash, fade)
- [ ] Per-button LED control (3 pixels per button)

---

## Phase 3: Display System

### 3.1 Widget Framework
- [ ] Implement base widget class with:
  - Position, size
  - Visibility
  - Value binding (to state)
  - Draw method
- [ ] Create core widgets:
  - Label (text with font/color/alignment)
  - Value display (parameter with units)
  - Progress bar (horizontal/vertical/arc)
  - Icon
  - Button indicator (matches physical button layout)

### 3.2 Layout Engine
- [ ] Define layouts in TOML:
  ```toml
  # /config/layouts/katana.toml
  [layout]
  name = "Katana Control"
  background = "black"

  [[widgets]]
  type = "label"
  x = 0
  y = 0
  width = 240
  height = 40
  text = "{preset_name}"
  font = "large"
  align = "center"

  [[widgets]]
  type = "row"
  y = 50
  items = [
    { type = "value", label = "GAIN", bind = "katana.gain", color = "amber" },
    { type = "value", label = "VOL", bind = "katana.volume", color = "green" },
  ]

  [[widgets]]
  type = "button_grid"
  y = 180
  rows = 2
  cols = 4
  # Reflects physical button states/labels
  ```
- [ ] Implement layout manager with page transitions

### 3.3 Tuner Mode
- [ ] Port tuner display from HKAudio firmware
- [ ] Note detection via incoming MIDI (pitch + pitch bend)
- [ ] Visual cents display with center indicator
- [ ] Auto-activate on tuner CC trigger

### 3.4 Font System
- [ ] Include multiple font sizes (small, medium, large)
- [ ] Support for custom BDF fonts
- [ ] Efficient text rendering with caching

---

## Phase 4: Device Profiles

### 4.1 Profile System Architecture
- [ ] Design profile structure:
  ```toml
  # /profiles/katana.toml
  [profile]
  name = "BOSS Katana"
  manufacturer = "Roland"
  model_id = [0x00, 0x00, 0x00, 0x33]

  [sysex]
  prefix = [0xF0, 0x41, 0x00, 0x00, 0x00, 0x00, 0x33]
  query_op = 0x11
  set_op = 0x12
  suffix = [0xF7]
  checksum = "roland"  # checksum algorithm

  [parameters.amp_type]
  name = "Amp Type"
  address = [0x00, 0x00, 0x04, 0x20]
  type = "enum"
  values = ["Acoustic", "Clean", "Crunch", "Lead", "Brown"]

  [parameters.gain]
  name = "Gain"
  address = [0x00, 0x00, 0x04, 0x21]
  type = "range"
  min = 0
  max = 100
  display = "{value}%"

  # ... hundreds more parameters from SysEx docs
  ```

### 4.2 BOSS Katana Profile
- [ ] Convert KAT-100_PARAMS.md to TOML profile
- [ ] Implement BTS (BOSS Tone Studio) mode commands:
  - Enter/exit BTS mode
  - Preset save/recall
  - Parameter query/set
- [ ] Bidirectional sync (query amp state on connect)
- [ ] Preset name display
- [ ] Parameter feedback (amp -> pedal display)

### 4.3 Generic CC Profile
- [ ] Simple CC/PC mapping for any device
- [ ] Learn mode (capture incoming MIDI to auto-map)
- [ ] USB HID mode for DAW control (keyboard shortcuts)

### 4.4 Neural DSP Profile (Example)
- [ ] Document Neural DSP MIDI implementation
- [ ] Create profile for common Archetype plugins
- [ ] Scene/snapshot switching
- [ ] Parameter control

---

## Phase 5: Expression Pedal Processing

### 5.1 Calibration & Configuration
- [ ] Auto-calibration routine (min/max detection)
- [ ] Deadzone configuration
- [ ] Save calibration to config file:
  ```toml
  [expression.pedal1]
  min = 120
  max = 65400
  deadzone = 5
  curve = "linear"  # or "log", "exp", "scurve"
  ```

### 5.2 Curve Processing
- [ ] Implement response curves:
  - Linear
  - Logarithmic (audio taper)
  - Exponential
  - S-curve (soft ends)
  - Custom (lookup table)
- [ ] Visual curve editor on display (using encoder)

### 5.3 Advanced Mapping
- [ ] Multi-target mapping (one pedal -> multiple CCs)
- [ ] Range limiting (min/max output)
- [ ] Split mode (lower half = CC1, upper half = CC2)
- [ ] Crossfade mode (pedal crossfades between two values)
- [ ] Wah-style auto-engage (value != 0 enables effect)

---

## Phase 6: Setlist & Scene Management

### 6.1 Setlist Structure
- [ ] Define setlist format:
  ```toml
  # /config/setlists/gig_20240115.toml
  [setlist]
  name = "Friday Gig"
  default_page = "katana-live"

  [[songs]]
  name = "Song One"
  page = "song1"  # optional page override
  on_enter = [
    { type = "midi_pc", program = 1 },
    { type = "sysex", template = "katana_preset", preset = "Ch1" }
  ]
  notes = "Start with clean, switch to drive at chorus"

  [[songs]]
  name = "Song Two"
  on_enter = [{ type = "midi_pc", program = 5 }]
  ```

### 6.2 Setlist Navigation
- [ ] Up/down buttons cycle through songs
- [ ] Display shows current/next song
- [ ] Auto-send MIDI on song change
- [ ] Optional confirmation before switching

### 6.3 Scene System
- [ ] Scenes are "snapshots" of multiple parameters
- [ ] One-button recall of complex states
- [ ] Scene copy/paste/edit via on-device menu

---

## Phase 7: On-Device Configuration

### 7.1 Menu System
- [ ] Encoder-driven menu navigation
- [ ] Menu structure:
  ```
  Settings
  ├── MIDI
  │   ├── Channel: 1-16
  │   ├── USB: On/Off
  │   └── DIN: On/Off
  ├── Display
  │   ├── Brightness: 0-100%
  │   └── Timeout: 0-60s
  ├── LEDs
  │   ├── Brightness: 0-100%
  │   └── Idle Mode: Dim/Off/Rainbow
  ├── Expression
  │   ├── Pedal 1: Calibrate
  │   └── Pedal 2: Calibrate
  ├── Profile
  │   └── [List of profiles]
  └── About
  ```

### 7.2 MIDI Learn Mode
- [ ] Press and hold button to enter learn mode
- [ ] Wiggle controller on target device
- [ ] Capture CC and auto-assign
- [ ] Save to current page config

### 7.3 Backup/Restore
- [ ] Export config to USB drive
- [ ] Import config from USB drive
- [ ] Factory reset option

---

## Phase 8: Advanced Features

### 8.1 MIDI Clock & Tap Tempo
- [ ] Receive MIDI clock, calculate BPM
- [ ] Display BPM on screen
- [ ] Tap tempo footswitch (outputs clock or tempo CC)
- [ ] LED pulse to beat

### 8.2 Looper Integration
- [ ] Specialized looper control page
- [ ] Visual loop state (recording, playing, overdub)
- [ ] Time display
- [ ] Integration with your Rust looper project

### 8.3 Macro Sequencer
- [ ] Record button press sequences
- [ ] Play back as one-button macro
- [ ] Timed sequences (delay between actions)

### 8.4 USB HID Mode
- [ ] Keyboard shortcut output (DAW transport, etc.)
- [ ] Consumer control (play/pause/volume)
- [ ] Simultaneous MIDI + HID

---

## Phase 9: Testing & Documentation

### 9.1 Testing
- [ ] Unit tests for config parsing
- [ ] Hardware-in-loop testing scripts
- [ ] Profile validation tests

### 9.2 User Documentation
- [ ] Getting started guide
- [ ] Configuration reference
- [ ] Profile creation guide
- [ ] Troubleshooting guide

### 9.3 Developer Documentation
- [ ] Architecture overview
- [ ] Adding new device profiles
- [ ] Contributing guide

---

## Milestones

### MVP (Minimum Viable Product)
- Core framework with TOML config
- Basic button -> MIDI CC mapping
- LED feedback
- Simple display showing button labels
- Works as generic CC controller

### Katana Alpha
- Full Katana SysEx support
- Bidirectional parameter sync
- Preset switching
- Basic tuner mode

### v1.0 Release
- Stable Katana profile
- Generic CC profile
- Expression pedal support with curves
- Setlist management
- On-device configuration menu
- User documentation

### Future
- Additional device profiles (Kemper, HX Stomp, etc.)
- Community profile repository
- Plugin/extension system
- Wireless module support (if applicable)

---

## Technical Decisions

### Why TOML?
- Human readable and writable
- Supports comments (unlike JSON)
- Less error-prone than YAML (no significant whitespace)
- Good Python library support
- Hierarchical structure for complex configs

### Why CircuitPython?
- Already proven on this hardware (OEM uses it)
- Rich library ecosystem (Adafruit)
- Easy development cycle (edit file, reboot)
- Accessible to beginners for customization
- Good async support for responsive I/O

### Memory Considerations
- RP2040 has 264KB SRAM - config files should be loaded incrementally
- Use generators/iterators where possible
- Compile .py to .mpy for smaller footprint
- Lazy-load profiles (only active profile in memory)

---

## Resources

- [Katana SysEx Specification](./KATANA/KAT-100_PARAMS.md) - Complete parameter map
- [Katana MIDI Control Guide](./KATANA/BOSS-KATANA_GEN3_MIDI_CONTROL.md) - Community resources
- [Hardware Reverse Engineering](../reveng.md) - GPIO pins, display, etc.
- [HKAudio Firmware](../../HKAudio_firmware/) - Reference implementation
- [OEM Firmware Backup](../../MIDICAPTAIN_OEM_BACKUP/) - Original Paint Audio code

---

## Contributing

This is an open source project. Contributions welcome:
- Device profiles for other gear
- Bug fixes and improvements
- Documentation
- Testing and feedback

---

*Last updated: 2024-01-21*
