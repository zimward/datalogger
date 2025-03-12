#![no_std]
#![no_main]

extern crate panic_semihosting;

use core::convert::Infallible;
use core::mem;
use core::ops::Deref;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering::Relaxed;

use cortex_m::asm::delay;
use cortex_m::interrupt::Mutex;
use cortex_m_semihosting::hprintln;

use cortex_m_rt::entry;

use stm32f1xx_hal::pac::{tim6, NVIC, TIM1, TIM2, TIM3};

use stm32f1xx_hal::timer::{pwm, Counter, CounterHz, Event};

use stm32f1xx_hal::{device::interrupt, prelude::*, stm32};

mod led;

static mut G_TIMER3: Option<Counter<TIM3, 10000>> = None;

//Time overflow after ~119,3h
static mut TIME: AtomicU32 = AtomicU32::new(0);
fn get_millis() -> u32 {
    unsafe { TIME.load(Relaxed) }
}

/*
#######################
pin  funktion
PB1  usb dp pin pullup
PA0  messkanal B
PA1  messkanal A
PA2  status LED
PB0 start/stop -- in dokumentation nach remap möglichkeit schauen.

#######################
*/
#[allow(non_snake_case, clippy::similar_names, clippy::too_many_lines)]
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

    let mut tm2: Counter<TIM2, 1000> = dp.TIM2.counter(&clocks);
    let mut led = gpioa.pa2.into_alternate_push_pull(&mut gpioa.crl);
    let pwm = tm2.pwm(led, &mut afio.mapr, 1.kHz()).split();

    {
        let mut tm3: Counter<TIM3, 10000> = dp.TIM3.counter(&clocks);
        let _ = tm3.start(1.millis());
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
    cortex_m::interrupt::free(|_cs| {
        unsafe {
            TIME.fetch_add(1, Relaxed);
        }
        if let Some(ref mut timer) = unsafe { &mut G_TIMER3 } {
            timer.clear_interrupt(Event::Update);
        }
    });
}
