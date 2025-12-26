#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{PIO0, USB};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::usb;
use embassy_time::{with_timeout, Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use smart_leds::{SmartLedsWrite, RGB8};
use {defmt_rtt as _, panic_probe as _};

mod neopixel;
mod servo;
mod switch;

use crate::neopixel::Neopixel;
use crate::servo::{Channel, Servo};
use crate::switch::Switch;
use bead_sorter_bsp::Board;

const HOPPER_MIN: u16 = 567;
const HOPPER_MAX: u16 = 2266;

const CHUTES_MIN: u16 = 500;
const CHUTES_MAX: u16 = 1167;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO0>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let board = Board::new(p);

    // --- USB Setup ---
    let driver = embassy_rp::usb::Driver::new(board.usb, Irqs);
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Bead Sorter");
    config.product = Some("Firmware");
    config.serial_number = Some("12345678");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    let mut state = State::new();
    let mut config_desc = [0u8; 256];
    let mut bos_desc = [0u8; 256];
    let mut control_buf = [0u8; 64];
    let mut msos_desc = [0u8; 256];

    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        &mut config_desc,
        &mut bos_desc,
        &mut msos_desc,
        &mut control_buf,
    );

    let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);
    let mut usb = builder.build();

    // --- Hardware Initialization (Using Steal Workaround) ---

    // 1. Neopixel (GPIO20, PIO0, SM0, DMA_CH0)
    let neopixel_pin = unsafe { embassy_rp::peripherals::PIN_20::steal() };
    let neopixel_dma = unsafe { embassy_rp::peripherals::DMA_CH0::steal() };
    let neopixel_pio_peri = unsafe { embassy_rp::peripherals::PIO0::steal() };

    let mut pio = Pio::new(neopixel_pio_peri, Irqs);
    let program = PioWs2812Program::new(&mut pio.common);
    let ws2812 = PioWs2812::new(
        &mut pio.common,
        pio.sm0,
        neopixel_dma,
        neopixel_pin,
        &program,
    );
    let mut neopixel: Neopixel<0, 1> = Neopixel::new(ws2812);

    // 2. Servos
    // Config: 50Hz (20ms)
    // Clock 125MHz. Divider 125 -> 1MHz. Top 20000 -> 20ms.
    let mut servo_config = PwmConfig::default();
    servo_config.divider = fixed::FixedU16::from_num(125); // 125.0
    servo_config.top = 20000;

    // Hopper (GPIO18 - PWM1 A)
    let hopper_slice = unsafe { embassy_rp::peripherals::PWM_SLICE1::steal() };
    let hopper_pin = unsafe { embassy_rp::peripherals::PIN_18::steal() };
    let hopper_pwm = Pwm::new_output_a(hopper_slice, hopper_pin, servo_config.clone());
    let mut hopper = Servo::new(hopper_pwm, Channel::A, HOPPER_MIN, HOPPER_MAX);

    // Chutes (GPIO26 - PWM5 A)
    let chutes_slice = unsafe { embassy_rp::peripherals::PWM_SLICE5::steal() };
    let chutes_pin = unsafe { embassy_rp::peripherals::PIN_26::steal() };
    let chutes_pwm = Pwm::new_output_a(chutes_slice, chutes_pin, servo_config);
    let mut chutes = Servo::new(chutes_pwm, Channel::A, CHUTES_MIN, CHUTES_MAX);

    // 3. Pause Switch (GPIO19)
    let pause_pin = unsafe { embassy_rp::peripherals::PIN_19::steal() };
    let pause_input = Input::new(pause_pin, Pull::Up);
    let mut switch = Switch::new(pause_input);

    // 4. Camera LED (GPIO23)
    let cam_led_pin = unsafe { embassy_rp::peripherals::PIN_23::steal() };
    let mut led = Output::new(cam_led_pin, Level::Low);

    // --- Tasks ---
    let usb_fut = usb.run();

    let demo_fut = async {
        // Wait for USB
        Timer::after(Duration::from_millis(1000)).await;

        loop {
            if switch.is_active() {
                // OFF/Paused state logic
                led.set_high(); // LED ON = Paused (inverse logic?) or just indicator.
                if class.dtr() {
                    let _ =
                        with_timeout(Duration::from_millis(5), class.write_packet(b"Paused\r\n"))
                            .await;
                }
            } else {
                led.set_low();
                if class.dtr() {
                    let _ =
                        with_timeout(Duration::from_millis(5), class.write_packet(b"Running\r\n"))
                            .await;
                }

                // Demo routine
                // Move to "Start" positions (e.g. Min)
                hopper
                    .move_to(HOPPER_MIN, Duration::from_millis(1000))
                    .await;
                chutes
                    .move_to(CHUTES_MIN, Duration::from_millis(1000))
                    .await;

                neopixel.write(&[RGB8::new(20, 0, 0)]).await; // Red
                Timer::after(Duration::from_millis(500)).await;

                // Move to "End" positions (e.g. Max)
                hopper
                    .move_to(HOPPER_MAX, Duration::from_millis(1000))
                    .await;
                // For chutes, demonstrate a mid-point or full range
                chutes
                    .move_to(CHUTES_MAX, Duration::from_millis(1000))
                    .await;

                neopixel.write(&[RGB8::new(0, 0, 20)]).await; // Blue
                Timer::after(Duration::from_millis(500)).await;
            }
            Timer::after(Duration::from_millis(100)).await;
        }
    };

    futures::join!(usb_fut, demo_fut);
}
