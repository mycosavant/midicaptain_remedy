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
//! - [`expression`] — the two ADC expression pedals (GP27/GP28) →
//!   [`crate::events::ExprEvent`].
//!
//! Planned (per `ARCHITECTURE.md`): `buttons`, `encoder`, `leds`. They slot
//! in here as sibling modules, each with its own `*_task` and channel
//! aliases.

pub mod expression;
