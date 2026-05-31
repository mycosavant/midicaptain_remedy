//! Footswitch debouncer.
//!
//! Polls the ten chassis footswitches and emits debounced
//! [`ButtonEvent`](crate::events::ButtonEvent) edges. Lifted out of the app
//! binary so it sits alongside the other input HAL tasks (`encoder`,
//! `expression`) with the same channel-alias + task surface.
//!
//! The target task graph (see `ARCHITECTURE.md`) uses GPIO IRQs; polling at
//! 200 Hz is the honest first cut and plenty for ten switches. Per-pin
//! debounce: a level must read stable for [`SETTLE_SAMPLES`] consecutive
//! polls before it's accepted, then an event fires only when the accepted
//! level differs from the last *reported* state.

use embassy_rp::gpio::Input;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Ticker};

use crate::events::ButtonEvent;
use crate::pins;

/// Number of footswitches scanned (chassis switches, excluding the encoder
/// push-button, which the encoder task owns).
pub const COUNT: usize = pins::FOOTSWITCH_COUNT;

/// Depth of the [`ButtonEvent`] channel. Buttons are bursty (a stomp can
/// bounce a few edges); 16 absorbs that.
pub const BUTTON_QUEUE_DEPTH: usize = 16;

/// Poll period for the debouncer. `POLL_MS × SETTLE_SAMPLES` = settle time.
const POLL_MS: u64 = 5;
/// Consecutive stable samples required before a level change is accepted.
/// 3 × 5 ms = 15 ms — comfortably past contact bounce, under human perception.
const SETTLE_SAMPLES: u8 = 3;

/// Bounded MPSC channel carrying [`ButtonEvent`]s to the router.
pub type ButtonChannel = Channel<CriticalSectionRawMutex, ButtonEvent, BUTTON_QUEUE_DEPTH>;
/// Sender half — held by [`buttons_task`].
pub type ButtonSender = Sender<'static, CriticalSectionRawMutex, ButtonEvent, BUTTON_QUEUE_DEPTH>;
/// Receiver half — held by the router.
pub type ButtonReceiver = Receiver<'static, CriticalSectionRawMutex, ButtonEvent, BUTTON_QUEUE_DEPTH>;

/// Poll the footswitches, debounce, and emit edge events.
///
/// `buttons` are the [`COUNT`] footswitch GPIOs as `Input`s, in the order
/// that defines each switch's `ButtonEvent.index` (and hence its
/// `config::SWITCH_FOR_BUTTON` LED mapping). They are active-LOW with
/// internal pull-ups: `is_low()` == pressed.
#[embassy_executor::task]
pub async fn buttons_task(buttons: [Input<'static>; COUNT], sender: ButtonSender) {
    let mut reported = [false; COUNT]; // last debounced state emitted
    let mut raw_prev = [false; COUNT]; // last raw sample
    let mut stable = [0u8; COUNT]; // consecutive stable-sample count

    let mut poll = Ticker::every(Duration::from_millis(POLL_MS));
    loop {
        for i in 0..COUNT {
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
