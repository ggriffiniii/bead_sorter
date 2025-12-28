use embassy_rp::pio::{
    Common, Config, Direction, LoadedProgram, Pin, ShiftDirection, StateMachine, StateMachineRx,
};
use pio::{pio_asm, InstructionOperands, JmpCondition};

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

impl<'d, T: embassy_rp::pio::Instance, const S: usize> Dvp<'d, T, S> {
    pub fn new(
        pio: &mut Common<'d, T>,
        mut sm: StateMachine<'d, T, S>,
        d0_pin: Pin<'d, T>,
        _d1_pin: Pin<'d, T>,
        _d2_pin: Pin<'d, T>,
        _d3_pin: Pin<'d, T>,
        _d4_pin: Pin<'d, T>,
        _d5_pin: Pin<'d, T>,
        _d6_pin: Pin<'d, T>,
        _d7_pin: Pin<'d, T>,
        _pclk_pin: Pin<'d, T>,
        href_pin: Pin<'d, T>,
        _vsync_pin: Pin<'d, T>,
    ) -> Self {
        // Pins must be sequential for IN: D0..D7.
        // DVP Capture Program
        // 1. Wait for HREF (GPIO 10) high (Start of Line)
        // 2. Loop PCLK (GPIO 9) cycles to capture data
        // 3. Exit loop when HREF goes low
        let prg = pio_asm!(
            "wait 0 gpio 11",
            "wait 1 gpio 11",
            ".wrap_target",
            "wait 1 gpio 10", // Wait for HREF High
            "wait 1 gpio 9",  // Wait PCLK High
            "in pins, 8",     // Capture D0-D7
            "wait 0 gpio 9",  // Wait PCLK Low
            ".wrap"
        );

        let program = pio.load_program(&prg.program);

        // Configure State Machine Here
        let mut config = Config::default();
        config.use_program(&program, &[]);

        sm.set_pin_dirs(
            Direction::In,
            &[
                &d0_pin, &_d1_pin, &_d2_pin, &_d3_pin, &_d4_pin, &_d5_pin, &_d6_pin, &_d7_pin,
            ],
        );

        config.set_in_pins(&[
            &d0_pin, &_d1_pin, &_d2_pin, &_d3_pin, &_d4_pin, &_d5_pin, &_d6_pin, &_d7_pin,
        ]);

        config.shift_in.direction = ShiftDirection::Right; // bit 0 = D0.
        config.shift_in.auto_fill = true; // Auto Push
        config.shift_in.threshold = 32; // 4 bytes per word

        sm.set_config(&config);
        sm.set_enable(false); // Start disabled

        Self {
            sm,
            d0: d0_pin,
            d1: _d1_pin,
            d2: _d2_pin,
            d3: _d3_pin,
            d4: _d4_pin,
            d5: _d5_pin,
            d6: _d6_pin,
            d7: _d7_pin,
            pclk: _pclk_pin,
            href: href_pin,
            vsync: _vsync_pin,
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
        // Ensure we jump to the absolute address where the program is loaded.
        let origin = self.program.origin;
        let instr = InstructionOperands::JMP {
            condition: JmpCondition::Always,
            address: origin,
        }
        .encode();
        unsafe {
            self.sm.exec_instr(instr);
        }

        // 4. Re-enable SM to start waiting for VSYNC/HREF from the top
        self.sm.set_enable(true);
    }

    pub fn stop(&mut self) {
        self.sm.set_enable(false);
    }
}
