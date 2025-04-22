#![no_std]
#![no_main]
#![allow(static_mut_refs)]

extern crate panic_semihosting;

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering::Relaxed;

use avg::Avg;
use cortex_m::asm::nop;
use cortex_m::singleton;

use cortex_m_rt::entry;

use embedded_hal::spi::Mode;
use embedded_sdmmc::{BlockDevice, File, SdCard, TimeSource, VolumeIdx, VolumeManager};
use led::Led;
use sdcard::{config_error, sderror, FakeTimeSource};
use stm32f1xx_hal::adc::{Adc, SampleTime, SetChannels};
use stm32f1xx_hal::dma::Half;
use stm32f1xx_hal::gpio::{Analog, PA0, PA1};
use stm32f1xx_hal::pac::{ADC1, NVIC, TIM3};

use stm32f1xx_hal::spi::Spi;
use stm32f1xx_hal::timer::Tim2NoRemap;
use stm32f1xx_hal::timer::{Counter, Event};

use stm32f1xx_hal::{device::interrupt, prelude::*, stm32};

mod avg;
mod led;
mod sdcard;

static mut G_TIMER3: Option<Counter<TIM3, 10000>> = None;

//Time overflow after ~119,3h
static mut TIME: AtomicU32 = AtomicU32::new(0);
fn get_millis() -> u32 {
    unsafe { TIME.load(Relaxed) }
}

//slowest sample time for adc
const SAMPLE_TIME: SampleTime = SampleTime::T_239;
const SAMPLE_FREQ: u32 = 1_333_333 / 239; // 2MHz adc clock, 1.5 cycle per conversion averaged over 239 cycle
                                          // 1_333_333 = 2E6 / 1.5

#[derive(serde::Deserialize)]
struct Config {
    ms_per_sample: u32,
    factor_per_ma: f32,
}

#[derive(serde::Serialize)]
struct DataPair(f32, f32);

fn convert(factor_per_ma: f32, value: u32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let u = (value as f32) * (3.3 / 1024.0);
    let current = u / (65_000.0 * 25.0 / (10_000.0 + 25.0) * 1000.0);
    current * factor_per_ma
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

fn save<D, T, P>(data: &[u8], file: File, vol_mgr: &mut VolumeManager<D, T>, led: &mut Led<P>)
where
    T: TimeSource,
    D: BlockDevice,
    P: FnMut(u16),
{
    let mut offset: usize = 0;
    loop {
        let res = vol_mgr
            .write(file, &data[offset..])
            .unwrap_or_else(|_| sderror(led));
        offset += res;
        if offset == data.len() {
            //write finished
            break;
        }
    }
}

#[allow(non_snake_case, clippy::similar_names, clippy::too_many_lines)]
#[entry]
fn main() -> ! {
    //must only be executed once in the entire program
    let dp = unsafe { stm32::Peripherals::steal() };
    let cp = unsafe { cortex_m::Peripherals::steal() };

    let mut flash = dp.FLASH.constrain();
    let rcc = dp.RCC.constrain();
    let clocks = rcc
        .cfgr
        .use_hse(8.MHz())
        .sysclk(72.MHz())
        .pclk1(24.MHz())
        .adcclk(2.MHz())
        .freeze(&mut flash.acr);
    assert!(clocks.usbclk_valid());

    let mut afio = dp.AFIO.constrain();

    let mut gpioa = dp.GPIOA.split();

    let dma_ch1 = dp.DMA1.split();

    //ADC dma config
    let mut adc = {
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
        // (adc, channelA, channelB)
    };

    //SPI config
    let mut gpiob = dp.GPIOB.split();
    let spi = {
        let pins = (
            gpioa.pa5.into_alternate_push_pull(&mut gpioa.crl),
            gpioa.pa6.into_pull_up_input(&mut gpioa.crl),
            gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl),
        );
        let spi_mode = Mode {
            polarity: embedded_hal::spi::Polarity::IdleLow,
            phase: embedded_hal::spi::Phase::CaptureOnFirstTransition,
        };
        let spi = Spi::spi1(dp.SPI1, pins, &mut afio.mapr, spi_mode, 8.MHz(), clocks);
        //maybe implement "blocking" Transfer and write for the dma object
        // spi.with_tx_dma(dma_ch1)
        spi
    };

    let btn = gpiob.pb0.into_floating_input(&mut gpiob.crl);

    let mut led = {
        // let led_pin = gpioa.pa2.into_alternate_push_pull(&mut gpioa.crl);
        let mut led_pin = gpioa.pa2.into_push_pull_output(&mut gpioa.crl);
        // let mut pwm = Timer::new(dp.TIM2, &clocks).pwm_hz::<Tim2NoRemap, _, _>(
        //     led_pin,
        //     &mut afio.mapr,
        //     1.kHz(),
        // );
        // let max_duty = pwm.get_max_duty();
        Led::new(
            move |duty| {
                // pwm.set_duty(stm32f1xx_hal::timer::Channel::C3, duty);
                if duty == 0 {
                    led_pin.set_low();
                } else {
                    led_pin.set_high();
                }
            },
            255,
        )
    };
    led.set_mode(led::LedMode::On);

    let delay = cp.SYST.delay(&clocks);

    let sdcard = SdCard::new(spi, gpioa.pa4.into_push_pull_output(&mut gpioa.crl), delay);
    let mut volume_mgr = VolumeManager::new(sdcard, FakeTimeSource::new());
    let vol0 = volume_mgr
        .open_volume(VolumeIdx(0))
        .unwrap_or_else(|_| sderror(&mut led));
    let root = volume_mgr
        .open_root_dir(vol0)
        .unwrap_or_else(|_| sderror(&mut led));
    let config = volume_mgr
        .open_file_in_dir(root, "config.csv", embedded_sdmmc::Mode::ReadOnly)
        .unwrap_or_else(|_| sderror(&mut led));
    let mut buffer = [0u8; 512];

    let cfg: Config = {
        let mut binding = serde_csv_core::csv_core::ReaderBuilder::new();
        let builder = binding.terminator(serde_csv_core::csv_core::Terminator::Any(b'\n'));
        let mut reader = serde_csv_core::Reader::<32>::from_builder(builder);
        let mut cfg = None;
        loop {
            let bytes = volume_mgr.read(config, &mut buffer);
            if let Ok(bytes) = bytes {
                if bytes == 0 {
                    break; // EOF
                }
                let record = reader.deserialize::<Config>(&buffer);
                if let Ok((conf, _)) = record {
                    cfg = Some(conf);
                    break;
                }
            } else {
                //read is failing for some reason
                sderror(&mut led);
            }
        }
        cfg
    }
    .unwrap_or_else(|| {
        config_error(&mut led);
    });
    //close config file
    let _ = volume_mgr.close_file(config);

    let outfile = volume_mgr
        .open_file_in_dir(
            root,
            "out.csv",
            embedded_sdmmc::Mode::ReadWriteCreateOrTruncate,
        )
        .unwrap_or_else(|_| sderror(&mut led));
    //LED config

    //system timer setup
    {
        let mut tm3: Counter<TIM3, 10000> = dp.TIM3.counter(&clocks);
        let _ = tm3.start(1.millis());
        tm3.listen(Event::Update);

        //unsafe due to involving global state used by an interrupt
        unsafe { G_TIMER3 = Some(tm3) };
        unsafe {
            NVIC::unmask(interrupt::TIM3);
        }
    }

    // led.set_mode(led::LedMode::Breathe);
    let mut last = get_millis();
    let dma_buffer = {
        let b = singleton!(: [[u16;256];2]=[[0;256];2]);
        //ugly unwrap to prevent panics in release build
        assert!(b.is_some());
        unsafe { b.unwrap_unchecked() }
    };
    let mut last_half = Half::Second;

    let mut channel_a_avg = Avg::new(cfg.ms_per_sample * SAMPLE_FREQ / 1000);
    let mut channel_b_avg = Avg::new(cfg.ms_per_sample * SAMPLE_FREQ / 1000);

    let mut writer = serde_csv_core::Writer::new();

    let mut dir = false;
    loop {
        if last + 500 <= get_millis() {
            last = get_millis();
            if dir {
                led.set_mode(led::LedMode::On);
            } else {
                led.set_mode(led::LedMode::Off);
            }
            dir = !dir;
        }
        if btn.is_high() {
            let start = get_millis();
            while start + 30 > get_millis() {} //debounce delay
            while btn.is_high() {} //wait for button to be released
            led.set_mode(led::LedMode::On);
            break;
        }
    }
    //start reading adc
    let mut adc_buffer = adc.circ_read(dma_buffer);

    loop {
        //100 Hz loop for uncritical purposes (LED)
        if last + 10 <= get_millis() {
            last = get_millis();
            led.update();
        }

        //stop button logic
        if btn.is_high() {
            volume_mgr
                .close_file(outfile)
                .unwrap_or_else(|_| sderror(&mut led));
            volume_mgr
                .close_dir(root)
                .unwrap_or_else(|_| sderror(&mut led));
            volume_mgr
                .close_volume(vol0)
                .unwrap_or_else(|_| sderror(&mut led));
            led.set_mode(led::LedMode::Off);
            loop {
                nop();
            }
        }

        if let Ok(half) = adc_buffer.readable_half() {
            if half != last_half {
                last_half = half;
                if let Ok(half) = adc_buffer.peek(|half, _| *half) {
                    for vals in half.windows(2) {
                        // read vals
                        let a = channel_a_avg
                            .update(vals[0])
                            .map(|v| convert(cfg.factor_per_ma, v));
                        let b = channel_b_avg
                            .update(vals[1])
                            .map(|v| convert(cfg.factor_per_ma, v));
                        if let (Some(a), Some(b)) = (a, b) {
                            let pair = DataPair(a, b);
                            //one line should not be larger than 30 digits so this is enough margin
                            let mut buf = [0u8; 36];
                            if let Ok(size) = writer.serialize(&pair, &mut buf) {
                                //write to disk
                                save(&buf[..size], outfile, &mut volume_mgr, &mut led);
                            }
                        }
                    }
                }
            }
        } else {
            //reset dma
            let (buf, adc) = adc_buffer.stop();
            adc_buffer = adc.circ_read(buf);
            last_half = Half::Second;
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
