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
        mclk_config.top = 6; // ~17.8 MHz
        mclk_config.compare_a = 3; // Duty cycle 50%
        let mclk_pwm = Pwm::new_output_a(mclk_slice, pins.mclk, mclk_config);

        // 2. Initialize SCCB
        let mut sccb_ctrl = Sccb::new(i2c);

        // Soft Reset
        sccb_ctrl.write_reg(reg::COM7, COM7_RESET).await.ok();
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
        match sccb_ctrl.read_reg(reg::PID).await {
            Ok(pid) => {
                defmt::info!("OV7670 PID: 0x{:02x}", pid);
            }
            Err(_) => {
                defmt::error!("OV7670 PID Read Failed!");
            }
        }

        // 3. Initialize DVP (PIO)
        // Pass pins individually; Dvp::new handles conversion to PioPin
        let dvp = Dvp::new(
            pio, sm, pins.d0, pins.d1, pins.d2, pins.d3, pins.d4, pins.d5, pins.d6, pins.d7,
            pins.pclk, pins.href, pins.vsync,
        );

        Self {
            dvp,
            sccb: sccb_ctrl,
            dma,
            _mclk_pwm: mclk_pwm,
        }
    }

    pub async fn capture(&mut self, buf: &mut [u32]) -> Result<(), ()> {
        // 1. Prepare DVP (PIO)
        self.dvp.prepare_capture();
        self.dvp
            .rx()
            .dma_pull(self.dma.reborrow(), buf, false)
            .await;
        self.dvp.stop();
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn enable_test_pattern(&mut self) {
        // Enable Color Bar Test Pattern (Bit 7 of SCALING_XSC and SCALING_YSC)
        // Assuming DIV16 40x30 config (0x40 base).
        let val = 0x40 | 0x80;
        let _ = self.sccb.write_reg(reg::SCALING_YSC, val).await;
        let _ = self.sccb.write_reg(reg::SCALING_XSC, val).await;
    }
}

#[allow(dead_code)]
pub mod reg {
    // Register Addresses
    pub const GAIN: u8 = 0x00;
    pub const BLUE: u8 = 0x01;
    pub const RED: u8 = 0x02;
    pub const VREF: u8 = 0x03;
    pub const COM1: u8 = 0x04;
    pub const BAVE: u8 = 0x05;
    pub const GB_AVE: u8 = 0x06;
    pub const AECHH: u8 = 0x07;
    pub const RAVE: u8 = 0x08;
    pub const COM2: u8 = 0x09;
    pub const PID: u8 = 0x0A;
    pub const VER: u8 = 0x0B;
    pub const COM3: u8 = 0x0C;
    pub const COM4: u8 = 0x0D;
    pub const COM5: u8 = 0x0E;
    pub const COM6: u8 = 0x0F;
    pub const AECH: u8 = 0x10;
    pub const CLKRC: u8 = 0x11;
    pub const COM7: u8 = 0x12;
    pub const COM8: u8 = 0x13;
    pub const COM9: u8 = 0x14;
    pub const COM10: u8 = 0x15;
    pub const HSTART: u8 = 0x17;
    pub const HSTOP: u8 = 0x18;
    pub const VSTART: u8 = 0x19;
    pub const VSTOP: u8 = 0x1A;
    pub const PSHFT: u8 = 0x1B;
    pub const MIDH: u8 = 0x1C;
    pub const MIDL: u8 = 0x1D;
    pub const MVFP: u8 = 0x1E;
    pub const LAEC: u8 = 0x1F;
    pub const ADCCTR0: u8 = 0x20;
    pub const ADCCTR1: u8 = 0x21;
    pub const ADCCTR2: u8 = 0x22;
    pub const ADCCTR3: u8 = 0x23;
    pub const AEW: u8 = 0x24;
    pub const AEB: u8 = 0x25;
    pub const VPT: u8 = 0x26;
    pub const BBIAS: u8 = 0x27;
    pub const GB_BIAS: u8 = 0x28;
    pub const EXHCH: u8 = 0x2A;
    pub const EXHCL: u8 = 0x2B;
    pub const RBIAS: u8 = 0x2C;
    pub const ADVFL: u8 = 0x2D;
    pub const ADVFH: u8 = 0x2E;
    pub const YAVE: u8 = 0x2F;
    pub const HSYST: u8 = 0x30;
    pub const HSYEN: u8 = 0x31;
    pub const HREF: u8 = 0x32;
    pub const CHLF: u8 = 0x33;
    pub const ARBLM: u8 = 0x34;
    pub const ADC: u8 = 0x37;
    pub const ACOM: u8 = 0x38;
    pub const OFON: u8 = 0x39;
    pub const TSLB: u8 = 0x3A;
    pub const COM11: u8 = 0x3B;
    pub const COM12: u8 = 0x3C;
    pub const COM13: u8 = 0x3D;
    pub const COM14: u8 = 0x3E;
    pub const EDGE: u8 = 0x3F;
    pub const COM15: u8 = 0x40;
    pub const COM16: u8 = 0x41;
    pub const COM17: u8 = 0x42;
    pub const AWBC1: u8 = 0x43;
    pub const AWBC2: u8 = 0x44;
    pub const AWBC3: u8 = 0x45;
    pub const AWBC4: u8 = 0x46;
    pub const AWBC5: u8 = 0x47;
    pub const AWBC6: u8 = 0x48;
    pub const REG4B: u8 = 0x4B;
    pub const DNSTH: u8 = 0x4C;
    pub const MTX1: u8 = 0x4F;
    pub const MTX2: u8 = 0x50;
    pub const MTX3: u8 = 0x51;
    pub const MTX4: u8 = 0x52;
    pub const MTX5: u8 = 0x53;
    pub const MTX6: u8 = 0x54;
    pub const BRIGHT: u8 = 0x55;
    pub const CONTRAS: u8 = 0x56;
    pub const CONTRAS_CENTER: u8 = 0x57;
    pub const MTXS: u8 = 0x58;
    pub const LCC1: u8 = 0x62;
    pub const LCC2: u8 = 0x63;
    pub const LCC3: u8 = 0x64;
    pub const LCC4: u8 = 0x65;
    pub const LCC5: u8 = 0x66;
    pub const MANU: u8 = 0x67;
    pub const MANV: u8 = 0x68;
    pub const GFIX: u8 = 0x69;
    pub const GGAIN: u8 = 0x6A;
    pub const DBLV: u8 = 0x6B;
    pub const AWBCTR3: u8 = 0x6C;
    pub const AWBCTR2: u8 = 0x6D;
    pub const AWBCTR1: u8 = 0x6E;
    pub const AWBCTR0: u8 = 0x6F;
    pub const SCALING_XSC: u8 = 0x70;
    pub const SCALING_YSC: u8 = 0x71;
    pub const SCALING_DCWCTR: u8 = 0x72;
    pub const SCALING_PCLK_DIV: u8 = 0x73;
    pub const REG74: u8 = 0x74;
    pub const REG76: u8 = 0x76;
    pub const SLOP: u8 = 0x7A;
    pub const GAM_BASE: u8 = 0x7B;
    pub const RGB444: u8 = 0x8C;
    pub const DM_LNL: u8 = 0x92;
    pub const LCC6: u8 = 0x94;
    pub const LCC7: u8 = 0x95;
    pub const HAECC1: u8 = 0x9F;
    pub const HAECC2: u8 = 0xA0;
    pub const SCALING_PCLK_DELAY: u8 = 0xA2;
    pub const BD50MAX: u8 = 0xA5;
    pub const HAECC3: u8 = 0xA6;
    pub const HAECC4: u8 = 0xA7;
    pub const HAECC5: u8 = 0xA8;
    pub const HAECC6: u8 = 0xA9;
    pub const HAECC7: u8 = 0xAA;
    pub const BD60MAX: u8 = 0xAB;
    pub const ABLC1: u8 = 0xB1;
    pub const THL_ST: u8 = 0xB3;
    pub const SATCTR: u8 = 0xC9;
}

// Bit Constants
const COM7_RESET: u8 = 0x80;
const COM7_RGB: u8 = 0x04;
#[allow(dead_code)]
const COM7_QCIF: u8 = 0x08;
const COM15_RGB565: u8 = 0x10;
const COM15_R00FF: u8 = 0xC0;
const COM3_DCWEN: u8 = 0x04;
const COM3_SCALEEN: u8 = 0x08;

// CircuitPython Initialization Sequence (Magic Numbers included)
pub const ADAFRUIT_OV7670_INIT: &[Register] = &[
    Register::new(reg::TSLB, 0x04),  // YLAST, No auto window
    Register::new(reg::COM10, 0x02), // VS_NEG (VSYNC Negative)
    Register::new(reg::SLOP, 0x20),
    Register::new(reg::GAM_BASE, 0x1C),
    Register::new(reg::GAM_BASE + 1, 0x28),
    Register::new(reg::GAM_BASE + 2, 0x3C),
    Register::new(reg::GAM_BASE + 3, 0x55),
    Register::new(reg::GAM_BASE + 4, 0x68),
    Register::new(reg::GAM_BASE + 5, 0x76),
    Register::new(reg::GAM_BASE + 6, 0x80),
    Register::new(reg::GAM_BASE + 7, 0x88),
    Register::new(reg::GAM_BASE + 8, 0x8F),
    Register::new(reg::GAM_BASE + 9, 0x96),
    Register::new(reg::GAM_BASE + 10, 0xA3),
    Register::new(reg::GAM_BASE + 11, 0xAF),
    Register::new(reg::GAM_BASE + 12, 0xC4),
    Register::new(reg::GAM_BASE + 13, 0xD7),
    Register::new(reg::GAM_BASE + 14, 0xE8),
    Register::new(reg::COM8, 0xC0 | 0x20), // FASTAEC, AECSTEP, BANDING
    Register::new(reg::GAIN, 0x00),
    Register::new(reg::COM2, 0x00), // Output Drive Capability 1x
    Register::new(reg::COM4, 0x00),
    Register::new(reg::COM9, 0x20), // Max AGC
    Register::new(reg::BD50MAX, 0x05),
    Register::new(reg::BD60MAX, 0x07),
    Register::new(reg::AEW, 0x75),
    Register::new(reg::AEB, 0x63),
    Register::new(reg::VPT, 0xA5),
    Register::new(reg::HAECC1, 0x78),
    Register::new(reg::HAECC2, 0x68),
    Register::new(0xA1, 0x03),
    Register::new(reg::HAECC3, 0xDF),
    Register::new(reg::HAECC4, 0xDF),
    Register::new(reg::HAECC5, 0xF0),
    Register::new(reg::HAECC6, 0x90),
    Register::new(reg::HAECC7, 0x94),
    Register::new(reg::COM8, 0xC0 | 0x20 | 0x04 | 0x01), // + AGC, AEC (No AWB)
    Register::new(reg::COM5, 0x61),
    Register::new(reg::COM6, 0x4B),
    Register::new(0x16, 0x02),
    Register::new(reg::MVFP, 0x07),
    Register::new(reg::ADCCTR1, 0x02),
    Register::new(reg::ADCCTR2, 0x91),
    Register::new(0x29, 0x07),
    Register::new(reg::CHLF, 0x0B),
    Register::new(0x35, 0x0B),
    Register::new(reg::ADC, 0x1D),
    Register::new(reg::ACOM, 0x71),
    Register::new(reg::OFON, 0x2A),
    Register::new(reg::COM12, 0x78),
    Register::new(0x4D, 0x40),
    Register::new(0x4E, 0x20),
    Register::new(reg::GFIX, 0x5D),
    Register::new(reg::REG74, 0x19),
    Register::new(0x8D, 0x4F),
    Register::new(0x8E, 0x00),
    Register::new(0x8F, 0x00),
    Register::new(0x90, 0x00),
    Register::new(0x91, 0x00),
    Register::new(reg::DM_LNL, 0x00),
    Register::new(0x96, 0x00),
    Register::new(0x9A, 0x80),
    Register::new(0xB0, 0x84),
    Register::new(reg::ABLC1, 0x0C),
    Register::new(0xB2, 0x0E),
    Register::new(reg::THL_ST, 0x82),
    Register::new(0xB8, 0x0A),
    Register::new(reg::AWBC1, 0x14),
    Register::new(reg::AWBC2, 0xF0),
    Register::new(reg::AWBC3, 0x34),
    Register::new(reg::AWBC4, 0x58),
    Register::new(reg::AWBC5, 0x28),
    Register::new(reg::AWBC6, 0x3A),
    Register::new(0x59, 0x88),
    Register::new(0x5A, 0x88),
    Register::new(0x5B, 0x44),
    Register::new(0x5C, 0x67),
    Register::new(0x5D, 0x49),
    Register::new(0x5E, 0x0E),
    Register::new(reg::LCC3, 0x04),
    Register::new(reg::LCC4, 0x20),
    Register::new(reg::LCC5, 0x05),
    Register::new(reg::LCC6, 0x04),
    Register::new(reg::LCC7, 0x08),
    Register::new(reg::AWBCTR3, 0x0A),
    Register::new(reg::AWBCTR2, 0x55),
    Register::new(reg::MTX1, 0x80),
    Register::new(reg::MTX2, 0x80),
    Register::new(reg::MTX3, 0x00),
    Register::new(reg::MTX4, 0x22),
    Register::new(reg::MTX5, 0x5E),
    Register::new(reg::MTX6, 0x80),
    Register::new(reg::AWBCTR1, 0x11),
    Register::new(reg::AWBCTR0, 0x9F),
    Register::new(reg::BRIGHT, 0x00),
    Register::new(reg::CONTRAS, 0x40),
    Register::new(reg::CONTRAS_CENTER, 0x80),
    Register::new(reg::MVFP, 0x37), // flip X and Y
];

// 40x30 Configuration (DIV16) derived from _frame_control in CircuitPython
// size = 4 (DIV16)
// window = [15, 252, 3, 2] (vstart=15, hstart=252, edge=3, pclk_delay=2)
pub const OV7670_DIV16_40X30: &[Register] = &[
    // COM3: Enable DCW and Scale
    Register::new(reg::COM3, COM3_DCWEN | COM3_SCALEEN),
    // COM14: 0x18 + 4 = 0x1C (Enable PCLK Divider)
    Register::new(reg::COM14, 0x1C),
    // SCALING_DCWCTR: 3 * 0x11 = 0x33
    Register::new(reg::SCALING_DCWCTR, 0x33),
    // SCALING_PCLK_DIV: 0xF0 + 4 = 0xF4 (Enable PCLK Divider /16)
    Register::new(reg::SCALING_PCLK_DIV, 0xF4),
    // SCALING_XSC / YSC
    // CircuitPython Reads current and applies 0x40 (0.5 zoom) for DIV16
    // Since we are resetting, we assume default 0x3A.
    // However, Adafruit says test pattern settings are stored there.
    // Let's assume we want 0x3A | 0x40 logic?
    // Actually, defaults are approx 0x3A. With zoom 0.5 (0x40), we want ~0x7A?
    // Let's use what CircuitPython likely results in for Normal operation.
    // xsc = (0 & 0x80) | 0x40 = 0x40.
    // ysc = (0 & 0x80) | 0x40 = 0x40.
    Register::new(reg::SCALING_XSC, 0x40),
    Register::new(reg::SCALING_YSC, 0x40),
    // Windowing for DIV16
    // vstart=15, vstop=15+480=495
    // hstart=252, hstop=(252+640)%784 = 108
    // edge=3

    // HSTART = 252 >> 3 = 31 (0x1F)
    Register::new(reg::HSTART, 0x1F),
    // HSTOP = 108 >> 3 = 13 (0x0D)
    Register::new(reg::HSTOP, 0x0D),
    // HREF = (3 << 6) | ((108&7)<<3) | (252&7)
    // 108&7 = 4. 252&7 = 4.
    // HREF = 0xC0 | 0x20 | 0x04 = 0xE4
    Register::new(reg::HREF, 0xE4),
    // VSTART = 15 >> 2 = 3
    Register::new(reg::VSTART, 0x03),
    // VSTOP = 495 >> 2 = 123 (0x7B)
    Register::new(reg::VSTOP, 0x7B),
    // VREF = ((495&3)<<2) | (15&3)
    // 495&3 = 3. 15&3 = 3.
    // VREF = 0x0C | 0x03 = 0x0F
    Register::new(reg::VREF, 0x0F),
    // SCALING_PCLK_DELAY = 2
    Register::new(reg::SCALING_PCLK_DELAY, 0x02),
];

pub const OV7670_RGB565: &[Register] = &[
    Register::new(reg::COM7, COM7_RGB),                    // RGB
    Register::new(reg::RGB444, 0x00),                      // Disable RGB444
    Register::new(reg::COM15, COM15_RGB565 | COM15_R00FF), // RGB565, Full Range
];
