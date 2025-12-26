use embassy_rp::gpio::Input;

#[allow(dead_code)]
pub struct Switch<'d> {
    input: Input<'d>,
}

#[allow(dead_code)]
impl<'d> Switch<'d> {
    pub fn new(input: Input<'d>) -> Self {
        Self { input }
    }

    pub fn is_active(&self) -> bool {
        // Assuming "Switch" pulls to ground when active (standard switch)
        // Adjust logic if user provided schematic implies otherwise.
        // Schematic (implied context): usually GPIO -> Switch -> GND.
        self.input.is_low()
    }

    pub async fn wait_for_active(&mut self) {
        self.input.wait_for_low().await;
    }

    pub async fn wait_for_inactive(&mut self) {
        self.input.wait_for_high().await;
    }
}
