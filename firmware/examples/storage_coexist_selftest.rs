//! `storage_coexist_selftest` — regression guard for the "config push wipes
//! saved settings" bug.
//!
//! ## What broke
//!
//! The scalar settings (MIDI channel, brightness, pedal calibration) and the
//! user-config blob share ONE `sequential-storage` key-value map. That map is
//! log-structured, so reading *any* scalar key scans every item in the active
//! page through the accessor's scratch buffer. Once a ~2 KB config blob lived in
//! the map, a scalar read with the old 128-byte buffer faulted with
//! `BufferTooSmall`; the firmware caught the error and silently fell back to the
//! default — so pushing a config wiped the user's saved MIDI channel /
//! brightness / pedal calibration. (`store_item`'s garbage collection migrates
//! items through the same buffer, so even a scalar *write* could fault.)
//!
//! ## What this proves
//!
//! It replays that exact sequence on real flash and asserts it no longer
//! happens:
//!   1. `factory_reset` → clean store.
//!   2. store distinctive `Settings`; read back (sanity, no blob present yet).
//!   3. store a config blob — the event that used to corrupt the scalar reads.
//!   4. read the `Settings` back — they must be **unchanged** (the regression
//!      check; before the fix this returned defaults).
//!   5. write another scalar *after* the blob exists (exercises the GC-migrate
//!      path, which also scans the blob through the buffer) and read it back.
//!   6. confirm the config blob itself also survived.
//!   7. validate [`config::MAX_SERIALIZED_LEN`] really is an upper bound, then
//!      round-trip a worst-case (max-pages, max-length) config — the hardest
//!      case for the scalar scan buffer — and re-check the settings beside it.
//!
//! Any failure panics through `panic-probe`; a clean run logs "ALL PASS".
//!
//! NOTE: like the other storage examples this writes only to the dedicated
//! CONFIG flash region, which `memory.x` keeps disjoint from the firmware image.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use heapless::Vec;
use midicaptain_firmware::config::{self, Action, CcValue, OwnedButton, OwnedPage, RuntimeConfig};
use midicaptain_firmware::storage::{self, PedalCal, Settings, Storage};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

/// Settings distinct from every field's default, so a silent revert-to-default
/// is unmistakable.
const WRITTEN: Settings = Settings {
    midi_channel: 7,
    display_brightness: 55,
    led_brightness: 30,
    pedal_cal: [
        PedalCal { min: 1234, max: 60000 },
        PedalCal { min: 200, max: 51000 },
    ],
};

/// A realistic non-default config. Even the two-page default already serializes
/// to a few hundred bytes — far larger than the old 128-byte scalar buffer — so
/// storing it is enough to trip the original bug.
fn sample_config() -> RuntimeConfig {
    let mut cfg = RuntimeConfig::default_config();
    let mut label = config::Label::new();
    let _ = label.push_str("COEXIST");
    cfg.pages[0].buttons[0].label = label;
    cfg.pages[0].buttons[0].on_press = Action::MidiCc {
        cc: 42,
        value: CcValue::Fixed(99),
    };
    cfg
}

/// Fill a button label to its full [`config::LABEL_CAP`].
fn fill_label() -> config::Label {
    let mut s = config::Label::new();
    while s.push('X').is_ok() {}
    s
}

/// Fill a page name to its full [`config::NAME_CAP`].
fn fill_name() -> config::PageName {
    let mut s = config::PageName::new();
    while s.push('X').is_ok() {}
    s
}

/// Worst-case config: [`config::MAX_PAGES`] pages, every name/label at its cap,
/// the largest `Action` variant on both press slots — the biggest blob the model
/// can produce, hence the hardest case for the settings store's scan buffer.
fn max_config() -> RuntimeConfig {
    let button = OwnedButton {
        label: fill_label(),
        color: config::color::WHITE,
        on_press: Action::MidiCc {
            cc: 127,
            value: CcValue::Fixed(127),
        },
        on_long_press: Action::MidiCc {
            cc: 127,
            value: CcValue::Fixed(127),
        },
        group: config::MAX_GROUPS as u8,
    };
    let page = OwnedPage {
        name: fill_name(),
        buttons: core::array::from_fn(|_| button.clone()),
    };
    let mut pages: Vec<OwnedPage, { config::MAX_PAGES }> = Vec::new();
    for _ in 0..config::MAX_PAGES {
        let _ = pages.push(page.clone());
    }
    RuntimeConfig {
        pages,
        midi_thru: config::ThruRoutes::NONE,
    }
}

/// Load every setting and assert it equals `want`, or panic with `ctx`.
async fn assert_settings(storage: &mut Storage, want: &Settings, ctx: &str) {
    let got = storage.load().await;
    defmt::assert!(
        got == *want,
        "{=str}: settings changed (got {}, want {})",
        ctx,
        got,
        want
    );
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("storage_coexist_selftest: boot");
    let p = embassy_rp::init(Default::default());

    static SCRATCH: StaticCell<[u8; storage::CONFIG_SCRATCH_LEN]> = StaticCell::new();
    let scratch = SCRATCH.init([0; storage::CONFIG_SCRATCH_LEN]);

    let mut storage = Storage::new(p.FLASH);

    // 1. Clean slate so the run is deterministic regardless of prior state.
    defmt::unwrap!(storage.factory_reset().await);

    // 2. Store distinctive settings and read them back — no blob present yet, so
    //    this passed even before the fix; it's a baseline.
    defmt::unwrap!(storage.store(&WRITTEN).await);
    assert_settings(&mut storage, &WRITTEN, "pre-config").await;
    info!("  settings stored + read back (pre-config) OK");

    // 3. Store a config blob — the event that used to corrupt the scalar reads.
    let sample = sample_config();
    let blob_len = {
        let blob = defmt::unwrap!(config::serialize(&sample, scratch));
        blob.len()
    };
    defmt::assert!(
        blob_len > 128,
        "sample blob ({=usize} B) must exceed the old 128 B buffer to be a valid regression",
        blob_len
    );
    defmt::unwrap!(storage.store_config(&sample, scratch).await);
    info!("  config stored ({=usize} bytes)", blob_len);

    // 4. THE REGRESSION CHECK: settings must survive the config store. Before the
    //    fix, these reads faulted with BufferTooSmall and returned defaults.
    assert_settings(&mut storage, &WRITTEN, "after config store").await;
    info!("  settings SURVIVED config store -- regression guard OK");

    // 5. Write another scalar with the blob present. This is the store/GC path:
    //    migrating the live blob to a fresh page reads it through the same buffer.
    defmt::unwrap!(storage.set_midi_channel(11).await);
    let mut want2 = WRITTEN;
    want2.midi_channel = 11;
    assert_settings(&mut storage, &want2, "after post-config scalar write").await;
    info!("  scalar write after config OK (GC-migrate path)");

    // 6. The config blob must be intact after the scalar churn around it.
    let loaded = storage.load_config(scratch).await;
    defmt::assert!(loaded == sample, "config blob corrupted by scalar writes");
    info!("  config blob intact after scalar writes OK");

    // 7. Bound validation + worst-case round-trip — the largest blob the model
    //    can emit, stored beside the scalars, must not break either.
    let maxc = max_config();
    let max_len = {
        let blob = defmt::unwrap!(config::serialize(&maxc, scratch));
        blob.len()
    };
    info!(
        "  max config blob: {=usize} bytes (bound {=usize})",
        max_len,
        config::MAX_SERIALIZED_LEN
    );
    defmt::assert!(
        max_len <= config::MAX_SERIALIZED_LEN,
        "config::MAX_SERIALIZED_LEN ({=usize}) is not an upper bound (saw {=usize})",
        config::MAX_SERIALIZED_LEN,
        max_len
    );
    defmt::unwrap!(storage.store_config(&maxc, scratch).await);
    // Worst-case blob now in the map → scalar reads must STILL succeed.
    assert_settings(&mut storage, &want2, "after max-config store").await;
    let loaded_max = storage.load_config(scratch).await;
    defmt::assert!(loaded_max == maxc, "max config round-trip mismatch");
    info!("  worst-case config + settings coexist OK");

    // Clean up after ourselves. This test PERSISTS a worst-case 8-page config
    // (every label "XXXX", every button cc127/group=8) plus test settings. If
    // left in flash they poison the next boot of the real firmware, which would
    // load this garbage config instead of the baked default (XXXX labels, no
    // page nav, identical pages). Erase the whole store so the device is left
    // at defaults — same hygiene config_selftest applies after its checks.
    defmt::unwrap!(storage.factory_reset().await);
    let after = storage.load_config(scratch).await;
    defmt::assert!(
        after == RuntimeConfig::default_config(),
        "post-cleanup load != baked default"
    );
    info!("  store erased -- device left at baked default (cleanup) OK");

    info!("storage_coexist_selftest: ALL PASS");

    // Idle heartbeat so the RTT session stays attached and the pass is obvious
    // (a failure would have panicked above through panic-probe instead).
    let mut beat = Ticker::every(Duration::from_secs(5));
    loop {
        beat.next().await;
        info!("storage_coexist_selftest: ALL PASS (idle)");
    }
}
