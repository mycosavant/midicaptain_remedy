//! `midicaptain` — the application binary.
//!
//! First real (non-PoC) binary for the port. It stands up the smallest
//! end-to-end slice of the task graph in `ARCHITECTURE.md`:
//!
//! ```text
//!   buttons task ──ButtonEvent──▶ router task ──DisplayCmd──▶ display task
//!   (poll+debounce)               (per-switch counters)        (TextPanel)
//! ```
//!
//! Each box is its own Embassy task; they communicate only through bounded
//! `embassy_sync` channels — no shared mutable state, one owner per
//! peripheral (the display task is the sole owner of the ST7789; the
//! buttons task is the sole owner of the footswitch GPIOs). This is the
//! skeleton the LED, MIDI, encoder and expression subsystems will plug
//! into: each becomes another task feeding the router.
//!
//! What it does on hardware: pressing any footswitch updates a text panel
//! on the display with the switch name and a per-switch press counter, and
//! logs the round-trip over RTT. That proves the channel plumbing before
//! we hang real behaviour off the router.
//!
//! Deliberately minimal for now — NOT yet wired: encoder, LEDs, MIDI,
//! expression pedals, page/config system. Those land per-subsequent
//! session, each as a task on the same router.

#![no_std]
#![no_main]

use core::fmt::Write as _;

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Output, Pull};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Ticker};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::prelude::*;
use heapless::String;
use midicaptain_firmware::display::{self, DisplayPeripherals, RemedyDisplay};
use midicaptain_firmware::events::{ButtonEvent, DisplayCmd};
use midicaptain_firmware::ui::{Palette, TextPanel, Widget};
use {defmt_rtt as _, panic_probe as _};

/// Footswitch count — the 10 chassis switches (encoder push handled later).
const SWITCH_COUNT: usize = 10;

/// Human-readable switch names, parallel to the GPIO order in `main`.
/// Index = the `ButtonEvent.index` the buttons task emits.
const SWITCH_NAMES: [&str; SWITCH_COUNT] =
    ["SW1", "SW2", "SW3", "SW4", "A", "B", "C", "D", "UP", "DOWN"];

/// Channel depths. Buttons are bursty (a stomp can bounce a few edges);
/// 16 absorbs that. Display commands are coalesced by the router, so 8 is
/// ample.
const BUTTON_Q: usize = 16;
const DISPLAY_Q: usize = 8;

/// Poll period for the debouncer. 5 ms × `SETTLE_SAMPLES` = settle time.
const POLL_MS: u64 = 5;
/// Consecutive stable samples required before a level change is accepted.
/// 3 × 5 ms = 15 ms — comfortably past contact bounce, well under human
/// perception.
const SETTLE_SAMPLES: u8 = 3;

// `ButtonEvent` and `DisplayCmd` now live in the shared channel-contract
// module (`midicaptain_firmware::events`) so parallel subsystem tasks code
// against the same types. See `src/events.rs`.

// Static channels live in `.bss`; `Channel::new()` is `const`. Senders and
// receivers are cloneable handles with `'static` lifetime, safe to hand to
// spawned tasks.
static BUTTON_CH: Channel<CriticalSectionRawMutex, ButtonEvent, BUTTON_Q> = Channel::new();
static DISPLAY_CH: Channel<CriticalSectionRawMutex, DisplayCmd, DISPLAY_Q> = Channel::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("MIDICaptain app: boot");
    let p = embassy_rp::init(Default::default());

    // Display task owns this for its whole life.
    let (disp, backlight) = display::init(DisplayPeripherals {
        spi:       p.SPI1,
        clk:       p.PIN_14,
        mosi:      p.PIN_15,
        cs:        p.PIN_13,
        dc:        p.PIN_12,
        backlight: p.PIN_8,
    })
    .expect("display init");

    // Footswitches: active-LOW with internal pull-ups. Order here defines
    // the `ButtonEvent.index` → `SWITCH_NAMES` mapping. Pins mirror
    // `pins.rs` / `HARDWARE.md`.
    let buttons: [Input<'static>; SWITCH_COUNT] = [
        Input::new(p.PIN_1, Pull::Up),  // SW1
        Input::new(p.PIN_25, Pull::Up), // SW2
        Input::new(p.PIN_24, Pull::Up), // SW3
        Input::new(p.PIN_23, Pull::Up), // SW4
        Input::new(p.PIN_9, Pull::Up),  // A
        Input::new(p.PIN_10, Pull::Up), // B
        Input::new(p.PIN_11, Pull::Up), // C
        Input::new(p.PIN_18, Pull::Up), // D
        Input::new(p.PIN_20, Pull::Up), // UP
        Input::new(p.PIN_19, Pull::Up), // DOWN
    ];

    // Spawn order is irrelevant — the channels decouple them. (0.10 spawn
    // idiom: the `#[task]` call yields a Result; unwrap to the token.)
    spawner.spawn(display_task(disp, backlight, DISPLAY_CH.receiver()).unwrap());
    spawner.spawn(router_task(BUTTON_CH.receiver(), DISPLAY_CH.sender()).unwrap());
    spawner.spawn(buttons_task(buttons, BUTTON_CH.sender()).unwrap());

    // Keep the main task alive with a low-rate liveness heartbeat. (If main
    // returns, the executor keeps running the spawned tasks anyway, but the
    // heartbeat is a cheap "still breathing" signal on RTT.)
    let mut beat = Ticker::every(Duration::from_secs(5));
    loop {
        beat.next().await;
        info!("app: alive");
    }
}

/// Poll the footswitches, debounce, and emit edge events.
///
/// Per-pin debounce: a level must read stable for `SETTLE_SAMPLES`
/// consecutive polls before it's accepted, then an event fires only when
/// the accepted level differs from the last *reported* state. This
/// suppresses contact bounce without an interrupt (the target task graph
/// uses GPIO IRQs; polling is the honest first cut and plenty for 10
/// switches at 200 Hz).
#[embassy_executor::task]
async fn buttons_task(
    buttons: [Input<'static>; SWITCH_COUNT],
    sender: Sender<'static, CriticalSectionRawMutex, ButtonEvent, BUTTON_Q>,
) {
    let mut reported = [false; SWITCH_COUNT]; // last debounced state emitted
    let mut raw_prev = [false; SWITCH_COUNT]; // last raw sample
    let mut stable = [0u8; SWITCH_COUNT]; // consecutive stable-sample count

    let mut poll = Ticker::every(Duration::from_millis(POLL_MS));
    loop {
        for i in 0..SWITCH_COUNT {
            let raw = buttons[i].is_low(); // active LOW → low == pressed
            if raw == raw_prev[i] {
                if stable[i] < SETTLE_SAMPLES {
                    stable[i] += 1;
                }
            } else {
                raw_prev[i] = raw;
                stable[i] = 0;
            }

            if stable[i] >= SETTLE_SAMPLES && reported[i] != raw {
                reported[i] = raw;
                // Block-the-producer: button edges are state changes we
                // don't want to drop. The queue depth makes this safe.
                sender
                    .send(ButtonEvent {
                        index: i as u8,
                        pressed: raw,
                    })
                    .await;
            }
        }
        poll.next().await;
    }
}

/// Maintain per-switch press counters and tell the display what to show.
///
/// This is the stub "app state". As subsystems land it grows into the real
/// event router (page navigation, MIDI dispatch, LED frames); for now it
/// just counts presses and forwards a render request.
#[embassy_executor::task]
async fn router_task(
    buttons: Receiver<'static, CriticalSectionRawMutex, ButtonEvent, BUTTON_Q>,
    display: Sender<'static, CriticalSectionRawMutex, DisplayCmd, DISPLAY_Q>,
) {
    let mut counts = [0u32; SWITCH_COUNT];
    loop {
        let ev = buttons.receive().await;
        let idx = ev.index as usize;
        if idx >= SWITCH_COUNT {
            continue; // defensive: ignore out-of-range indices
        }
        if ev.pressed {
            counts[idx] = counts[idx].wrapping_add(1);
            info!(
                "router: {} pressed -> count={}",
                SWITCH_NAMES[idx], counts[idx]
            );
            display
                .send(DisplayCmd::Pressed {
                    index: ev.index,
                    count: counts[idx],
                })
                .await;
        }
    }
}

/// Sole owner of the ST7789. Draws a static title and a status panel that
/// updates on each `DisplayCmd`.
#[embassy_executor::task]
async fn display_task(
    mut display: RemedyDisplay,
    _backlight: Output<'static>, // held to keep the backlight on
    commands: Receiver<'static, CriticalSectionRawMutex, DisplayCmd, DISPLAY_Q>,
) {
    // Dark background to match the firmware's theme.
    let _ = display.clear(Palette::BLACK.to_rgb565());

    // Static title bar across the top.
    let mut title: TextPanel<16> = TextPanel::new(
        Point::new(8, 8),
        Size::new(224, 56),
        Palette::WHITE,
        Palette::AZURE,
        &FONT_10X20,
        12,
    );
    title.set_text("MIDI Captain");
    let _ = title.render(&mut display);

    // Status panel: shows the most recent switch + its press count.
    let mut status: TextPanel<32> = TextPanel::new(
        Point::new(8, 96),
        Size::new(224, 88),
        Palette::WHITE,
        Palette::DARK_GREEN,
        &FONT_10X20,
        10,
    );
    status.set_text("press a switch");
    let _ = status.render(&mut display);

    loop {
        match commands.receive().await {
            DisplayCmd::Pressed { index, count } => {
                let name = SWITCH_NAMES
                    .get(index as usize)
                    .copied()
                    .unwrap_or("?");
                let mut line: String<32> = String::new();
                // Infallible for our content; ignore the capacity Result.
                let _ = write!(line, "{} x{}", name, count);
                status.set_text(&line);
                // render() returns Ok(false) if nothing changed — the
                // dirty-flag gate keeps the SPI quiet when it can.
                if let Ok(true) = status.render(&mut display) {
                    info!("display: {}", line.as_str());
                }
            }
        }
    }
}
