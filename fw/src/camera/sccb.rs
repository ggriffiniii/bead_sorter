use embassy_rp::i2c::{Async, I2c, Instance};

// OV7670 I2C Address (0x42 write / 0x43 read) -> 7-bit is 0x21
pub const CAM_ADDR: u8 = 0x21;

pub struct Sccb<'d, T: Instance> {
    i2c: I2c<'d, T, Async>,
}

impl<'d, T: Instance> Sccb<'d, T> {
    pub fn new(i2c: I2c<'d, T, Async>) -> Self {
        Self { i2c }
    }

    pub async fn read_reg(&mut self, reg: u8) -> Result<u8, embassy_rp::i2c::Error> {
        let mut buf = [0u8; 1];
        // SCCB often prefers Write(Reg) -> Stop -> Read(Data) -> Stop
        // instead of a standard I2C Repeated Start.
        // We split this into two separate transactions.
        self.i2c.write_async(CAM_ADDR, [reg]).await?;
        self.i2c.read_async(CAM_ADDR, &mut buf).await?;
        Ok(buf[0])
    }

    pub async fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), embassy_rp::i2c::Error> {
        self.i2c.write_async(CAM_ADDR, [reg, val]).await
    }
}
