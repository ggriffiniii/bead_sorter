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
// extern crate alloc;
use {defmt_rtt as _, panic_probe as _};

// mod camera;
mod neopixel;
mod servo;
mod switch;

use crate::neopixel::Neopixel;
use crate::servo::{Channel, Servo};
use crate::switch::Switch;
use bead_sorter_bsp::Board;

const HOPPER_MIN: u16 = 567;
const HOPPER_MAX: u16 = 2266;

// Hopper States
const HOPPER_PICKUP_POS: u16 = 900;
const HOPPER_CAMERA_POS: u16 = 1580;
const HOPPER_ROW_POSITIONS: [u16; 4] = [2240, 2107, 1994, 1914];
const HOPPER_DROP_POS: u16 = 1700;

const CHUTES_MIN: u16 = 500;
const CHUTES_MAX: u16 = 1167;
const TUBE_COUNT: u8 = 30;

const CHUTE_SLICE_POSITIONS: [u16; 15] = [
    544, 589, 633, 678, 722, 767, 811, 856, 900, 945, 989, 1034, 1078, 1123, 1167,
];

fn get_chute_pos(index: u8) -> u16 {
    let slice_idx = index as usize % 15;
    CHUTE_SLICE_POSITIONS[slice_idx]
}

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO0>;
    // I2C0_IRQ => embassy_rp::i2c::InterruptHandler<embassy_rp::peripherals::I2C0>;
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

    // 5. Camera SCCB (I2C0: SDA=GPIO12, SCL=GPIO13)
    /*
    let sda = unsafe { embassy_rp::peripherals::PIN_12::steal() };
    let scl = unsafe { embassy_rp::peripherals::PIN_13::steal() };
    let i2c0 = unsafe { embassy_rp::peripherals::I2C0::steal() };

    let i2c =
        embassy_rp::i2c::I2c::new_async(i2c0, scl, sda, Irqs, embassy_rp::i2c::Config::default());
    let mut sccb = crate::camera::sccb::Sccb::new(i2c);
    */

    // --- Tasks ---
    let usb_fut = usb.run();

    let demo_fut = async {
        // Wait for USB
        Timer::after(Duration::from_millis(1000)).await;

        // Camera Presence Check
        /*
        match sccb.read_reg(0x0A).await {
            Ok(pid) => {
                let _ = class
                    .write_packet(
                        alloc::format!("Camera Connected! PID: {:02x}\r\n", pid).as_bytes(),
                    )
                    .await;
            }
            Err(e) => {
                let _ = class.write_packet(b"Camera connect failed\r\n").await;
            }
        }
        */

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

                // Sorting Sequence Demo

                // 1. Pickup Bead
                // let _ = class.write_packet(b"Pickup\r\n").await;
                hopper
                    .move_to(HOPPER_PICKUP_POS, Duration::from_millis(500))
                    .await;
                Timer::after(Duration::from_millis(200)).await; // Wait for bead pickup

                // 2. Move to Camera
                // let _ = class.write_packet(b"Inspect\r\n").await;
                hopper
                    .move_to(HOPPER_CAMERA_POS, Duration::from_millis(500))
                    .await;
                Timer::after(Duration::from_millis(200)).await; // Simulate picture snap time

                // 3. Classify & Select Chute
                let timestamp = embassy_time::Instant::now().as_millis();
                let tube_index = ((timestamp / 1000) % 30) as u8; // Cycle through tubes based on time

                // let msg = alloc::format!("Target Chute: {}\r\n", tube_index);
                // let _ = class.write_packet(msg.as_bytes()).await;

                let chute_target = get_chute_pos(tube_index);
                chutes
                    .move_to(chute_target, Duration::from_millis(500))
                    .await;

                neopixel.write(&[RGB8::new(0, 20, 0)]).await; // Green indicates "Classified"

                // 4. Drop Bead
                // let _ = class.write_packet(b"Drop\r\n").await;
                hopper
                    .move_to(HOPPER_DROP_POS, Duration::from_millis(500))
                    .await;
                Timer::after(Duration::from_millis(200)).await; // Wait for drop

                // Reset indicator
                neopixel.write(&[RGB8::new(0, 0, 0)]).await;
            }
            Timer::after(Duration::from_millis(100)).await;
        }
    };

    futures::join!(usb_fut, demo_fut);
}
