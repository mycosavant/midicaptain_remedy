//! `hal` — thin async wrappers over the `embassy-rp` peripherals.
//!
//! Each submodule owns exactly one peripheral and exposes it to the rest
//! of the system as an Embassy task plus a typed `embassy_sync` channel,
//! per the task graph in [`ARCHITECTURE.md`](../../ARCHITECTURE.md). No
//! shared mutable state, one owner per peripheral.
//!
//! Landed so far:
//! - [`leds`] — drives the 30-pixel WS2812 chain on GP7 from `LedFrame`s.
//!
//! Planned (one per follow-up session): `buttons`, `encoder`, `expression`.

pub mod leds;
