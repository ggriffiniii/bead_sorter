#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Pull};
use embassy_rp::peripherals::{PIO0, USB};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::usb;
use embassy_time::{Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use panic_probe as _;
use static_cell::{ConstStaticCell, StaticCell};

mod camera;
mod neopixel;
mod servo;
mod sorter;
mod switch;

use crate::camera::ov7670::Ov7670;
use crate::neopixel::Neopixel;
use crate::servo::{Channel, Servo};
use crate::sorter::BeadSorter;
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

static USB_CDC_ACM_STATE: StaticCell<State> = StaticCell::new();
static USB_CONFIG_DESC_BUF: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0u8; 256]);
static USB_BOS_DESC_BUF: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0u8; 256]);
static USB_CONTROL_BUF_BUF: ConstStaticCell<[u8; 64]> = ConstStaticCell::new([0u8; 64]);
static USB_MSOS_DESC_BUF: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0u8; 256]);
static USB_DATA_CDC_ACM_STATE: StaticCell<State> = StaticCell::new();

#[embassy_executor::task]
async fn usb_defmt_logger(
    mut driver: embassy_usb::UsbDevice<'static, embassy_rp::usb::Driver<'static, USB>>,
    tx: embassy_usb::class::cdc_acm::Sender<'static, embassy_rp::usb::Driver<'static, USB>>,
) {
    join(driver.run(), defmt_embassy_usbserial::logger(tx)).await;
}

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

    let state = USB_CDC_ACM_STATE.init(State::new());

    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        USB_CONFIG_DESC_BUF.take(),
        USB_BOS_DESC_BUF.take(),
        USB_MSOS_DESC_BUF.take(),
        USB_CONTROL_BUF_BUF.take(),
    );

    let mut class = CdcAcmClass::new(&mut builder, state, 64);
    let (tx, _rx) = class.split();

    let data_state = USB_DATA_CDC_ACM_STATE.init(State::new());
    let data_class = CdcAcmClass::new(&mut builder, data_state, 64);
    let (mut data_tx, _data_rx) = data_class.split();

    let usb = builder.build();
    spawner.must_spawn(usb_defmt_logger(usb, tx));

    defmt::info!("USB Logging initialized");

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
    let switch = Switch::new(pause_input);

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
    let i2c =
        embassy_rp::i2c::I2c::new_async(board.i2c0, board.i2c_scl, board.i2c_sda, Irqs, i2c_config);

    // --- Tasks ---
    let main_fut = async {
        Timer::after(Duration::from_millis(10000)).await;
        //panic!("test panic");
        // Ensure LED is ON (50%)
        led.set_config(&led_config);

        // Homing
        let chutes_fut = chutes.move_to(CHUTE_SLICE_POSITIONS[7]);
        let hopper_align_fut = async {
            hopper.move_to(HOPPER_DROP_POS).await;
            Timer::after(Duration::from_millis(300)).await;
        };
        join(chutes_fut, hopper_align_fut).await;

        // Initialize Ov7670 Camera
        let mut camera = Ov7670::new(
            i2c,
            &mut pio.common,
            pio.sm1,
            board.cam_dma,
            board.camera_mclk_pwm,
            board.cam_pins,
        )
        .await;

        // Sorting State
        let mut sorter = BeadSorter::new();

        loop {
            if switch.is_active() {
                // Paused
                // Turn OFF LED when paused
                led_config.compare_b = 0;
                led.set_config(&led_config);
                defmt::info!("Paused");
                Timer::after(Duration::from_millis(1000)).await;
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

            // Safety: Transmuting valid u32 slice to u8 slice.
            // The helper function keeps the lifetimes tied together.
            unsafe fn u32_slice_to_u8_slice(input: &[u32]) -> &[u8] {
                unsafe { core::slice::from_raw_parts(input.as_ptr() as *const u8, input.len() * 4) }
            }
            let buf_bytes = unsafe { u32_slice_to_u8_slice(&buf) };

            if data_tx.dtr() {
                // If host is connected to second ACM port, send image data
                // Image data is a magic u32 followed by 1200 bytes of rgb565
                // (30x40 pixels)
                let header = [0xBE, 0xAD, 0x1F, 0x01];
                let _ = data_tx.write_packet(&header).await;

                // Write in chunks to avoid overwhelming USB buffer if necessary
                for chunk in buf_bytes.chunks(64) {
                    let _ = data_tx.write_packet(chunk).await;
                }
            }

            let tube_index = sorter.get_tube_for_image(buf_bytes, 40, 30).unwrap_or(0);
            let chute_target = get_chute_pos(tube_index);

            let row_index = ((tube_index / 15) << 1) | ((tube_index % 15) & 1);
            defmt::info!(
                "Dropping bead into tube: {} row: {} chute: {}",
                tube_index,
                row_index,
                chute_target
            );
            let drop_row = HOPPER_ROW_POSITIONS[row_index as usize];

            let chutes_fut = chutes.move_to(chute_target);
            let hopper_align_fut = async {
                hopper.move_to(drop_row).await;
                Timer::after(Duration::from_millis(200)).await;
            };

            join(chutes_fut, hopper_align_fut).await;

            hopper.move_to(HOPPER_DROP_POS).await;
            Timer::after(Duration::from_millis(350)).await;
        }
    };

    main_fut.await
}
