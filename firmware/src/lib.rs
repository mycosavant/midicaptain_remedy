//! MIDI Captain Remedy — Rust + Embassy firmware library.
//!
//! This crate is the shared layer for the example binaries in `examples/`.
//! As the port matures it will grow modules for the event dispatcher,
//! config loader, display driver, MIDI multiplexer, etc. For now it just
//! pins down the board's GPIO map so every example refers to the same
//! constants.
//!
//! See `ARCHITECTURE.md` for the task graph and the planned module layout.

#![no_std]

pub mod display;
pub mod events;
pub mod hal;
pub mod pins;
pub mod storage;
pub mod ui;
