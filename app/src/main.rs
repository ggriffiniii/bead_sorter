#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_time::{with_timeout, Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use panic_probe as _;

// Import our BSP
use bead_sorter_bsp::{self as bsp, Board};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let board = Board::new(p);

    // -- USB Setup --
    let driver = Driver::new(board.usb, Irqs);
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

    // -- Blink Task --
    let mut camera_led = Output::new(board.camera_led, Level::Low);

    let usb_fut = usb.run();

    let blink_fut = async {
        // Wait for USB enumeration roughly (or we can wait for DTR)
        Timer::after(Duration::from_millis(1000)).await;
        // ignore errors
        if class.dtr() {
            let _ = with_timeout(
                Duration::from_millis(5),
                class.write_packet("Firmware Started!\r\n".as_bytes()),
            )
            .await;
        }

        loop {
            camera_led.set_high();
            if class.dtr() {
                let _ = with_timeout(
                    Duration::from_millis(5),
                    class.write_packet("LED ON\r\n".as_bytes()),
                )
                .await;
            }
            Timer::after(Duration::from_millis(500)).await;

            camera_led.set_low();
            if class.dtr() {
                let _ = with_timeout(
                    Duration::from_millis(5),
                    class.write_packet("LED OFF\r\n".as_bytes()),
                )
                .await;
            }
            Timer::after(Duration::from_millis(500)).await;
        }
    };

    // Run both tasks concurrently
    futures::join!(usb_fut, blink_fut);
}
