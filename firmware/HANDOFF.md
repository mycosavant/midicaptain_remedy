# Handoff — next session: ORCHESTRATION + full-scope planning

This handoff is different from the previous ones. The display stack is
hardware-validated and the application skeleton exists, so the port has
crossed from "prove the toolchain" into "build out subsystems." The next
session's job is **planning and orchestration**: lay out the remaining
trajectory, then drive it — parallel subagents / parallel sessions for
genuinely independent subsystems, serialised integration for the shared
router. This file hands you the map to start from.

Read `ARCHITECTURE.md` (task graph + channel rules) alongside this.

## Where things stand (all on PR #6, base `SAFE_main`)

**Hardware-validated on real silicon** (Pi Debug Probe + probe-rs):
- ST7789 display: geometry (`Deg0` + `display_offset(0,0)`), colour
  inversion ON (`ColorInversion::Inverted`), 24 MHz SPI. Splash + widgets
  render correctly, upright, centred, smooth.
- SWD debug access: 3-pad bottom-side header (square=SWCLK, mid=GND,
  far=SWDIO) — see `HARDWARE.md`. `probe-rs run` flashes + streams RTT.
- UI scene graph: `Widget` trait, const `Palette`, `ValueBar`
  (delta-paint, no flicker), `TextPanel`. Dirty-flag gating proven via
  `examples/display_widgets.rs`.
- **Application skeleton**: `src/bin/midicaptain.rs` —
  buttons → router → display over `embassy_sync` channels. Compiles
  clean; **not yet flashed** (built right before this handoff).

**Toolchain reality:**
- Build anywhere; **flash only from Windows PowerShell** (WSL can't see
  USB; probe-rs.exe + the probe live on Windows). `cargo run` uses the
  runner in `.cargo/config.toml`.
- The probe-rs runner flip in `.cargo/config.toml` is a **local,
  uncommitted** change (repo default stays UF2 so probe-less contributors
  aren't broken). Don't `git add` that file.

**First action for next session on hardware:** flash the app skeleton and
confirm the pipeline:
```powershell
cargo run --release --bin midicaptain
```
Press footswitches → the status panel should show `<name> x<count>` and
RTT should log `router: A pressed -> count=N` / `display: A xN`. That
validates the channel architecture before fanning out work onto it.

## The remaining trajectory (priority + parallelization)

The router in `bin/midicaptain.rs` is the **shared integration surface**.
Everything else is either an *independent module* that feeds the router
through a channel (parallelisable), or *integrative* work that edits the
router/app state (serialise it).

### Step 0 — Planning task (do this first, it unblocks parallelism)

Define the **channel contracts** in one place (suggest `src/events.rs` or
`src/app.rs`): the message enums every task codes against —
`ButtonEvent` (exists, in the bin), `EncoderEvent`, `ExprEvent`,
`MidiRx`, `MidiCmd`, `LedFrame`, `DisplayCmd` (exists). Freeze these
types first; then independent sessions can build against stable
interfaces without colliding. This is the highest-leverage 30 minutes in
the whole plan.

### Independent modules — safe to parallelise (new files, channel-driven)

Each is a new `src/<area>/*.rs` + a spawnable task + its event type. They
barely touch shared files, so they can run as concurrent subagents or
parallel sessions and merge cleanly. CP reference in parens.

| Module | New files | Feeds | CP reference | Notes |
|---|---|---|---|---|
| **WS2812 LEDs** | `src/hal/leds.rs` | consumes `LedFrame` | `remedy/lib/pins.py` LED_MAP, `hardware.py` brightness | `examples/blink.rs` already proves PIO/DMA; port per-switch RGB + idle dim + toggle states. 30 LEDs, 3/switch. |
| **MIDI engine** | `src/midi/{mux,sysex,katana}.rs` | `MidiRx`→router, consumes `MidiCmd` | `remedy/lib/midi.py` | `examples/midi_passthrough.rs` proves USB↔DIN. Add SysEx reassembly across USB-MIDI 4-byte CINs; Roland checksum + Katana model ID. The biggest single chunk. |
| **Encoder** | `src/hal/encoder.rs` | `EncoderEvent`→router | `remedy/lib/hardware.py` EncoderHandler | Quadrature on GP2/GP3, push GP0. GPIO-IRQ driven. |
| **Expression** | `src/hal/expression.rs` | `ExprEvent`→router | `remedy/lib/hardware.py`, `menu.py` calib | ADC on GP27/28. Needs calibration storage (stub first, wire to Storage later). |
| **Storage** | `src/storage.rs` | request/response | NVM layout in root `CLAUDE.md` | `sequential-storage` over `embassy_rp::flash`. Unblocks expression calibration + settings + config persistence. Foundational — worth doing early in parallel. |

### Integrative work — serialise (edits router / app state / display modes)

These change `bin/midicaptain.rs` (or `app.rs` once extracted) and/or add
display modes. One owner at a time to avoid churn on the shared file.

1. **Config + page system** (`remedy/lib/config.py`, `events.py`,
   `config/*.toml`). Defines what buttons *do* (CC/PC/SysEx/page-nav per
   button per page). This is the router's brain — promote the stub
   counter logic into real action dispatch. Start with a baked-in config
   (serde over a `&str`), move to flash-loaded later.
2. **Settings menu** (`remedy/lib/menu.py`). Encoder-driven; needs
   encoder + display + storage. A display "mode."
3. **Tuner** (`remedy/lib/tuner.py`). Big note glyph + cents needle;
   pitch via MIDI. A display "mode." Self-contained once MIDI-in exists.
4. **Device sync** (Katana RQ1 on boot → toggle LED states). Needs MIDI +
   LEDs + config.
5. **Webapp sync** (COBS+CRC16 over USB CDC). Later; rides the CDC class.

### Suggested orchestration shape

- **Wave 1 (parallel):** Storage, LEDs, Encoder, MIDI-engine — four
  independent module sessions/subagents against the frozen channel
  contracts. Each lands a `src/...` module + task + a tiny example or
  unit of proof. None edits the bin.
- **Wave 2 (serialise, you drive):** integrate Wave 1 tasks into the app
  binary; build the config/page system (the router's real logic).
- **Wave 3 (parallel again):** Settings menu, Tuner, Device sync — each a
  display mode / feature on top of the integrated base.
- **Cross-cutting:** promote the inline buttons/router/display tasks out
  of `bin/` into `src/hal/` + `src/app.rs` once two+ subsystems are in
  (don't pre-abstract — let it pull apart naturally).

A note on cost: parallel subagents editing the *same* files conflict.
Keep Wave-1 work in **new files** and the integration **serial**. Use
`isolation: "worktree"` for any agent that must touch shared files
concurrently.

## Invariants (do not violate)

- **Branch/PR:** target `SAFE_main`, **never `main`**. The repo is a
  **fork of `nicola-lunghi/hiper-midicaptain`** — `gh pr create` defaults
  the base to the *upstream*. `gh repo set-default mycosavant/midicaptain_remedy`
  is already set; still pass `-R mycosavant/midicaptain_remedy --base
  SAFE_main` explicitly and verify `isCrossRepository=false`. (See the
  `project-rust-port-branching` memory.)
- **Don't modify** `../remedy/`, `../webapp/`, `../MIDICAPTAIN_OEM_BACKUP/`,
  `../presets/`, root `../CLAUDE.md`. Firmware work stays in `firmware/`.
- **No USB MSC.** The device owns its flash exclusively.
- **Commit `Cargo.lock`.** Don't commit the `.cargo/config.toml` runner
  flip.
- **Green gate before pushing:**
  `cargo build --release --bins --examples` and
  `cargo clippy --release --bins --examples -- -D warnings` (note
  `--bins` — there's a binary now).

## Gotchas still live

- embassy-executor 0.10 spawn idiom: `spawner.spawn(task(args).unwrap())`
  (the `#[task]` call yields a Result; unwrap to the token). No
  `must_spawn`.
- mipidsi 0.10 ≠ CircuitPython conventions: offset is constant across
  rotation; this panel needs colour inversion ON. (Documented in
  `display.rs` header — don't "fix" it back to the CP numbers.)
- Channels need a `RawMutex`: `CriticalSectionRawMutex` +
  `static CH: Channel<...> = Channel::new()` (const). See the bin.
- Older display-driver / bootstrap gotchas: `git log -p firmware/HANDOFF.md`.

## Meta

Previous handoffs encoded forward-looking knowledge here and let code +
commits carry the backward-looking detail. This one widens that: it's a
*planning seed* for an orchestrator, not a single-task brief. Keep that
framing if you re-hand-off — give the next driver the dependency map and
the parallel/serial split, not a linear to-do list.

Good luck — the foundation is solid and validated; it's build-out from here.
