MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 16M - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}

EXTERN(BOOT2_FIRMWARE)

SECTIONS {
    /* valid for all RP2040 flashes */
    .boot2 ORIGIN(BOOT2) :
    {
        KEEP(*(.boot2));
    } > BOOT2
}

/* We don't strictly need to re-define everything if we use the standard cortex-m-rt linkers, 
   but specifying FLASH length is critical. 
   The cortex-m-rt will pick up these MEMORY regions. */
