use stm32f1xx_hal::gpio::{ErasedPin, Output};

pub struct Led {
    pin: ErasedPin<Output>,
    counter: u16,
}

impl Led {
    pub const fn new(pin: ErasedPin<Output>) -> Self {
        Self { pin, counter: 0u16 }
    }

    pub fn update(&mut self) {
        self.counter = self.counter.wrapping_add(1);
        //mode logic
    }

    pub fn set_mode() {}
}
