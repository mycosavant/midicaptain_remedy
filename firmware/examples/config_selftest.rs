//! `config_selftest` — proof binary for the runtime config model + persistence.
//!
//! Two layers, both `defmt::assert!`ed on real silicon (or it panics on the
//! first failure):
//!
//! 1. **RAM round-trip** — `RuntimeConfig` → postcard blob → `RuntimeConfig`,
//!    for both the baked default and a hand-tweaked config; the result must
//!    equal the input (proves the serde model is lossless).
//! 2. **Flash round-trip** — store a tweaked config, read it back (must match),
//!    then `factory_reset` and confirm the load falls back to the default.
//!
//! This validates the foundation the webapp config-sync builds on, without any
//! transport or hardware beyond the board itself.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use midicaptain_firmware::config::{self, Action, CcValue, RuntimeConfig};
use midicaptain_firmware::events::HidReport;
use midicaptain_firmware::storage::{self, Storage};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

/// The default config with a page name, a button label, and a button action
/// changed — something that must survive a round-trip unchanged.
fn sample_config() -> RuntimeConfig {
    let mut cfg = RuntimeConfig::default_config();
    let mut name = config::PageName::new();
    let _ = name.push_str("MYPAGE");
    cfg.pages[0].name = name;
    let mut label = config::Label::new();
    let _ = label.push_str("HELLO");
    cfg.pages[0].buttons[0].label = label;
    cfg.pages[0].buttons[0].on_press = Action::MidiCc {
        cc: 42,
        value: CcValue::Fixed(99),
    };
    // Exercise the Trigger CC mode (self-toggling devices) through the round-trip.
    cfg.pages[0].buttons[3].on_press = Action::MidiCc {
        cc: 43,
        value: CcValue::Trigger(127),
    };
    // Exercise the per-page continuous bindings (encoder + expr) — all three
    // `ContinuousBinding` variants (None / Sysex / Sysex; MidiCc is covered by
    // the baked default pages) — through the round-trip.
    cfg.pages[0].encoder = config::ContinuousBinding::Sysex(config::ContinuousSysex::Volume);
    cfg.pages[0].expr = [
        config::ContinuousBinding::None,
        config::ContinuousBinding::Sysex(config::ContinuousSysex::Wah),
    ];
    // Exercise the TapTempo action (payload-less) through the round-trip.
    cfg.pages[0].buttons[4].on_press = Action::TapTempo;
    // Exercise the radio-group field (Tier 3) through the round-trip too.
    cfg.pages[0].buttons[1].group = config::MAX_GROUPS as u8;
    // Exercise the HID action variant (Tier 5): a Ctrl+Shift keystroke and a
    // consumer/media usage, so both `HidReport` shapes round-trip.
    cfg.pages[0].buttons[1].on_long_press = Action::Hid(HidReport::Key {
        keycode: 0x1D, // Z
        modifiers: 0x03, // Ctrl | Shift
    });
    cfg.pages[0].buttons[2].on_long_press =
        Action::Hid(HidReport::Consumer { usage: 0x00CD }); // Play/Pause
    cfg
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("config_selftest: boot");
    let p = embassy_rp::init(Default::default());

    static SCRATCH: StaticCell<[u8; storage::CONFIG_SCRATCH_LEN]> = StaticCell::new();
    let scratch = SCRATCH.init([0; storage::CONFIG_SCRATCH_LEN]);

    let default = RuntimeConfig::default_config();
    let sample = sample_config();
    defmt::assert!(sample != default, "sample must differ from default");

    // ── 1. RAM round-trips ─────────────────────────────────────────────
    {
        let blob = defmt::unwrap!(config::serialize(&default, scratch));
        info!("  default blob: {=usize} bytes", blob.len());
        let got = defmt::unwrap!(config::deserialize(blob));
        defmt::assert!(got == default, "default round-trip mismatch");
    }
    {
        let blob = defmt::unwrap!(config::serialize(&sample, scratch));
        info!("  sample blob: {=usize} bytes", blob.len());
        let got = defmt::unwrap!(config::deserialize(blob));
        defmt::assert!(got == sample, "sample round-trip mismatch");
    }
    info!("config self-test: RAM round-trip OK");

    // ── 1b. uses_katana_sysex detects SysEx via *any* binding ──────────────
    {
        // Default has SysEx buttons (the Katana page) → detected.
        defmt::assert!(default.uses_katana_sysex(), "default should use katana sysex");

        // Strip every SysEx *button* but keep the SysEx encoder/expr bindings:
        // detection must still fire via the continuous-binding path (the fix —
        // a config whose only Katana use is a SysEx volume knob).
        let mut cont_only = default.clone();
        for page in cont_only.pages.iter_mut() {
            for b in page.buttons.iter_mut() {
                if matches!(b.on_press, Action::Sysex(_)) {
                    b.on_press = Action::None;
                }
                if matches!(b.on_long_press, Action::Sysex(_)) {
                    b.on_long_press = Action::None;
                }
            }
        }
        defmt::assert!(
            cont_only.uses_katana_sysex(),
            "continuous SysEx binding must trigger katana detection"
        );

        // Now also drop the continuous SysEx bindings → no SysEx anywhere (the
        // baked cycles are CC-only) → not a Katana setup.
        let mut none = cont_only.clone();
        for page in none.pages.iter_mut() {
            page.encoder = config::ContinuousBinding::MidiCc(7);
            page.expr = [
                config::ContinuousBinding::MidiCc(1),
                config::ContinuousBinding::MidiCc(7),
            ];
        }
        defmt::assert!(
            !none.uses_katana_sysex(),
            "pure-CC config must not trigger katana detection"
        );
        info!("config self-test: uses_katana_sysex OK");
    }

    // ── 2. Flash round-trip ────────────────────────────────────────────
    let mut storage = Storage::new(p.FLASH);
    defmt::unwrap!(storage.store_config(&sample, scratch).await);
    let loaded = storage.load_config(scratch).await;
    defmt::assert!(loaded == sample, "flash load != stored sample");
    info!("config self-test: flash persist OK");

    // factory_reset wipes the region → load returns the baked default.
    defmt::unwrap!(storage.factory_reset().await);
    let after = storage.load_config(scratch).await;
    defmt::assert!(after == default, "post-reset load != default");
    info!("config self-test: default fallback OK");

    info!("config self-test: ALL PASS");

    let mut beat = Ticker::every(Duration::from_secs(5));
    loop {
        beat.next().await;
        info!("config self-test: ALL PASS (idle)");
    }
}
