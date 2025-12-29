#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Pull};
use embassy_rp::peripherals::{PIO0, USB};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::usb;
// use embassy_rp::Peripheral;
use embassy_futures::join::join;
use embassy_time::{with_timeout, Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use smart_leds::RGB8;
// use smart_leds::SmartLedsWrite;
use core::fmt::Write;
use heapless::String;
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
use sorter_logic::{analyze_image, Palette, PaletteMatch};

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
    let mut hopper = Servo::new(hopper_pwm, Channel::A, HOPPER_MIN, HOPPER_MAX, 5250); // 2000us/s speed

    // Chutes (PWM Slice 5 A)
    let chutes_pwm = Pwm::new_output_a(board.chutes_pwm, board.chutes_servo, servo_config);
    let mut chutes = Servo::new(chutes_pwm, Channel::A, CHUTES_MIN, CHUTES_MAX, 6000); // 2000us/s speed

    // 4. Pause Switch
    let pause_input = Input::new(board.pause_button, Pull::Up);
    let mut switch = Switch::new(pause_input);

    // 5. Camera LED (PWM Slice 3 B, Pin 23)
    let mut led_config = PwmConfig::default();
    led_config.divider = fixed::FixedU16::from_num(125); // 1MHz (1us tick)
    led_config.top = 1000; // 1kHz (1ms period)
    led_config.compare_b = 500; // 50% Duty Cycle
    let mut led = Pwm::new_output_b(board.camera_led_pwm, board.camera_led, led_config.clone());

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
        // Ensure LED is ON (50%)
        led.set_config(&led_config);
        Timer::after(Duration::from_millis(100)).await;

        Timer::after(Duration::from_millis(300)).await;

        // Initialize Ov7670 Camera
        // Pass board.cam_pins and board.cam_dma to the new struct
        let mut camera = crate::camera::ov7670::Ov7670::new(
            i2c,
            &mut pio.common,
            pio.sm1,
            board.cam_dma,
            board.camera_mclk_pwm,
            board.cam_pins,
        )
        .await;

        // Sorting State
        let mut palette: Palette<128> = Palette::new();
        let mut tubes: heapless::Vec<sorter_logic::PaletteEntry, 30> = heapless::Vec::new();
        // Index is PaletteID, Value is TubeID. 0xFF = None
        let mut palette_to_tube: [u8; 128] = [0xFF; 128];

        // Sorting Loop
        if class.dtr() {
            let _ = class.write_packet(b"Starting Sorter Loop...\r\n").await;
        }

        loop {
            if switch.is_active() {
                // Paused
                // Turn OFF LED when paused
                led_config.compare_b = 0;
                led.set_config(&led_config);
                if class.dtr() {
                    let _ =
                        with_timeout(Duration::from_millis(5), class.write_packet(b"Paused\r\n"))
                            .await;
                }
                Timer::after(Duration::from_millis(500)).await;
                continue;
            }
            // Turn ON LED (50%) when running
            led_config.compare_b = 500;
            led.set_config(&led_config);

            // 1. Pickup Bead (Agitate to capture)
            let pickup_center = HOPPER_PICKUP_POS;
            hopper.move_to(pickup_center - 250).await;
            hopper.move_to(pickup_center + 250).await;
            hopper.move_to(pickup_center - 150).await;
            hopper.move_to(pickup_center + 150).await;
            hopper.move_to(pickup_center - 75).await;
            hopper.move_to(pickup_center + 75).await;
            hopper.move_to(pickup_center).await;
            Timer::after(Duration::from_millis(100)).await;

            // 2. Move to Camera
            hopper.move_to(HOPPER_CAMERA_POS).await;
            Timer::after(Duration::from_millis(200)).await; // Settle for stable image

            let mut buf = [0u32; 600];
            let _ = camera.capture(&mut buf).await;

            // RESTORE BINARY STREAM
            // Send Image Header: [0xBE, 0xAD, 0x1F, 0x01]
            let header = [0xBE, 0xAD, 0x1F, 0x01];
            let _ = class.write_packet(&header).await;

            // Send Image Data
            // Safety: Transmuting valid u32 slice to u8 slice for transmission.
            let buf_bytes =
                unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };

            // Write in chunks to avoid overwhelming USB buffer if necessary
            for chunk in buf_bytes.chunks(64) {
                let _ = class.write_packet(chunk).await;
            }

            let _ = class.write_packet(b"Captured\r\n").await;

            let analysis = analyze_image(buf_bytes, 40, 30); // 40x30 resolution

            // Default to Bin 0 (Waste/Unclassified) if analysis fails
            let mut tube_index = 0;

            if let Some(ana) = analysis {
                // Adaptive Learning
                let match_result = palette.match_color(&ana.average_color, ana.variance, 15);

                let p_idx = match match_result {
                    PaletteMatch::Match(i) => Some(i),
                    PaletteMatch::NewEntry(i) => Some(i),
                    PaletteMatch::Full => None, // Palette Full -> Unclassified (Tube 0)
                };

                if let Some(idx) = p_idx {
                    // Update Learning
                    palette.add_sample(idx, &ana.average_color, ana.variance);

                    // Map to Tube
                    let tid = if palette_to_tube[idx] != 0xFF {
                        palette_to_tube[idx] as usize
                    } else {
                        // New Palette, assign tube
                        if tubes.len() < 30 {
                            // New Tube
                            // Note: Firmware needs PaletteEntry struct too.
                            // Assuming sorter_logic exposes PaletteEntry publicly (it does).
                            // But struct construction might differ without 'new'?
                            // sorter_logic::PaletteEntry::new(rgb, var)
                            // We need to import PaletteEntry.
                            let entry =
                                sorter_logic::PaletteEntry::new(ana.average_color, ana.variance);
                            let _ = tubes.push(entry);
                            tubes.len() - 1
                        } else {
                            // Find closest tube
                            let mut best_t = 0;
                            let mut min_d = u32::MAX;
                            for (t_i, t_entry) in tubes.iter().enumerate() {
                                let (t_avg, _) = t_entry.avg();
                                let d = ana.average_color.dist_lab(&t_avg);
                                if d < min_d {
                                    min_d = d;
                                    best_t = t_i;
                                }
                            }
                            best_t
                        }
                    };

                    if idx < 128 {
                        palette_to_tube[idx] = tid as u8;
                    }

                    // Update Tube Stats
                    if tid < tubes.len() {
                        tubes[tid].add(ana.average_color, ana.variance);
                    }

                    tube_index = tid as u8;
                }
            }

            // Send classification info
            // let mut msg = String::<64>::new();
            // let _ = class.write_packet(msg.as_bytes()).await;

            let chute_target = get_chute_pos(tube_index);

            // Calculate Hopper Row
            // Formula: (tube / 15) * 2 + (tube % 15) & 1 ?
            // User formula: (tube_idx / 15) << 1 | ((tube_idx % 15) & 1)
            // (0..14) -> 0 -> row 0 or 1.
            // (15..29) -> 1 -> row 2 or 3.
            let row_index = ((tube_index / 15) << 1) | ((tube_index % 15) & 1);
            let drop_row = HOPPER_ROW_POSITIONS[row_index as usize];

            // 4. Move Chute & 5. Align Hopper (Concurrent)
            // Ensure Chutes are done (750ms) before Drops proceed.
            // Hopper align takes 500ms + 200ms wait = 700ms.
            // So join will finish at 750ms (dominated by chutes).
            let chutes_fut = chutes.move_to(chute_target);
            let hopper_align_fut = async {
                hopper.move_to(drop_row).await;
                Timer::after(Duration::from_millis(200)).await;
            };

            join(chutes_fut, hopper_align_fut).await;

            // 6. Retract/Drop
            hopper.move_to(HOPPER_DROP_POS).await;
            Timer::after(Duration::from_millis(300)).await;
        }
    };

    futures::join!(usb_fut, demo_fut);
}
