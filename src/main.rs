#![no_std]
#![no_main]

use core::convert::Infallible;
use core::mem;
use core::ops::Deref;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::Relaxed;

use cortex_m::asm::delay;
use cortex_m::interrupt::Mutex;
use cortex_m_semihosting::hprintln;

use cortex_m_rt::entry;

use stm32f1xx_hal::gpio::{ErasedPin, Input, Output, PullDown, PushPull};
use stm32f1xx_hal::i2c::{BlockingI2c, I2c, Mode};
use stm32f1xx_hal::pac::{NVIC, TIM3};

use stm32f1xx_hal::timer::{Counter, Event};

use stm32f1xx_hal::usb::{Peripheral, UsbBus, UsbBusType};
use stm32f1xx_hal::{device::interrupt, prelude::*, stm32};

//static mut G_TIMER2: Option<Timer<Timer2>> = None;
//static mut G_TIMER1: Option<Timer<TIMER1>> = None;
static mut G_TIMER3: Option<Counter<TIM3, 10000>> = None;

//Time overflow after ~119,3h
static mut TIME: AtomicUsize = AtomicUsize::new(0);
fn get_millis() -> usize {
    unsafe { TIME.load(Relaxed) }
}

#[allow(
    non_snake_case,
    clippy::unwrap_used,
    clippy::similar_names,
    clippy::too_many_lines
)]
#[entry]
fn main() -> ! {
    let take = stm32::Peripherals::take().unwrap();
    let dp = take;

    let mut flash = dp.FLASH.constrain();
    let rcc = dp.RCC.constrain();
    let clocks = rcc
        .cfgr
        .use_hse(8.MHz())
        .sysclk(48.MHz())
        .pclk1(24.MHz())
        .freeze(&mut flash.acr);
    assert!(clocks.usbclk_valid());

    let mut afio = dp.AFIO.constrain();

    let mut gpioa = dp.GPIOA.split();
    let mut gpiob = dp.GPIOB.split();
    let mut led = gpiob.pb8.into_push_pull_output(&mut gpiob.crh);

    {
        let mut tm3: Counter<TIM3, 10000> = dp.TIM3.counter(&clocks);
        tm3.start(1.millis());
        tm3.listen(Event::Update);
        unsafe { G_TIMER3 = Some(tm3) };
        unsafe {
            NVIC::unmask(interrupt::TIM3);
        }
    }

    let mut last = get_millis();
    loop {
        if last + 1_000 <= get_millis() {
            last = get_millis();
        }
    }
}

#[allow(clippy::unwrap_used)]
#[interrupt]
fn TIM3() {
    cortex_m::interrupt::free(|cs| {
        unsafe {
            TIME.fetch_add(1, Relaxed);
        }
        if let Some(ref mut timer) = unsafe { &mut G_TIMER3 } {
            timer.clear_interrupt(Event::Update);
        }
    });
}
