/* Linker memory layout for the RP2040 (2 MB QSPI XIP flash, 264 KB SRAM). */
MEMORY {
    /* BOOT2 is the 256-byte second-stage bootloader stub that the BootROM */
    /* runs before our firmware. Provided by rp2040-boot2 via embassy-rp.  */
    BOOT2  : ORIGIN = 0x10000000, LENGTH = 0x100

    /* Firmware image. Length is the full 2 MB, minus BOOT2, minus the      */
    /* CONFIG region reserved at the very top of flash (see below).         */
    /* Shrinking FLASH here is what GUARANTEES the linker can never place   */
    /* code or data into the key-value store's erase sectors: the two       */
    /* ranges are physically disjoint by construction, not by convention.   */
    FLASH  : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100 - 64K

    /* CONFIG: 64 KB (16 x 4 KB erase sectors) at the TOP of QSPI flash,    */
    /* 0x101F0000 .. 0x10200000. This is NOT a linker output section —      */
    /* nothing is placed here at link time; the region exists to document   */
    /* and reserve the range. It is owned at RUNTIME by `src/storage.rs`,   */
    /* which drives a `sequential-storage` wear-levelling key-value store   */
    /* over `embassy_rp::flash`, addressing it by FLASH-relative offsets    */
    /* CONFIG_REGION_START .. FLASH_SIZE (0x1F0000 .. 0x200000). Keep those */
    /* constants in storage.rs in sync with the numbers here.              */
    CONFIG : ORIGIN = 0x101F0000, LENGTH = 64K

    RAM    : ORIGIN = 0x20000000, LENGTH = 256K
}
