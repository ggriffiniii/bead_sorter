use embassy_rp::dma::Channel;
use embassy_rp::i2c::{Async, I2c, Instance as I2cInstance};
use embassy_rp::peripherals::PWM_SLICE4;
use embassy_rp::pio::{Common, Instance as PioInstance, StateMachine};
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::Peri;
// use embedded_hal_async::i2c::I2c as I2cTrait; // Unused

use crate::camera::dvp::Dvp;
use crate::camera::sccb::Sccb;
use bead_sorter_bsp::OVCamPins;

#[derive(Clone, Copy)]
pub struct Register {
    pub addr: u8,
    pub val: u8,
}

impl Register {
    pub const fn new(addr: u8, val: u8) -> Self {
        Self { addr, val }
    }
}

pub struct Ov7670<'d, PIO: PioInstance, I2C: I2cInstance, DMA: Channel, const SM: usize> {
    dvp: Dvp<'d, PIO, SM>,
    sccb: Sccb<'d, I2C>,
    dma: Peri<'d, DMA>,
    _mclk_pwm: Pwm<'d>,
}

impl<'d, PIO: PioInstance, I2C: I2cInstance, DMA: Channel, const SM: usize>
    Ov7670<'d, PIO, I2C, DMA, SM>
{
    pub async fn new(
        i2c: I2c<'d, I2C, Async>,
        pio: &mut Common<'d, PIO>,
        sm: StateMachine<'d, PIO, SM>,
        dma: Peri<'d, DMA>,
        mclk_slice: Peri<'d, PWM_SLICE4>,
        pins: OVCamPins,
    ) -> Self {
        // 1. Initialize MCLK (PWM)
        let mut mclk_config = PwmConfig::default();
        mclk_config.divider = fixed::FixedU16::from_num(1);
        mclk_config.top = 6;
        mclk_config.compare_a = 3; // Duty cycle 50%
        let mclk_pwm = Pwm::new_output_a(mclk_slice, pins.mclk, mclk_config);

        // 2. Initialize SCCB
        let mut sccb_ctrl = Sccb::new(i2c);

        // Soft Reset
        sccb_ctrl.write_reg(REG_COM7, COM7_RESET).await.ok();
        embassy_time::Timer::after(embassy_time::Duration::from_millis(100)).await;

        // Write Init Sequence
        for reg in ADAFRUIT_OV7670_INIT {
            sccb_ctrl.write_reg(reg.addr, reg.val).await.ok();
            embassy_time::Timer::after(embassy_time::Duration::from_micros(1000)).await;
        }

        for reg in OV7670_RGB565 {
            sccb_ctrl.write_reg(reg.addr, reg.val).await.ok();
            embassy_time::Timer::after(embassy_time::Duration::from_micros(1000)).await;
        }

        for reg in OV7670_DIV16_40X30 {
            sccb_ctrl.write_reg(reg.addr, reg.val).await.ok();
            embassy_time::Timer::after(embassy_time::Duration::from_micros(1000)).await;
        }

        // Wait for AEC/AGC to settle
        embassy_time::Timer::after(embassy_time::Duration::from_millis(500)).await;

        // Verify PID (0x76)
        match sccb_ctrl.read_reg(REG_PID).await {
            Ok(pid) => {
                defmt::info!("OV7670 PID: 0x{:02x}", pid);
            }
            Err(_) => {
                defmt::error!("OV7670 PID Read Failed!");
            }
        }

        // 2. Initialize DVP (PIO)
        let d0 = pio.make_pio_pin(pins.d0);
        let d1 = pio.make_pio_pin(pins.d1);
        let d2 = pio.make_pio_pin(pins.d2);
        let d3 = pio.make_pio_pin(pins.d3);
        let d4 = pio.make_pio_pin(pins.d4);
        let d5 = pio.make_pio_pin(pins.d5);
        let d6 = pio.make_pio_pin(pins.d6);
        let d7 = pio.make_pio_pin(pins.d7);
        let pclk = pio.make_pio_pin(pins.pclk);
        let href = pio.make_pio_pin(pins.href);
        let vsync = pio.make_pio_pin(pins.vsync);

        let dvp = Dvp::new(pio, sm, d0, d1, d2, d3, d4, d5, d6, d7, pclk, href, vsync);

        Self {
            dvp,
            sccb: sccb_ctrl,
            dma,
            _mclk_pwm: mclk_pwm,
        }
    }

    pub async fn capture(&mut self, buf: &mut [u32]) {
        self.dvp.prepare_capture();
        self.dvp
            .rx()
            .dma_pull(self.dma.reborrow(), buf, false)
            .await;
    }
}

#[allow(dead_code, non_upper_case_globals)]
// Register Addresses
const REG_GAIN: u8 = 0x00;
const REG_BLUE: u8 = 0x01;
const REG_RED: u8 = 0x02;
const REG_VREF: u8 = 0x03;
const REG_COM1: u8 = 0x04;
const REG_BAVE: u8 = 0x05;
const REG_GbAVE: u8 = 0x06;
const REG_AECHH: u8 = 0x07;
const REG_RAVE: u8 = 0x08;
const REG_COM2: u8 = 0x09;
const REG_PID: u8 = 0x0A;
const REG_VER: u8 = 0x0B;
const REG_COM3: u8 = 0x0C;
const REG_COM4: u8 = 0x0D;
const REG_COM5: u8 = 0x0E;
const REG_COM6: u8 = 0x0F;
const REG_AECH: u8 = 0x10;
const REG_CLKRC: u8 = 0x11;
const REG_COM7: u8 = 0x12;
const REG_COM8: u8 = 0x13;
const REG_COM9: u8 = 0x14;
const REG_COM10: u8 = 0x15;
const REG_HSTART: u8 = 0x17;
const REG_HSTOP: u8 = 0x18;
const REG_VSTART: u8 = 0x19;
const REG_VSTOP: u8 = 0x1A;
const REG_PSHFT: u8 = 0x1B;
const REG_MIDH: u8 = 0x1C;
const REG_MIDL: u8 = 0x1D;
const REG_MVFP: u8 = 0x1E;
const REG_LAEC: u8 = 0x1F;
const REG_ADCCTR0: u8 = 0x20;
const REG_ADCCTR1: u8 = 0x21;
const REG_ADCCTR2: u8 = 0x22;
const REG_ADCCTR3: u8 = 0x23;
const REG_AEW: u8 = 0x24;
const REG_AEB: u8 = 0x25;
const REG_VPT: u8 = 0x26;
const REG_BBIAS: u8 = 0x27;
const REG_GbBIAS: u8 = 0x28;
const REG_EXHCH: u8 = 0x2A;
const REG_EXHCL: u8 = 0x2B;
const REG_RBIAS: u8 = 0x2C;
const REG_ADVFL: u8 = 0x2D;
const REG_ADVFH: u8 = 0x2E;
const REG_YAVE: u8 = 0x2F;
const REG_HSYST: u8 = 0x30;
const REG_HSYEN: u8 = 0x31;
const REG_HREF: u8 = 0x32;
const REG_CHLF: u8 = 0x33;
const REG_ARBLM: u8 = 0x34;
const REG_ADC: u8 = 0x37;
const REG_ACOM: u8 = 0x38;
const REG_OFON: u8 = 0x39;
const REG_TSLB: u8 = 0x3A;
const REG_COM11: u8 = 0x3B;
const REG_COM12: u8 = 0x3C;
const REG_COM13: u8 = 0x3D;
const REG_COM14: u8 = 0x3E;
const REG_EDGE: u8 = 0x3F;
const REG_COM15: u8 = 0x40;
const REG_COM16: u8 = 0x41;
const REG_COM17: u8 = 0x42;
const REG_AWBC1: u8 = 0x43;
const REG_AWBC2: u8 = 0x44;
const REG_AWBC3: u8 = 0x45;
const REG_AWBC4: u8 = 0x46;
const REG_AWBC5: u8 = 0x47;
const REG_AWBC6: u8 = 0x48;
const REG_REG4B: u8 = 0x4B;
const REG_DNSTH: u8 = 0x4C;
const REG_MTX1: u8 = 0x4F;
const REG_MTX2: u8 = 0x50;
const REG_MTX3: u8 = 0x51;
const REG_MTX4: u8 = 0x52;
const REG_MTX5: u8 = 0x53;
const REG_MTX6: u8 = 0x54;
const REG_BRIGHT: u8 = 0x55;
const REG_CONTRAS: u8 = 0x56;
const REG_CONTRAS_CENTER: u8 = 0x57;
const REG_MTXS: u8 = 0x58;
const REG_LCC1: u8 = 0x62;
const REG_LCC2: u8 = 0x63;
const REG_LCC3: u8 = 0x64;
const REG_LCC4: u8 = 0x65;
const REG_LCC5: u8 = 0x66;
const REG_MANU: u8 = 0x67;
const REG_MANV: u8 = 0x68;
const REG_GFIX: u8 = 0x69;
const REG_GGAIN: u8 = 0x6A;
const REG_DBLV: u8 = 0x6B;
const REG_AWBCTR3: u8 = 0x6C;
const REG_AWBCTR2: u8 = 0x6D;
const REG_AWBCTR1: u8 = 0x6E;
const REG_AWBCTR0: u8 = 0x6F;
const REG_SCALING_XSC: u8 = 0x70;
const REG_SCALING_YSC: u8 = 0x71;
const REG_SCALING_DCWCTR: u8 = 0x72;
const REG_SCALING_PCLK_DIV: u8 = 0x73;
const REG_REG74: u8 = 0x74;
const REG_REG76: u8 = 0x76;
const REG_SLOP: u8 = 0x7A;
const REG_GAM_BASE: u8 = 0x7B;
const REG_RGB444: u8 = 0x8C;
const REG_DM_LNL: u8 = 0x92;
const REG_LCC6: u8 = 0x94;
const REG_LCC7: u8 = 0x95;
const REG_HAECC1: u8 = 0x9F;
const REG_HAECC2: u8 = 0xA0;
const REG_SCALING_PCLK_DELAY: u8 = 0xA2;
const REG_BD50MAX: u8 = 0xA5;
const REG_HAECC3: u8 = 0xA6;
const REG_HAECC4: u8 = 0xA7;
const REG_HAECC5: u8 = 0xA8;
const REG_HAECC6: u8 = 0xA9;
const REG_HAECC7: u8 = 0xAA;
const REG_BD60MAX: u8 = 0xAB;
const REG_ABLC1: u8 = 0xB1;
const REG_THL_ST: u8 = 0xB3;
const REG_SATCTR: u8 = 0xC9;

// Bit Constants
const COM7_RESET: u8 = 0x80;
const COM7_RGB: u8 = 0x04;
const COM7_QCIF: u8 = 0x08;
const COM15_RGB565: u8 = 0x10;
const COM15_R00FF: u8 = 0xC0;
const COM3_DCWEN: u8 = 0x04;
const COM3_SCALEEN: u8 = 0x08;

// CircuitPython Initialization Sequence (Magic Numbers included)
pub const ADAFRUIT_OV7670_INIT: &[Register] = &[
    Register::new(REG_TSLB, 0x04),  // YLAST, No auto window
    Register::new(REG_COM10, 0x02), // VS_NEG (VSYNC Negative)
    Register::new(REG_SLOP, 0x20),
    Register::new(REG_GAM_BASE, 0x1C),
    Register::new(REG_GAM_BASE + 1, 0x28),
    Register::new(REG_GAM_BASE + 2, 0x3C),
    Register::new(REG_GAM_BASE + 3, 0x55),
    Register::new(REG_GAM_BASE + 4, 0x68),
    Register::new(REG_GAM_BASE + 5, 0x76),
    Register::new(REG_GAM_BASE + 6, 0x80),
    Register::new(REG_GAM_BASE + 7, 0x88),
    Register::new(REG_GAM_BASE + 8, 0x8F),
    Register::new(REG_GAM_BASE + 9, 0x96),
    Register::new(REG_GAM_BASE + 10, 0xA3),
    Register::new(REG_GAM_BASE + 11, 0xAF),
    Register::new(REG_GAM_BASE + 12, 0xC4),
    Register::new(REG_GAM_BASE + 13, 0xD7),
    Register::new(REG_GAM_BASE + 14, 0xE8),
    Register::new(REG_COM8, 0xC0 | 0x20), // FASTAEC, AECSTEP, BANDING
    Register::new(REG_GAIN, 0x00),
    Register::new(REG_COM2, 0x00), // Output Drive Capability 1x, NO S-Sleep
    Register::new(REG_COM4, 0x00),
    Register::new(REG_COM9, 0x20), // Max AGC
    Register::new(REG_BD50MAX, 0x05),
    Register::new(REG_BD60MAX, 0x07),
    Register::new(REG_AEW, 0x75),
    Register::new(REG_AEB, 0x63),
    Register::new(REG_VPT, 0xA5),
    Register::new(REG_HAECC1, 0x78),
    Register::new(REG_HAECC2, 0x68),
    Register::new(0xA1, 0x03),
    Register::new(REG_HAECC3, 0xDF),
    Register::new(REG_HAECC4, 0xDF),
    Register::new(REG_HAECC5, 0xF0),
    Register::new(REG_HAECC6, 0x90),
    Register::new(REG_HAECC7, 0x94),
    Register::new(REG_COM8, 0xC0 | 0x20 | 0x04 | 0x01), // + AGC, AEC
    Register::new(REG_COM5, 0x61),
    Register::new(REG_COM6, 0x4B),
    Register::new(0x16, 0x02),
    Register::new(REG_MVFP, 0x07),
    Register::new(REG_ADCCTR1, 0x02),
    Register::new(REG_ADCCTR2, 0x91),
    Register::new(0x29, 0x07),
    Register::new(REG_CHLF, 0x0B),
    Register::new(0x35, 0x0B),
    Register::new(REG_ADC, 0x1D),
    Register::new(REG_ACOM, 0x71),
    Register::new(REG_OFON, 0x2A),
    Register::new(REG_COM12, 0x78),
    Register::new(0x4D, 0x40),
    Register::new(0x4E, 0x20),
    Register::new(REG_GFIX, 0x5D),
    Register::new(REG_REG74, 0x19),
    Register::new(0x8D, 0x4F),
    Register::new(0x8E, 0x00),
    Register::new(0x8F, 0x00),
    Register::new(0x90, 0x00),
    Register::new(0x91, 0x00),
    Register::new(REG_DM_LNL, 0x00),
    Register::new(0x96, 0x00),
    Register::new(0x9A, 0x80),
    Register::new(0xB0, 0x84),
    Register::new(REG_ABLC1, 0x0C),
    Register::new(0xB2, 0x0E),
    Register::new(REG_THL_ST, 0x82),
    Register::new(0xB8, 0x0A),
    Register::new(REG_AWBC1, 0x14),
    Register::new(REG_AWBC2, 0xF0),
    Register::new(REG_AWBC3, 0x34),
    Register::new(REG_AWBC4, 0x58),
    Register::new(REG_AWBC5, 0x28),
    Register::new(REG_AWBC6, 0x3A),
    Register::new(0x59, 0x88),
    Register::new(0x5A, 0x88),
    Register::new(0x5B, 0x44),
    Register::new(0x5C, 0x67),
    Register::new(0x5D, 0x49),
    Register::new(0x5E, 0x0E),
    Register::new(REG_LCC3, 0x04),
    Register::new(REG_LCC4, 0x20),
    Register::new(REG_LCC5, 0x05),
    Register::new(REG_LCC6, 0x04),
    Register::new(REG_LCC7, 0x08),
    Register::new(REG_AWBCTR3, 0x0A),
    Register::new(REG_AWBCTR2, 0x55),
    Register::new(REG_MTX1, 0x80),
    Register::new(REG_MTX2, 0x80),
    Register::new(REG_MTX3, 0x00),
    Register::new(REG_MTX4, 0x22),
    Register::new(REG_MTX5, 0x5E),
    Register::new(REG_MTX6, 0x80),
    Register::new(REG_AWBCTR1, 0x11),
    Register::new(REG_AWBCTR0, 0x9F),
    Register::new(REG_BRIGHT, 0x00),
    Register::new(REG_CONTRAS, 0x40),
    Register::new(REG_CONTRAS_CENTER, 0x80),
];

// 40x30 Configuration (DIV16) derived from _frame_control in CircuitPython
// size = 4 (DIV16)
// window = [15, 252, 3, 2] (vstart=15, hstart=252, edge=3, pclk_delay=2)
pub const OV7670_DIV16_40X30: &[Register] = &[
    // COM3: Enable DCW and Scale
    Register::new(REG_COM3, COM3_DCWEN | COM3_SCALEEN),
    // COM14: 0x18 + 4 = 0x1C (Enable PCLK Divider)
    Register::new(REG_COM14, 0x1C),
    // SCALING_DCWCTR: 4 * 0x11 = 0x44
    Register::new(REG_SCALING_DCWCTR, 0x44),
    // SCALING_PCLK_DIV: 0xF0 + 4 = 0xF4
    Register::new(REG_SCALING_PCLK_DIV, 0xF4),
    // SCALING_XSC / YSC
    // CircuitPython Reads current and applies 0x40 (0.5 zoom) for DIV16
    // Since we are resetting, we assume default 0x3A.
    // However, Adafruit says test pattern settings are stored there.
    // Let's assume we want 0x3A | 0x40 logic?
    // Actually, defaults are approx 0x3A. With zoom 0.5 (0x40), we want ~0x7A?
    // Let's use what CircuitPython likely results in for Normal operation.
    // xsc = (0 & 0x80) | 0x40 = 0x40.
    // ysc = (0 & 0x80) | 0x40 = 0x40.
    Register::new(REG_SCALING_XSC, 0x40),
    Register::new(REG_SCALING_YSC, 0x40),
    // Windowing for DIV16
    // vstart=15, vstop=15+480=495
    // hstart=252, hstop=(252+640)%784 = 108
    // edge=3

    // HSTART = 252 >> 3 = 31 (0x1F)
    Register::new(REG_HSTART, 0x1F),
    // HSTOP = 108 >> 3 = 13 (0x0D)
    Register::new(REG_HSTOP, 0x0D),
    // HREF = (3 << 6) | ((108&7)<<3) | (252&7)
    // 108&7 = 4. 252&7 = 4.
    // HREF = 0xC0 | 0x20 | 0x04 = 0xE4
    Register::new(REG_HREF, 0xE4),
    // VSTART = 15 >> 2 = 3
    Register::new(REG_VSTART, 0x03),
    // VSTOP = 495 >> 2 = 123 (0x7B)
    Register::new(REG_VSTOP, 0x7B),
    // VREF = ((495&3)<<2) | (15&3)
    // 495&3 = 3. 15&3 = 3.
    // VREF = 0x0C | 0x03 = 0x0F
    Register::new(REG_VREF, 0x0F),
    // SCALING_PCLK_DELAY = 2
    Register::new(REG_SCALING_PCLK_DELAY, 0x02),
];

pub const OV7670_RGB565: &[Register] = &[
    Register::new(REG_COM7, COM7_RGB),                    // RGB
    Register::new(REG_RGB444, 0x00),                      // Disable RGB444
    Register::new(REG_COM15, COM15_RGB565 | COM15_R00FF), // RGB565, Full Range
];
