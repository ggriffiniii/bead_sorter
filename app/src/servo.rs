use embassy_rp::pwm::{Pwm, SetDutyCycle};
use embassy_time::{Duration, Instant, Timer};

pub enum Channel {
    A,
    B,
}

pub struct Servo<'d> {
    pwm: Pwm<'d>,
    channel: Channel, // Kept for reference, though new_output_a/b might bind it.
    min_us: u16,
    max_us: u16,
    current_us: u16,
}

impl<'d> Servo<'d> {
    pub fn new(pwm: Pwm<'d>, channel: Channel, min_us: u16, max_us: u16) -> Self {
        Self {
            pwm,
            channel,
            min_us,
            max_us,
            current_us: min_us, // Default to min position
        }
    }

    pub fn set_pulse_width(&mut self, us: u16) {
        let us = us.clamp(self.min_us, self.max_us);
        self.current_us = us;
        // Period is 20ms (20000us).
        // set_duty_cycle_fraction(num, denom).
        // num = us, denom = 20000.
        // This assumes top is 20000?
        // No, set_duty_cycle_fraction calculates based on top?
        // Actually, easiest is raw set_compare if available.
        // If not, use fraction.
        // If configured with Top=20000.
        // Then fraction us/20000 maps to counts us.
        let _ = self.pwm.set_duty_cycle_fraction(us, 20000);
    }

    pub async fn move_to(&mut self, target_us: u16, duration: Duration) {
        let start_us = self.current_us;
        let start_time = Instant::now();

        loop {
            let elapsed = Instant::now().duration_since(start_time);
            if elapsed >= duration {
                break;
            }

            let progress = elapsed.as_millis() as f32 / duration.as_millis() as f32;
            let eased_progress = Self::easing_curve(progress);

            // Interpolate
            let diff = (target_us as i32) - (start_us as i32);
            let new_us = start_us as i32 + (diff as f32 * eased_progress) as i32;

            self.set_pulse_width(new_us as u16);

            Timer::after(Duration::from_millis(20)).await; // 50Hz update rate
        }

        // Ensure final position is set exactly
        self.set_pulse_width(target_us);
    }

    // Ease Out Quartic: 1 - (1 - x)^4
    // Starts fast, decelerates aggressively and has a long gentle stop.
    fn easing_curve(x: f32) -> f32 {
        let t = 1.0 - x;
        1.0 - (t * t * t * t)
    }
}
