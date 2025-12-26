#![no_std]

pub use embassy_rp;
use embassy_rp::i2c;
use embassy_rp::peripherals;
use embassy_rp::Peri;
pub type Neopixel = Peri<'static, peripherals::PIN_20>;
pub type CameraLed = Peri<'static, peripherals::PIN_23>;
pub type PauseButton = Peri<'static, peripherals::PIN_19>;
pub type HopperServo = Peri<'static, peripherals::PIN_18>;
pub type ChutesServo = Peri<'static, peripherals::PIN_26>;

// I2C
pub type I2cData = peripherals::PIN_12;
pub type I2cClock = peripherals::PIN_13;
pub type I2c = i2c::I2c<'static, i2c::Blocking, peripherals::I2C0>;

// Camera
pub type CamD0 = peripherals::PIN_0;
pub type CamD1 = peripherals::PIN_1;
pub type CamD2 = peripherals::PIN_2;
pub type CamD3 = peripherals::PIN_3;
pub type CamD4 = peripherals::PIN_4;
pub type CamD5 = peripherals::PIN_5;
pub type CamD6 = peripherals::PIN_6;
pub type CamD7 = peripherals::PIN_7;
pub type CamMclk = peripherals::PIN_8;
pub type CamPclk = peripherals::PIN_9;
pub type CamHref = peripherals::PIN_10;
pub type CamVsync = peripherals::PIN_11;

pub struct OVCamPins {
    pub d0: Peri<'static, CamD0>,
    pub d1: Peri<'static, CamD1>,
    pub d2: Peri<'static, CamD2>,
    pub d3: Peri<'static, CamD3>,
    pub d4: Peri<'static, CamD4>,
    pub d5: Peri<'static, CamD5>,
    pub d6: Peri<'static, CamD6>,
    pub d7: Peri<'static, CamD7>,
    pub mclk: Peri<'static, CamMclk>,
    pub pclk: Peri<'static, CamPclk>,
    pub href: Peri<'static, CamHref>,
    pub vsync: Peri<'static, CamVsync>,
}

pub struct Board {
    pub neopixel: Neopixel,
    pub camera_led: CameraLed,
    pub pause_button: PauseButton,
    pub hopper_servo: HopperServo,
    pub chutes_servo: ChutesServo,

    pub neopixel_pio: Peri<'static, peripherals::PIO0>,
    pub neopixel_dma: Peri<'static, peripherals::DMA_CH0>,

    pub hopper_pwm: Peri<'static, peripherals::PWM_SLICE1>,
    pub chutes_pwm: Peri<'static, peripherals::PWM_SLICE5>,

    pub i2c0: Peri<'static, peripherals::I2C0>,
    pub i2c_sda: Peri<'static, I2cData>,
    pub i2c_scl: Peri<'static, I2cClock>,

    pub cam_pins: OVCamPins,

    pub usb: Peri<'static, peripherals::USB>,
}

impl Board {
    pub fn new(p: embassy_rp::Peripherals) -> Self {
        Self {
            neopixel: p.PIN_20,
            camera_led: p.PIN_23,
            pause_button: p.PIN_19,
            hopper_servo: p.PIN_18,
            chutes_servo: p.PIN_26,

            neopixel_pio: p.PIO0,
            neopixel_dma: p.DMA_CH0,
            hopper_pwm: p.PWM_SLICE1,
            chutes_pwm: p.PWM_SLICE5,

            i2c0: p.I2C0,
            i2c_sda: p.PIN_12,
            i2c_scl: p.PIN_13,

            cam_pins: OVCamPins {
                d0: p.PIN_0,
                d1: p.PIN_1,
                d2: p.PIN_2,
                d3: p.PIN_3,
                d4: p.PIN_4,
                d5: p.PIN_5,
                d6: p.PIN_6,
                d7: p.PIN_7,
                mclk: p.PIN_8,
                pclk: p.PIN_9,
                href: p.PIN_10,
                vsync: p.PIN_11,
            },

            usb: p.USB,
        }
    }
}
