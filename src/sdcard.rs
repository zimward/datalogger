use embedded_sdmmc::{TimeSource, Timestamp};

use crate::led::{Led, LedMode};

pub struct FakeTimeSource {}
impl FakeTimeSource {
    pub const fn new() -> Self {
        Self {}
    }
}
impl TimeSource for FakeTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 4,
            minutes: 20,
            seconds: 1,
        }
    }
}

const SECOND: u32 = 48_000_000;
const HALF: u32 = 24_000_000;

pub fn sderror<T>(led: &mut Led<T>) -> !
where
    T: FnMut(u16),
{
    loop {
        led.set_mode(LedMode::On);
        cortex_m::asm::delay(SECOND);
        led.set_mode(LedMode::Off);
        cortex_m::asm::delay(SECOND);
    }
}

pub fn config_error<T>(led: &mut Led<T>) -> !
where
    T: FnMut(u16),
{
    loop {
        led.set_mode(LedMode::On);
        cortex_m::asm::delay(HALF);
        led.set_mode(LedMode::Off);
        cortex_m::asm::delay(HALF);
        led.set_mode(LedMode::On);
        cortex_m::asm::delay(SECOND);
        led.set_mode(LedMode::Off);
        cortex_m::asm::delay(SECOND);
    }
}
