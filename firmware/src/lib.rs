//! MIDI Captain Remedy — Rust + Embassy firmware library.
//!
//! Shared layer for the application binary (`src/bin/midicaptain.rs`) and
//! the bring-up examples in `examples/`. It holds the board pin map
//! ([`pins`]), the frozen channel contracts every task codes against
//! ([`events`]), the ST7789 driver + UI scene graph ([`display`], [`ui`]),
//! the hardware-abstraction tasks ([`hal`]), the MIDI engine ([`midi`]),
//! and the flash settings store ([`storage`]).
//!
//! See `ARCHITECTURE.md` for the task graph and module layout.

#![no_std]

pub mod app;
pub mod config;
pub mod display;
pub mod events;
pub mod hal;
pub mod menu;
pub mod midi;
pub mod pins;
pub mod pitch;
pub mod storage;
pub mod tuner;
pub mod ui;
