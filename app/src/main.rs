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
// use embassy_rp::Peripheral;
use embassy_time::{with_timeout, Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use smart_leds::RGB8;
// use smart_leds::SmartLedsWrite;
use {defmt_rtt as _, panic_probe as _};

mod camera;
mod neopixel;
mod servo;
mod switch;

// use crate::camera::dvp::Dvp; // Unused
use crate::neopixel::Neopixel;

use crate::servo::{Channel, Servo};
use crate::switch::Switch;
use bead_sorter_bsp::Board;

const HOPPER_MIN: u16 = 500;
const HOPPER_MAX: u16 = 2266;

// Hopper States
const HOPPER_PICKUP_POS: u16 = 760;
const HOPPER_CAMERA_POS: u16 = 1493;
const HOPPER_ROW_POSITIONS: [u16; 4] = [2153, 2020, 1887, 1780];
const HOPPER_DROP_POS: u16 = 1613;

const CHUTES_MIN: u16 = 500;
const CHUTES_MAX: u16 = 1167;
const TUBE_COUNT: u8 = 30;

const CHUTE_SLICE_POSITIONS: [u16; 15] = [
    545, 586, 632, 675, 718, 762, 802, 842, 879, 920, 958, 999, 1041, 1085, 1132,
];

fn get_chute_pos(index: u8) -> u16 {
    let slice_idx = index as usize % 15;
    CHUTE_SLICE_POSITIONS[slice_idx]
}

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO0>;
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<embassy_rp::peripherals::I2C0>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
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

    // --- Hardware Initialization ---

    // 1. PIO0 (Shared by Neopixel and DVP)
    let mut pio = Pio::new(board.neopixel_pio, Irqs);

    // 2. Neopixel (SM0, DMA_CH0)
    let program = PioWs2812Program::new(&mut pio.common);
    let ws2812 = PioWs2812::new(
        &mut pio.common,
        pio.sm0,
        board.neopixel_dma,
        board.neopixel,
        &program,
    );
    let _neopixel: Neopixel<0, 1> = Neopixel::new(ws2812);

    // 3. Servos (50Hz)
    let mut servo_config = PwmConfig::default();
    servo_config.divider = fixed::FixedU16::from_num(125); // 1MHz
    servo_config.top = 20000; // 20ms

    // Hopper (PWM Slice 1 A)
    let hopper_pwm = Pwm::new_output_a(board.hopper_pwm, board.hopper_servo, servo_config.clone());
    let mut hopper = Servo::new(hopper_pwm, Channel::A, HOPPER_MIN, HOPPER_MAX);

    // Chutes (PWM Slice 5 A)
    let chutes_pwm = Pwm::new_output_a(board.chutes_pwm, board.chutes_servo, servo_config);
    let mut chutes = Servo::new(chutes_pwm, Channel::A, CHUTES_MIN, CHUTES_MAX);

    // 4. Pause Switch
    let pause_input = Input::new(board.pause_button, Pull::Up);
    let mut switch = Switch::new(pause_input);

    // 5. Camera LED
    let mut led = Output::new(board.camera_led, Level::Low);

    // 7. I2C0 For ov7670 configuration
    let mut i2c_config = embassy_rp::i2c::Config::default();
    i2c_config.frequency = 100_000;
    i2c_config.sda_pullup = false;
    i2c_config.scl_pullup = false;
    let mut i2c =
        embassy_rp::i2c::I2c::new_async(board.i2c0, board.i2c_scl, board.i2c_sda, Irqs, i2c_config);

    // --- Tasks ---
    let usb_fut = usb.run();

    let demo_fut = async {
        // Wait for USB connection (DTR)
        while !class.dtr() {
            led.set_high();
            Timer::after(Duration::from_millis(100)).await;
            led.set_low();
            Timer::after(Duration::from_millis(100)).await;
        }
        led.set_low();
        Timer::after(Duration::from_millis(100)).await;

        // Camera Init removed from log
        Timer::after(Duration::from_millis(300)).await;

        // Scan Logic removed from log

        // Initialize Ov7670 Camera
        // Initialize Ov7670 Camera (Verbose log removed)

        // Pass board.cam_pins and board.cam_dma to the new struct
        // We use full path or imported path
        let mut camera = crate::camera::ov7670::Ov7670::new(
            i2c,
            &mut pio.common,
            pio.sm1,
            board.cam_dma,
            board.camera_mclk_pwm,
            board.cam_pins,
        )
        .await;

        // Buffer capture logs removed

        // Sorting Loop
        let _ = class.write_packet(b"Starting Sorter Loop...\r\n").await;

        loop {
            if switch.is_active() {
                // Paused
                led.set_high();
                if class.dtr() {
                    let _ =
                        with_timeout(Duration::from_millis(5), class.write_packet(b"Paused\r\n"))
                            .await;
                }
                Timer::after(Duration::from_millis(500)).await;
                continue;
            }
            led.set_low();

            // 1. Pickup Bead (Agitate to capture)
            let pickup_center = HOPPER_PICKUP_POS;
            hopper
                .move_to(pickup_center - 100, Duration::from_millis(150))
                .await;
            hopper
                .move_to(pickup_center + 100, Duration::from_millis(150))
                .await;
            hopper
                .move_to(pickup_center - 50, Duration::from_millis(150))
                .await;
            hopper
                .move_to(pickup_center + 50, Duration::from_millis(150))
                .await;
            hopper
                .move_to(pickup_center, Duration::from_millis(150))
                .await;
            Timer::after(Duration::from_millis(100)).await;

            // 2. Move to Camera
            hopper
                .move_to(HOPPER_CAMERA_POS, Duration::from_millis(500))
                .await;
            Timer::after(Duration::from_millis(100)).await; // Settle

            // 3. Capture & Classify
            // let mut buf = [0u8; 64]; // This buffer needs to be defined outside the loop or passed in
            // camera.capture(&mut buf).await;
            // let _ = class.write_packet(b"Captured\r\n").await;

            // Mock Classification
            let timestamp = embassy_time::Instant::now().as_millis();
            let tube_index = ((timestamp / 2000) % 30) as u8; // Cycle through all 30 tubes
            let chute_target = get_chute_pos(tube_index);

            // Calculate Hopper Row
            // Formula: (tube / 15) * 2 + (tube % 15) & 1 ?
            // User formula: (tube_idx / 15) << 1 | ((tube_idx % 15) & 1)
            // (0..14) -> 0 -> row 0 or 1.
            // (15..29) -> 1 -> row 2 or 3.
            let row_index = ((tube_index / 15) << 1) | ((tube_index % 15) & 1);
            let drop_row = HOPPER_ROW_POSITIONS[row_index as usize];

            // 4. Move Chute (750ms duration)
            chutes
                .move_to(chute_target, Duration::from_millis(750))
                .await;

            // 5. Drop Bead (Align with Row then Retract/Drop)
            hopper.move_to(drop_row, Duration::from_millis(500)).await;
            Timer::after(Duration::from_millis(200)).await;
            hopper
                .move_to(HOPPER_DROP_POS, Duration::from_millis(500))
                .await;
        }
    };

    futures::join!(usb_fut, demo_fut);
}
