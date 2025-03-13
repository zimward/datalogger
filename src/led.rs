pub enum LedMode {
    On,
    Off,
    BlinkSlow,
    BlinkFast,
    Breathe,
}

const THRESH_FAST: u16 = 20;
const THRESH_SLOW: u16 = 100;
const THRESH_BREATHE: u16 = 100;

pub struct Led<Pwm> {
    pwm: Pwm,
    max_duty: u16,
    counter: u16,
    mode: LedMode,
}

impl<Pwm> Led<Pwm>
where
    Pwm: FnMut(u16),
{
    pub fn new(mut pwm: Pwm, max_duty: u16) -> Self {
        pwm(0);
        Self {
            pwm,
            max_duty,
            mode: LedMode::Off,
            counter: 0u16,
        }
    }

    pub fn update(&mut self) {
        self.counter = self.counter.wrapping_add(1);
        match self.mode {
            LedMode::BlinkFast => {
                if self.counter > THRESH_FAST {
                    (self.pwm)(1);
                }
            }
            LedMode::BlinkSlow => {
                if self.counter > THRESH_SLOW {
                    (self.pwm)(1);
                }
            }
            LedMode::Breathe => {
                self.counter %= THRESH_BREATHE;
                let duty = if self.counter > THRESH_BREATHE / 2 {
                    (THRESH_BREATHE / 2 * self.max_duty) / self.counter
                } else {
                    (self.counter * self.max_duty) / (THRESH_BREATHE / 2)
                };
                (self.pwm)(duty);
            }
            _ => (),
        }
    }

    pub fn set_mode(&mut self, mode: LedMode) {
        match mode {
            LedMode::On => (self.pwm)(self.max_duty),
            LedMode::Off => (self.pwm)(0),
            _ => (),
        }
        self.mode = mode;
    }
}
