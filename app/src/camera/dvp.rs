use embassy_rp::pio::{
    Common, Config, Direction, LoadedProgram, Pin, ShiftDirection, StateMachine, StateMachineRx,
};

pub struct Dvp<'d, T: embassy_rp::pio::Instance, const S: usize> {
    sm: StateMachine<'d, T, S>,
    d0: Pin<'d, T>,
    d1: Pin<'d, T>,
    d2: Pin<'d, T>,
    d3: Pin<'d, T>,
    d4: Pin<'d, T>,
    d5: Pin<'d, T>,
    d6: Pin<'d, T>,
    d7: Pin<'d, T>,
    pclk: Pin<'d, T>,
    href: Pin<'d, T>,
    vsync: Pin<'d, T>,
    program: LoadedProgram<'d, T>,
}

use embassy_rp::pio::PioPin;
use embassy_rp::Peri;

impl<'d, T: embassy_rp::pio::Instance, const S: usize> Dvp<'d, T, S> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pio: &mut Common<'d, T>,
        mut sm: StateMachine<'d, T, S>,
        d0: Peri<'d, impl PioPin + 'd>,
        d1: Peri<'d, impl PioPin + 'd>,
        d2: Peri<'d, impl PioPin + 'd>,
        d3: Peri<'d, impl PioPin + 'd>,
        d4: Peri<'d, impl PioPin + 'd>,
        d5: Peri<'d, impl PioPin + 'd>,
        d6: Peri<'d, impl PioPin + 'd>,
        d7: Peri<'d, impl PioPin + 'd>,
        pclk: Peri<'d, impl PioPin + 'd>,
        href: Peri<'d, impl PioPin + 'd>,
        vsync: Peri<'d, impl PioPin + 'd>,
    ) -> Self {
        // Convert peripherals to PIO Pins
        let d0_pin = pio.make_pio_pin(d0);
        let d1_pin = pio.make_pio_pin(d1);
        let d2_pin = pio.make_pio_pin(d2);
        let d3_pin = pio.make_pio_pin(d3);
        let d4_pin = pio.make_pio_pin(d4);
        let d5_pin = pio.make_pio_pin(d5);
        let d6_pin = pio.make_pio_pin(d6);
        let d7_pin = pio.make_pio_pin(d7);
        let pclk_pin = pio.make_pio_pin(pclk);
        let href_pin = pio.make_pio_pin(href);
        let vsync_pin = pio.make_pio_pin(vsync);

        // DVP Capture Program
        // 1. Wait for VSYNC (Start of Frame) - Rising Edge
        // 2. Wait for HREF (Start of Line) - High
        // 3. Loop PCLK cycles to capture data
        // 4. Exit loop when HREF goes low (handled by wrap logic implicitly? No, wait 1 gpio 10 handles start, where is end?)

        // Original ASM:
        // wait 0 gpio 11
        // wait 1 gpio 11
        // .wrap_target
        // wait 1 gpio 10
        // wait 1 gpio 9
        // in pins, 8
        // wait 0 gpio 9
        // .wrap

        let mut a = pio::Assembler::<32>::new();
        let mut wrap_target = a.label();
        let mut wrap_source = a.label();

        // 1. Wait for VSYNC Rising Edge
        a.wait(0, pio::WaitSource::GPIO, vsync_pin.pin(), false); // False = Absolute
        a.wait(1, pio::WaitSource::GPIO, vsync_pin.pin(), false);

        // .wrap_target
        a.bind(&mut wrap_target);

        // 2. Wait for HREF High
        a.wait(1, pio::WaitSource::GPIO, href_pin.pin(), false);

        // 3. Wait PCLK High
        a.wait(1, pio::WaitSource::GPIO, pclk_pin.pin(), false);

        // 4. Capture D0-D7
        a.r#in(pio::InSource::PINS, 8);

        // 5. Wait PCLK Low
        a.wait(0, pio::WaitSource::GPIO, pclk_pin.pin(), false);

        // .wrap
        a.bind(&mut wrap_source);
        let prg = a.assemble_with_wrap(wrap_source, wrap_target);

        let program = pio.load_program(&prg);

        // Configure State Machine Here
        let mut config = Config::default();
        config.use_program(&program, &[]);

        sm.set_pin_dirs(
            Direction::In,
            &[
                &d0_pin, &d1_pin, &d2_pin, &d3_pin, &d4_pin, &d5_pin, &d6_pin, &d7_pin,
            ],
        );

        config.set_in_pins(&[
            &d0_pin, &d1_pin, &d2_pin, &d3_pin, &d4_pin, &d5_pin, &d6_pin, &d7_pin,
        ]);

        config.shift_in.direction = ShiftDirection::Right; // bit 0 = D0.
        config.shift_in.auto_fill = true; // Auto Push
        config.shift_in.threshold = 32; // 4 bytes per word

        sm.set_config(&config);
        sm.set_enable(false); // Start disabled

        Self {
            sm,
            d0: d0_pin,
            d1: d1_pin,
            d2: d2_pin,
            d3: d3_pin,
            d4: d4_pin,
            d5: d5_pin,
            d6: d6_pin,
            d7: d7_pin,
            pclk: pclk_pin,
            href: href_pin,
            vsync: vsync_pin,
            program,
        }
    }

    pub fn rx(&mut self) -> &mut StateMachineRx<'d, T, S> {
        self.sm.rx()
    }

    pub fn prepare_capture(&mut self) {
        // 1. Assert SM is disabled (enforcing stop() was called)
        if self.sm.is_enabled() {
            panic!("PIO State Machine is already enabled! Did you forget to call stop()?");
        }

        // 2. Clear FIFO and internal state
        self.sm.clear_fifos();
        self.sm.restart(); // Resets internal state but NOT PC
        self.sm.clkdiv_restart();

        // 3. Force Jump to Program Start (Reset PC)
        unsafe {
            self.sm.exec_jmp(self.program.origin);
        }

        // 4. Re-enable SM to start waiting for VSYNC/HREF from the top
        self.sm.set_enable(true);
    }

    pub fn stop(&mut self) {
        self.sm.set_enable(false);
    }
}
