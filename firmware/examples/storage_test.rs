//! `storage_test` — proves the flash key-value settings store round-trips.
//!
//! Loads the current settings (defaults on a blank device), writes a
//! distinctive value to every setting kind, reads them back, and asserts the
//! values survived the trip through real flash. All steps are logged over
//! defmt RTT, so with a probe attached (`cargo run --example storage_test`
//! once the probe-rs runner is enabled) you see:
//!
//! ```text
//! storage_test: boot
//! loaded (defaults on first run): Settings { midi_channel: 1, ... }
//! stored: Settings { midi_channel: 7, ... }
//! reloaded: Settings { midi_channel: 7, ... }
//! storage_test: round-trip OK
//! ```
//!
//! On the UF2 flash path (no probe) the asserts still run on-device; a
//! failure panics through `panic-probe`. Re-running the binary a second time
//! exercises the "value already present" path — the first load then reports
//! the previously written values instead of defaults, which is itself a
//! persistence check across reboots.
//!
//! NOTE: this writes to the dedicated CONFIG flash region only
//! (`storage::CONFIG_REGION_START..CONFIG_REGION_END`), which `memory.x`
//! keeps disjoint from the firmware image — running it cannot corrupt code.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use midicaptain_firmware::storage::{PedalCal, Settings, Storage};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("storage_test: boot");
    let p = embassy_rp::init(Default::default());

    // The store owns the FLASH peripheral; no DMA channel needed (the
    // blocking->async shim in storage.rs keeps it off the DMA subsystem).
    let mut storage = Storage::new(p.FLASH);

    // 1. What's currently persisted (factory defaults on a blank device,
    //    or the values from a previous run of this binary).
    let before = storage.load().await;
    info!("loaded (defaults on first run): {}", before);

    // 2. Write a distinctive value to every setting kind.
    let written = Settings {
        midi_channel: 7,
        display_brightness: 55,
        led_brightness: 30,
        pedal_cal: [
            PedalCal { min: 1234, max: 60000 },
            PedalCal { min: 200, max: 51000 },
        ],
    };
    match storage.store(&written).await {
        Ok(()) => info!("stored: {}", written),
        Err(e) => defmt::panic!("store failed: {}", e),
    }

    // 3. Read everything back and assert it survived the round-trip.
    let after = storage.load().await;
    info!("reloaded: {}", after);

    defmt::assert_eq!(after.midi_channel, written.midi_channel);
    defmt::assert_eq!(after.display_brightness, written.display_brightness);
    defmt::assert_eq!(after.led_brightness, written.led_brightness);
    defmt::assert_eq!(after.pedal_cal[0].min, written.pedal_cal[0].min);
    defmt::assert_eq!(after.pedal_cal[0].max, written.pedal_cal[0].max);
    defmt::assert_eq!(after.pedal_cal[1].min, written.pedal_cal[1].min);
    defmt::assert_eq!(after.pedal_cal[1].max, written.pedal_cal[1].max);

    info!("storage_test: round-trip OK");

    // Idle heartbeat so the RTT session stays attached and it's obvious the
    // asserts passed (a failure would have panicked above instead).
    loop {
        Timer::after(Duration::from_secs(5)).await;
        info!("storage_test: idle (round-trip passed)");
    }
}
