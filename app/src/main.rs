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
use smart_leds::{SmartLedsWrite, RGB8};
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
    let mut neopixel: Neopixel<0, 1> = Neopixel::new(ws2812);

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

        let _ = class
            .write_packet(b"Starting Camera (MCLK=17.9MHz)...\r\n")
            .await;
        Timer::after(Duration::from_millis(300)).await;

        let _ = class
            .write_packet(b"Scanning I2C Bus (0x08-0x77)...\r\n")
            .await;

        // Scan Loop using `i2c` directly
        for addr in 0x08u16..0x77u16 {
            let mut buf = [0u8; 1];
            // Very short timeout for scanning
            let res = with_timeout(Duration::from_millis(5), i2c.read_async(addr, &mut buf)).await;
            if let Ok(Ok(_)) = res {
                let mut packet = [0u8; 32];
                let msg = b"Found device at 0x";
                packet[0..18].copy_from_slice(msg);
                let hex = b"0123456789abcdef";
                packet[18] = hex[(addr >> 4) as usize];
                packet[19] = hex[(addr & 0xF) as usize];
                packet[20] = b'\r';
                packet[21] = b'\n';
                let _ = class.write_packet(&packet[0..22]).await;
            }
        }
        let _ = class.write_packet(b"Scan Complete.\r\n").await;

        // Initialize Ov7670 Camera
        let _ = class.write_packet(b"Initializing OV7670...\r\n").await;

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

        let _ = class.write_packet(b"Camera Configured.\r\n").await;

        // --- DVP Capture Test ---
        // DVP Capture Test
        let _ = class
            .write_packet(b"Capturing 40x30 Frame (DMA)... \r\n")
            .await;

        let mut buf = [0u32; 600];

        // DMA Capture via Ov7670
        camera.capture(&mut buf).await;

        let _ = class.write_packet(b"Frame Captured!\r\n").await;

        // Print first few pixels
        for i in 0..4 {
            let word = buf[i];
            // Each word is 2 pixels (2 bytes each).
            // Format: P1_L P1_H P0_L P0_H ?? Depends on shift.
            // Shift Left: D0 is LSB.
            // 32 bits: [Pixel 1] [Pixel 0]
            // Let's just print hex.
            let mut hex_buf = [0u8; 10]; // "0xXXXXXXXX"
            hex_buf[0..10].copy_from_slice(b"0x00000000");
            // Simple hex print helper or raw bytes
            // Let's just print "Pixel data captured" for now to save complexity.
        }

        // Temporary: Flash LED Green to show success
        neopixel.write(&[RGB8::new(0, 50, 0)]).await;
        Timer::after(Duration::from_millis(100)).await;
        neopixel.write(&[RGB8::new(0, 0, 0)]).await;

        Timer::after(Duration::from_millis(100)).await;

        // ...

        let count = buf.len(); // Full buffer filled by DMA

        // ...

        let mut msg: heapless::String<64> = heapless::String::new();
        use core::fmt::Write;
        let _ = write!(msg, "Captured {} words (40x30).\r\n", count);
        let _ = class.write_packet(msg.as_bytes()).await;

        let val0 = buf[0];
        let val1 = buf[1];
        msg.clear();
        let _ = write!(msg, "Data: {:08x} {:08x} ...\r\n", val0, val1);
        let _ = class.write_packet(msg.as_bytes()).await;

        loop {
            if switch.is_active() {
                // OFF/Paused state logic
                led.set_high(); // LED ON = Paused (inverse logic?) or just indicator.
                if class.dtr() {
                    let _ =
                        with_timeout(Duration::from_millis(5), class.write_packet(b"Paused\r\n"))
                            .await;
                }
                Timer::after(Duration::from_millis(1000)).await;
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
