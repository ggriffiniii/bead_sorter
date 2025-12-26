# Hardware Drivers Walkthrough

This document details the implementation of hardware drivers for the custom RP2040 board.

## Driver Overview

We have implemented three core drivers in the `app` crate:

1.  **Neopixel (GPIO20)**
    -   **Implementation**: Uses `embassy-rp`'s `PioWs2812` driver powered by PIO0 (State Machine 0) and DMA Channel 0.
    -   **Generics Resolution**: Resolved complex generic type bounds by explicitly specifying `embassy_rp::peripherals::PIO0` and `embassy_rp::pio_programs::ws2812::Grb` (Color Order).
    -   **API**: Exposed a `write` method accepting a fixed-size array of `smart_leds::RGB8` colors.

2.  **Servos (GPIO18 & GPIO26)**
    -   **Implementation**: Uses `embassy-rp`'s `Pwm` driver.
    -   **Configuration**: Configured for 50Hz (20ms period).
        -   **Hopper**: pulse width constrained to 567-2266 µs.
        -   **Chutes**: pulse width constrained to 500-1167 µs.
    -   **Safety**: Includes `min_us` and `max_us` clamping in the `set_pulse_width` method.
    -   **Smooth Movement**: Implements `move_to` async method with cubic easing to prevent jerky motion.
    -   **Sorting Logic**: Implements a `Pickup -> Camera -> Drop` sequence for the Hopper and maps 30 virtual tube positions to the Chutes servo range.

3.  **Pause Switch (GPIO19)**
    -   **Implementation**: Uses `embassy-rp`'s `Input` driver with internal Pull-Up resistor.
    -   **Logic**: Active-Low logic (common for switches connecting to Ground).

## Integration Strategy: The "Steal" Workaround

To resolve persistent trait bound conflicts (specifically regarding the `Peripheral` trait visibility and compatibility between the BSP's `Peri` wrappers and `embassy-rp` 0.1.0 driver signatures), we adopted a direct initialization strategy in `main.rs`:

-   **Bypassed BSP Wrappers**: Instead of using `board.neopixel` (which is a `Peri` wrapper), we use `unsafe { embassy_rp::peripherals::PIN_XX::steal() }` to obtain a fresh, raw peripheral handle.
-   **Direct Driver Initialization**: Drivers are initialized directly in `main` using these raw peripherals, ensuring full compatibility with `embassy-rp` driver constructors.
-   **Clean Separation**: The `Board` struct still handles general board setup, but specific drivers take ownership of their required resources directly.

## Code Snippet: Main Loop

The `main.rs` loop demonstrates concurrent operation of USB logging, LED blinking (status), and Servo/Neopixel updates:

```rust
loop {
    if switch.is_active() {
        led.set_high(); // Paused
    } else {
        led.set_low(); // Running
        // Sweep Servos & Cycle Colors
        hopper.set_pulse_width(1000);
        neopixel.write(&[RGB8::new(20, 0, 0)]).await; 
        Timer::after(Duration::from_millis(500)).await;
        // ...
    }
}
```

## Compilation Status

The firmware compiles successfully with `release` profile:
-   **Build Command**: `cargo build --release`
-   **Status**: Success (Exit Code 0)
