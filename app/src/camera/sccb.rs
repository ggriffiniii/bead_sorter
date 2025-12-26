use embassy_rp::i2c::{I2c, Instance, Mode};
use embassy_rp::peripheral::Peripheral;

// OV7670 I2C Address (0x42 write / 0x43 read) -> 7-bit is 0x21
pub const CAM_ADDR: u8 = 0x21;

pub struct Sccb<'d, T: Instance, M: Mode> {
    i2c: I2c<'d, T, M>,
}

impl<'d, T: Instance, M: Mode> Sccb<'d, T, M> {
    pub fn new(i2c: I2c<'d, T, M>) -> Self {
        Self { i2c }
    }

    pub async fn read_reg(&mut self, reg: u8) -> Result<u8, embassy_rp::i2c::Error> {
        let mut buf = [0u8; 1];
        self.i2c
            .write_read_async(CAM_ADDR, &[reg], &mut buf)
            .await?;
        Ok(buf[0])
    }

    pub async fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), embassy_rp::i2c::Error> {
        self.i2c.write_async(CAM_ADDR, &[reg, val]).await
    }
}
