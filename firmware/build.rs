//! Build script: puts memory.x on the linker search path and wires up the
//! linker scripts that cortex-m-rt + embassy-rp need.

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    // Stage memory.x where rustc/lld will find it.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    // Apply to every linked artifact (today: examples; once a top-level
    // application binary lands, this picks it up automatically).
    println!("cargo:rustc-link-arg=--nmagic");
    // cortex-m-rt's link script
    println!("cargo:rustc-link-arg=-Tlink.x");
    // embassy-rp's RP2040 boot2 stub injection
    println!("cargo:rustc-link-arg=-Tlink-rp.x");
    // defmt symbol table
    println!("cargo:rustc-link-arg=-Tdefmt.x");
}
