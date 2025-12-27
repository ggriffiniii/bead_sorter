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

use crate::camera::dvp::Dvp; // Import Dvp
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

    // 6. Camera MCLK (PWM4 A -> 17.86 MHz)
    let mut mclk_config = PwmConfig::default();
    mclk_config.divider = fixed::FixedU16::from_num(1);
    mclk_config.top = 6;
    mclk_config.compare_a = 3;
    // Assuming Config has enable (checked docs: often does not, Pwm::new enables usually).
    // But let's check if we can remove the 'default' and struct init explicitly if needed.
    // Actually, safer: just rely on Pwm::new enabling it.
    // BUT! Pin 8 IE force is needed.
    let _mclk_pwm = Pwm::new_output_a(board.camera_mclk_pwm, board.cam_pins.mclk, mclk_config);

    // 7. Camera SCCB (I2C0)
    let mut i2c_config = embassy_rp::i2c::Config::default();
    i2c_config.frequency = 100_000;
    i2c_config.sda_pullup = false;
    i2c_config.scl_pullup = false;
    let mut i2c =
        embassy_rp::i2c::I2c::new_async(board.i2c0, board.i2c_scl, board.i2c_sda, Irqs, i2c_config);

    // 8. DVP Capture (PIO0 SM1)
    // Wait for camera to wake up (CLK stable)
    Timer::after(Duration::from_millis(500)).await;

    let d0_pin = pio.common.make_pio_pin(board.cam_pins.d0);
    let d1_pin = pio.common.make_pio_pin(board.cam_pins.d1);
    let d2_pin = pio.common.make_pio_pin(board.cam_pins.d2);
    let d3_pin = pio.common.make_pio_pin(board.cam_pins.d3);
    let d4_pin = pio.common.make_pio_pin(board.cam_pins.d4);
    let d5_pin = pio.common.make_pio_pin(board.cam_pins.d5);
    let d6_pin = pio.common.make_pio_pin(board.cam_pins.d6);
    let d7_pin = pio.common.make_pio_pin(board.cam_pins.d7);
    let pclk_pin = pio.common.make_pio_pin(board.cam_pins.pclk);
    let href_pin = pio.common.make_pio_pin(board.cam_pins.href);
    let vsync_pin = pio.common.make_pio_pin(board.cam_pins.vsync);

    let mut dvp = Dvp::new(
        &mut pio.common,
        pio.sm1,
        d0_pin,
        d1_pin,
        d2_pin,
        d3_pin,
        d4_pin,
        d5_pin,
        d6_pin,
        d7_pin,
        pclk_pin,
        href_pin,
        vsync_pin,
    );

    // Acquire Camera DMA
    let mut cam_dma = board.cam_dma;

    // --- I2C Scan Debug ---
    // Perform a full bus scan to see if *any* device responds.
    // This helps rule out wiring/address issues.

    // We defer the Sccb creation until after the scan logic in `demo_fut`.
    // But `demo_fut` needs to own `i2c` or `sccb`.
    // Let's create `sccb` *inside* `demo_fut` or pass `i2c` to it?
    // Actually, to avoid lifetime hell, let's keep `sccb` here but add a `scan` method
    // OR just use `i2c` briefly here? No, `main` is async but we are in setup.
    // Wait, `i2c` is async. We can't use it easily in the synchronous setup part.
    // We have to move it into the async block.

    // Let's change the structure slightly:
    // Move `sccb` creation INTO `demo_fut`.

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

        // Now wrap i2c in Sccb
        let mut sccb = crate::camera::sccb::Sccb::new(i2c);

        // Soft Reset (COM7 = 0x80)
        let _ = class.write_packet(b"Resetting OV7670...\r\n").await;
        // REG_COM7 is 0x12, COM7_RESET is 0x80
        if let Err(_e) = sccb.write_reg(0x12, 0x80).await {
            let _ = class.write_packet(b"Reset Failed (I2C Error)!\r\n").await;
        }
        Timer::after(Duration::from_millis(100)).await;

        // Verify PID
        let _ = class.write_packet(b"Checking PID (0x0A)...\r\n").await;
        let mut pid_val: Option<u8> = None;
        match with_timeout(Duration::from_millis(100), sccb.read_reg(0x0A)).await {
            Ok(Ok(val)) => {
                pid_val = Some(val);
                if val == 0x76 {
                    let _ = class.write_packet(b"PID Match: 0x76\r\n").await;
                } else {
                    let _ = class.write_packet(b"PID Mismatch!\r\n").await;
                }
            }
            Ok(Err(_)) => {
                let _ = class.write_packet(b"PID Error\r\n").await;
            }
            Err(_) => {
                let _ = class.write_packet(b"PID Timeout\r\n").await;
            }
        }

        // VISUAL ALERT: If PID failed, fast blink forever
        if pid_val.is_none() {
            let _ = class.write_packet(b"HALT: Camera Unresponsive.\r\n").await;
            loop {
                led.set_high();
                Timer::after(Duration::from_millis(50)).await;
                led.set_low();
                Timer::after(Duration::from_millis(50)).await;
            }
        }

        // Write Camera Configuration
        // Write Camera Configuration
        let _ = class
            .write_packet(b"Configuring Camera (DIV16 40x30)...\r\n")
            .await;

        let _ = sccb.write_reg(0x12, 0x80).await; // RESET
        Timer::after(Duration::from_millis(100)).await;

        for reg in crate::camera::ov7670::ADAFRUIT_OV7670_INIT {
            let _ = sccb.write_reg(reg.addr, reg.val).await;
            Timer::after(Duration::from_micros(1000)).await;
        }

        for reg in crate::camera::ov7670::OV7670_RGB565 {
            let _ = sccb.write_reg(reg.addr, reg.val).await;
        }

        for reg in crate::camera::ov7670::OV7670_DIV16_40X30 {
            let _ = sccb.write_reg(reg.addr, reg.val).await;
        }

        // Wait for AEC/AGC to settle
        Timer::after(Duration::from_millis(500)).await;

        let _ = class.write_packet(b"Camera Configured.\r\n").await;

        // --- DVP Capture Test ---
        // DVP Capture Test
        let _ = class
            .write_packet(b"Capturing 40x30 Frame (DMA)... \r\n")
            .await;

        let mut buf = [0u32; 600];

        dvp.prepare_capture();

        // DMA Capture
        dvp.rx().dma_pull(cam_dma.reborrow(), &mut buf, false).await;

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
