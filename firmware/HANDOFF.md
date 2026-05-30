# Handoff — next session: ORCHESTRATION + full-scope planning

This handoff is different from the previous ones. The display stack is
hardware-validated and the application skeleton exists, so the port has
crossed from "prove the toolchain" into "build out subsystems." The next
session's job is **planning and orchestration**: lay out the remaining
trajectory, then drive it — parallel subagents / parallel sessions for
genuinely independent subsystems, serialised integration for the shared
router. This file hands you the map to start from.

Read `ARCHITECTURE.md` (task graph + channel rules) alongside this.

## Where things stand (Wave 1 merged to `SAFE_main` @ `d7c626a`)

**Wave 1 is complete in module form** (PRs #6–#11, all merged to
`SAFE_main`). The channel contract `src/events.rs` is frozen and on
`SAFE_main`; the orchestration hiccup where it briefly missed the branch
has been fully reconciled (single definition per type, green gate clean).
Landed since the original handoff: the `events` contract, the HAL tasks
(`hal/{encoder,expression,leds}`), the MIDI engine
(`midi/{mux,sysex,katana}`), and the flash settings store (`storage`) —
each with a proof example. **None of these are wired into the router yet**
— that's Wave 2 (below).

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

## Trajectory

The router in `bin/midicaptain.rs` is the **shared integration surface**.
Everything else is either an *independent module* that feeds the router
through a channel (parallelisable), or *integrative* work that edits the
router/app state (serialise it).

### ✅ Wave 1 — DONE (merged to `SAFE_main`)

Step 0 (freeze `src/events.rs` channel contracts) and the five independent
modules all landed, each with a proof example, gate green:

| Module | Files | Status |
|---|---|---|
| Channel contracts | `src/events.rs` | ✅ frozen, single def per type |
| WS2812 LEDs | `src/hal/leds.rs` | ✅ `leds_task` consumes `LedFrame`; 10→30 px fan-out, `idle_dim` |
| MIDI engine | `src/midi/{mux,sysex,katana}.rs` | ✅ USB+DIN mux, streaming SysEx, Roland/Katana builders (byte-exact self-test) |
| Encoder | `src/hal/encoder.rs` | ✅ IRQ quadrature + accel + debounced push |
| Expression | `src/hal/expression.rs` | ✅ async-ADC pedals + calibration map + wizard |
| Storage | `src/storage.rs` | ✅ flash KV settings store (direct async accessor) |

None of these are wired into the router yet — that's Wave 2.

### ▶ Wave 2 — integration + config/page system (SERIAL, the driver owns the bin)

This is the current focus. Concrete build sequence:

**Phase A — prep (3 independent units; new/isolated code, parallelisable):**
1. `hal/encoder.rs`: add `encoder_task` + `EncoderChannel/Sender/Receiver`
   aliases (the module currently exposes only `Encoder`/`next_event`,
   unlike `expression`/`leds` which already ship a task + aliases).
2. `src/config/` (new): the action model — `Action` enum
   (`MidiCc{cc,value|toggle}`, `ProgramChange`, `Sysex`, `PageNext`,
   `PagePrev`, …), `ButtonConfig{label,color,on_press,on_long_press}`,
   `Page{name,buttons:[_;10]}`, `Config{pages}` + a **baked-in default
   `Config`** (Rust consts; flash-loaded TOML deferred). Ref:
   `remedy/lib/{config,events}.py`, `remedy/config/*.toml`.
3. `storage.rs`: fix the "16-bit ADC span" comment (RP2040 ADC is 12-bit)
   and document the `PedalCal → expression::Calibration` mapping.

**Phase B — integration spine (serial; edits the bin):** one `bind_interrupts!`
merging `USBCTRL_IRQ`/`UART0_IRQ`/`ADC_IRQ_FIFO`/`PIO0_IRQ_0`/`DMA_IRQ_0`;
construct + spawn LEDs (PioWs2812), encoder, expression (load calibration
from `Storage` on boot), MIDI (USB composite + `BufferedUart`, mux loops in
concrete wrapper tasks — embassy `#[task]`s can't be generic). Promote the
router to multi-input via a `RouterIn` merge enum + small per-source
forwarder tasks (one clean `receive().await` loop), holding app state
(page index, per-button toggles).

**Phase C — the router's brain (serial):** replace the stub press-counter
with action dispatch (`on_press`/`on_long_press` → `MidiCmd`/SysEx/page-nav);
page nav cycles pages + clears toggles; LED feedback builds an `LedFrame`
from page colours (full vs `idle_dim` per toggle); incoming `MidiRx` CC
syncs toggle state; grow `DisplayCmd` with page-mode variants (additive —
keep the router match exhaustive).

**Phase D — cross-cutting extract (once 2+ subsystems are wired):** lift the
inline footswitch task → `hal/buttons.rs`, the router/app state → `app.rs`;
the bin becomes thin wiring.

**Key decisions (chosen):** router fan-in = `RouterIn` enum + forwarders;
config v1 = baked-in Rust consts (no_std serde-TOML from flash is a later
research item — don't let it block Phase C); USB stood up composite-capable
(MidiClass now, room for CDC later).

### Wave 3 — display modes / features (parallelisable on the integrated base)

1. **Settings menu** (`remedy/lib/menu.py`) — encoder-driven; needs encoder
   + display + storage. A display "mode."
2. **Tuner** (`remedy/lib/tuner.py`) — big note glyph + cents needle; pitch
   via MIDI. A display "mode," self-contained once MIDI-in exists.
3. **Device sync** — Katana RQ1 on boot → toggle LED states. Needs MIDI +
   LEDs + config.
4. **Webapp sync** — COBS+CRC16 over USB CDC. Rides the (already composite)
   CDC class.

A note on cost: parallel subagents editing the *same* files conflict. Keep
new work in **new files** and integration **serial**. Use
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
