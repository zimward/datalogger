#![no_std]
#![no_main]

extern crate panic_semihosting;

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering::Relaxed;

use avg::Avg;
use cortex_m::asm::delay;
use cortex_m::interrupt::Mutex;
use cortex_m::singleton;
use cortex_m_semihosting::hprintln;

use cortex_m_rt::entry;

use embedded_hal::spi::{self, Mode};
use led::Led;
use stm32f1xx_hal::adc::{Adc, SampleTime, SetChannels};
use stm32f1xx_hal::dma::Half;
use stm32f1xx_hal::gpio::{Analog, PA0, PA1};
use stm32f1xx_hal::pac::{ADC1, NVIC, TIM3};

use stm32f1xx_hal::spi::Spi;
use stm32f1xx_hal::timer::Tim2NoRemap;
use stm32f1xx_hal::timer::{Counter, Event, Timer};

use stm32f1xx_hal::{device::interrupt, prelude::*, stm32};

mod avg;
mod led;

static mut G_TIMER3: Option<Counter<TIM3, 1000>> = None;

//Time overflow after ~119,3h
static mut TIME: AtomicU32 = AtomicU32::new(0);
fn get_millis() -> u32 {
    unsafe { TIME.load(Relaxed) }
}

//slowest sample time for adc
const SAMPLE_TIME: SampleTime = SampleTime::T_239;

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
    //must only be executed once in the entire program
    let dp = unsafe { stm32::Peripherals::steal() };

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

    let dma_ch1 = dp.DMA1.split();

    //ADC dma config
    let adc = {
        struct AdcPins(PA0<Analog>, PA1<Analog>);
        impl SetChannels<AdcPins> for Adc<ADC1> {
            fn set_sequence(&mut self) {
                //first channel A then B, but doesn't really matter
                self.set_regular_sequence(&[1, 0]);
                //continually scan input channels
                self.set_continuous_mode(true);
            }

            fn set_samples(&mut self) {
                self.set_channel_sample_time(0, SAMPLE_TIME);
                self.set_channel_sample_time(1, SAMPLE_TIME);
            }
        }

        let channelB = gpioa.pa0.into_analog(&mut gpioa.crl);
        let channelA = gpioa.pa1.into_analog(&mut gpioa.crl);

        let mut adc = Adc::adc1(dp.ADC1, clocks);
        //slowest sampling time should be sufficient
        adc.set_sample_time(stm32f1xx_hal::adc::SampleTime::T_239);
        adc.with_scan_dma(AdcPins(channelB, channelA), dma_ch1.1)
    };

    //SPI config
    let gpiob = dp.GPIOB.split();
    {
        let pins = (gpiob.pb13, gpiob.pb14, gpiob.pb15);
        let spi_mode = Mode {
            polarity: embedded_hal::spi::Polarity::IdleLow,
            phase: embedded_hal::spi::Phase::CaptureOnFirstTransition,
        };

        let spi = Spi::spi2(dp.SPI2, pins, spi_mode, 100.kHz(), clocks);
    }
    //LED config

    let mut led = {
        let led_pin = gpioa.pa2.into_alternate_push_pull(&mut gpioa.crl);
        let mut pwm = Timer::new(dp.TIM2, &clocks).pwm_hz::<Tim2NoRemap, _, _>(
            led_pin,
            &mut afio.mapr,
            1.kHz(),
        );
        let max_duty = pwm.get_max_duty();
        Led::new(
            move |duty| {
                pwm.set_duty(stm32f1xx_hal::timer::Channel::C3, duty);
            },
            max_duty,
        )
    };

    //system timer setup
    {
        let mut tm3: Counter<TIM3, 1000> = dp.TIM3.counter(&clocks);
        let _ = tm3.start(1.millis());
        tm3.listen(Event::Update);

        //unsafe due to involving global state used by an interrupt
        unsafe { G_TIMER3 = Some(tm3) };
        unsafe {
            NVIC::unmask(interrupt::TIM3);
        }
    }

    led.set_mode(led::LedMode::Breathe);
    let mut last = get_millis();
    let dma_buffer = {
        let b = singleton!(: [[u16;32];2]=[[0;32];2]);
        //ugly unwrap to prevent panics in release build
        assert!(b.is_some());
        unsafe { b.unwrap_unchecked() }
    };
    let mut adc_buffer = adc.circ_read(dma_buffer);
    let mut last_half = Half::First;

    let mut channel_a_avg = Avg::new(200);
    let mut channel_b_avg = Avg::new(200);

    loop {
        //100 Hz loop for uncritical purposes (LED)
        if last + 10 <= get_millis() {
            last = get_millis();
            led.update();
        }
        match adc_buffer.readable_half() {
            Ok(half) => {
                if half != last_half {
                    last_half = half;
                    if let Ok(half) = adc_buffer.peek(|half, _| *half) {
                        for vals in half.windows(2) {
                            //read vals
                            channel_a_avg.update(vals[0]).map(|v| {
                                todo!();
                                //write values to buffer of sd card
                            });
                            channel_b_avg.update(vals[1]).map(|v| {
                                todo!();
                            });
                        }
                    } //else overrun
                }
                //else already read
            }
            Err(err) => {
                //should always be unreachable, unless we do too much work in the read loop
                unreachable!("DMA overrun")
            }
        }
    }
}

//millisecond system timer
#[interrupt]
fn TIM3() {
    cortex_m::interrupt::free(|_cs| {
        //unsafe due to involving shared state with an interrupt
        unsafe {
            TIME.fetch_add(1, Relaxed);
        }
        if let Some(ref mut timer) = unsafe { &mut G_TIMER3 } {
            timer.clear_interrupt(Event::Update);
        }
    });
}
