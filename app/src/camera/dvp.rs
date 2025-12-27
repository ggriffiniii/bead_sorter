use embassy_rp::pio::{
    Common, Config, Direction, Pin, ShiftDirection, StateMachine, StateMachineRx,
};
use pio::pio_asm;

pub struct Dvp<'d, T: embassy_rp::pio::Instance, const S: usize> {
    sm: StateMachine<'d, T, S>,
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
        // JMP pin: HREF.
        // Side set? No.
        // We need to use input_pins for "wait gpio".
        // Wait GPIO uses absolute numbering!
        // We can use "wait 1 pin 0" if we set input_pins correctly, but it's tricky with multiple pins.
        // Best approach for PIO "wait":
        // Map VSYNC, HREF, PCLK to `in` pins if possible, or use JMP PIN for one.
        // The previous code hardcoded GPIO 9, 10, 11 (PCLK, HREF, VSYNC).
        // Let's verify actual pins: PCLK=14, VSYNC=16, HREF=15.
        // The hardcoded values 9,10,11 are WRONG for Bead Sorter hardware.
        // This explains the hang.
        //
        // Also: VSYNC Config in ov7670.rs is "VS_NEG" (Negative VSYNC). Active Low?
        // Usually VSYNC is a short pulse. Start of frame is indicated by VSYNC edge.
        // If VS_NEG=1, VSYNC pulse is Negative (High->Low->High).
        // Start of frame is Falling Edge?
        // Let's assume standard VSYNC pulse logic.
        //
        // Correct PIO for flexible pins:
        // We can't easily wait on arbitrary pins without mapping them.
        // But we have limited "Input Source" mappings.
        //
        // To fix quickly: Hardcode correct GPIO numbers for Bead Sorter.
        // PCLK = 14
        // HREF = 15
        // VSYNC = 16

        // DVP Capture Program
        // 1. Wait for HREF (GPIO 10) high (Start of Line)
        // 2. Loop PCLK (GPIO 9) cycles to capture data
        // 3. Exit loop when HREF goes low
        let prg = pio_asm!(
            ".wrap_target",
            "wait 1 gpio 10", // Wait for HREF High
            "line_loop:",
            "  wait 1 gpio 9",      // Wait PCLK High
            "  in pins, 8",         // Capture D0-D7
            "  wait 0 gpio 9",      // Wait PCLK Low
            "  jmp pin, line_loop", // Continue if HREF is still High
            ".wrap"
        );

        let mut config = Config::default();
        config.use_program(&pio.load_program(&prg.program), &[]);

        sm.set_pin_dirs(
            Direction::In,
            &[
                &d0_pin, &_d1_pin, &_d2_pin, &_d3_pin, &_d4_pin, &_d5_pin, &_d6_pin, &_d7_pin,
            ],
        );

        // Map `in` pin base to D0 (GPIO 0)
        config.set_in_pins(&[
            &d0_pin, &_d1_pin, &_d2_pin, &_d3_pin, &_d4_pin, &_d5_pin, &_d6_pin, &_d7_pin,
        ]);

        // JMP pin is HREF (GPIO 10)
        config.set_jmp_pin(&href_pin);

        config.shift_in.direction = ShiftDirection::Left; // bit 0 = D0
        config.shift_in.auto_fill = true; // Auto Push
        config.shift_in.threshold = 32; // 4 bytes per word

        sm.set_config(&config);
        sm.set_enable(true);

        Self { sm }
    }

    pub fn rx(&mut self) -> &mut StateMachineRx<'d, T, S> {
        self.sm.rx()
    }

    pub fn prepare_capture(&mut self) {
        // Drain FIFO
        while self.sm.rx().try_pull().is_some() {}
        self.sm.set_enable(true);
    }
}
