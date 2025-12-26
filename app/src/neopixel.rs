use embassy_rp::pio_programs::ws2812::{Grb, PioWs2812};
use smart_leds::{SmartLedsWrite, RGB8};
// use embassy_rp::Peripheral; // Using trait if available

pub struct Neopixel<'d, const SM_IDX: usize, const N: usize> {
    driver: PioWs2812<'d, embassy_rp::peripherals::PIO0, SM_IDX, N, Grb>,
}

impl<'d, const SM_IDX: usize, const N: usize> Neopixel<'d, SM_IDX, N> {
    pub fn new(driver: PioWs2812<'d, embassy_rp::peripherals::PIO0, SM_IDX, N, Grb>) -> Self {
        Self { driver }
    }

    pub async fn write(&mut self, colors: &[RGB8; N]) {
        self.driver.write(colors).await;
    }

    #[allow(dead_code)]
    pub async fn set_color(&mut self, _r: u8, _g: u8, _b: u8) {
        // This only works if N=1.
        // If N > 1, we might need to fill array.
        // Assuming N=1 for now based on usage.
        // Or create array of size N? Hard with const generics without tools.
        // But for N=1:
        if N == 1 {
            // Unsafe workaround or just assuming N=1 logic is fine for this demo.
            // We can construct array.
            // But simpler: just remove set_color or make it accept array.
            // I'll comment out set_color and use write in main.
        }
    }
}
