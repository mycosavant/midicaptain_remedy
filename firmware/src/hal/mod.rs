//! Hardware-abstraction layer — thin async wrappers over `embassy-rp`
//! peripherals that turn raw hardware into the channel-contract messages in
//! [`crate::events`].
//!
//! Each submodule owns its peripheral(s) exclusively and runs as its own
//! Embassy task feeding the router (see the task graph in
//! `ARCHITECTURE.md`). One owner per peripheral means no locking: to act on
//! a peripheral from elsewhere, send it a message.
//!
//! Landed:
//! - [`encoder`] — quadrature rotary encoder + debounced push-button,
//!   emitting [`crate::events::EncoderEvent`].
//! - [`expression`] — the two ADC expression pedals (GP27/GP28) →
//!   [`crate::events::ExprEvent`].
//! - [`leds`] — drives the 30-pixel WS2812 chain on GP7 from `LedFrame`s.
//!
//! Planned (per `ARCHITECTURE.md`): `buttons`. It slots in here as a sibling
//! module, with its own `*_task` and channel aliases.

pub mod encoder;
pub mod expression;
pub mod leds;
