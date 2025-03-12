#![no_std]
#![no_main]

extern crate panic_semihosting;

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

static mut G_TIMER3: Option<Counter<TIM3, 10000>> = None;

//Time overflow after ~119,3h
static mut TIME: AtomicUsize = AtomicUsize::new(0);
fn get_millis() -> usize {
    unsafe { TIME.load(Relaxed) }
}

#[allow(non_snake_case, clippy::similar_names, clippy::too_many_lines)]
/*
#######################
pin  funktion
PB1  usb dp pin pullup
PA0  messkanal B
PA1  messkanal A
PA2  status LED
Boot0 start/stop -- in dokumentation nach remap möglichkeit schauen.

#######################
*/
#[entry]
fn main() -> ! {
    //sollte das fehlschlagen haben wir andere probleme
    let take = unsafe { stm32::Peripherals::take().unwrap_unchecked() };
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
    let mut led = gpioa.pa2.into_push_pull_output(&mut gpioa.crl);

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
