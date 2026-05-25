MEMORY {
    /* RP2040: 2 MB QSPI flash, 264 KB SRAM. */
    /* BOOT2 is the 256-byte second-stage bootloader stub that the BootROM */
    /* runs before our firmware. Provided by rp2040-boot2 via embassy-rp. */
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}
