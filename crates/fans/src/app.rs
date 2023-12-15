use std::{ops::Range, path::PathBuf, time::Duration};

use clap::Parser;
use log::warn;

use crate::{AmdGpuPwmEnable, Fan, Hwmon, HwmonEntry, MsiPwmEnable, Pwm, Temp, Then};

use anyhow::{anyhow, Result};

use futures::StreamExt;

#[derive(Debug, Copy, Clone, Parser)]
pub struct Fans {
    #[clap(short, long, parse(try_from_str = Self::tick_from_str), default_value = "0.1")]
    tick: Duration,
}

impl Fans {
    fn tick_from_str(s: &str) -> Result<Duration> {
        Ok(Duration::from_secs_f64(s.parse::<f64>()?))
    }

    pub fn run(self) -> Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;
        let _guard = runtime.enter();
        runtime.block_on(self.run_async())
    }

    pub async fn run_async(self) -> Result<()> {
        // Initialize Hwmon
        let hwmon = Hwmon::new().await?;

        // Fetch hwmon handles
        let hwmon_gpu = (&hwmon).then(get_entry("amdgpu"))?;

        // Fetch temp handles
        let temp_gpu = hwmon_gpu.then(get_temp(&2))?;

        // Fetch PWM handles
        let pwm_gpu = hwmon_gpu.then(get_pwm(&1))?;

        // Enable PWMs
        pwm_gpu.set_enable(AmdGpuPwmEnable::Manual.into()).await?;

        let mut coolant_temp_buf: Option<[f64; 60]> = None;
        let mut gpu_temp_buf: Option<[f64; 60]> = None;

        enum FansEvent {
            Tick,
            Exit,
        }

        let tick = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(self.tick))
            .map(|_| FansEvent::Tick);

        let exit = futures::stream_select!(
            tokio_stream::wrappers::SignalStream::new(tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::interrupt()
            )?),
            tokio_stream::wrappers::SignalStream::new(tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::hangup()
            )?),
            tokio_stream::wrappers::SignalStream::new(tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::terminate()
            )?),
        )
        .map(|_| FansEvent::Exit);

        let mut events = futures::stream_select!(tick, exit);

        while let Some(event) = events.next().await {
            match event {
                FansEvent::Tick => {
                    // Read coolant temp
                    let coolant_temp = tokio::fs::read_to_string("/tmp/coolant-temp").await?;
                    let coolant_temp = coolant_temp.strip_suffix('\n').unwrap_or(&coolant_temp);
                    let coolant_temp =
                        if let Ok(coolant_temp) = coolant_temp.then(str::parse::<i32>) {
                            coolant_temp
                        } else {
                            warn!("Empty coolant temp file");
                            continue;
                        }
                        .then(temp_input_to_f64);

                    let coolant_temp_buf = if let Some(buf) = &mut coolant_temp_buf {
                        buf
                    } else {
                        coolant_temp_buf = Some([coolant_temp; 60]);
                        coolant_temp_buf.as_mut().unwrap()
                    };

                    let coolant_temp = coolant_temp.then(moving_average(coolant_temp_buf));

                    let coolant_temp_norm = coolant_temp.then(map_from(&(30.9..34.0)));
                    let coolant_temp_norm_system = coolant_temp.then(map_from(&(31.6..34.0)));
                    let coolant_temp_norm_pump = coolant_temp.then(map_from(&(32.0..33.0)));

                    // Read GPU temp
                    let gpu_temp = temp_gpu.input().await?.then(temp_input_to_f64);

                    let gpu_temp_buf = if let Some(buf) = &mut gpu_temp_buf {
                        buf
                    } else {
                        gpu_temp_buf = Some([gpu_temp; 60]);
                        gpu_temp_buf.as_mut().unwrap()
                    };

                    let gpu_temp = gpu_temp.then(moving_average(gpu_temp_buf));
                    let gpu_temp_norm = gpu_temp.then(map_from(&(43.0..100.0)));

                    // Calculate general temp
                    let general_temp_norm = coolant_temp_norm_system.max(gpu_temp_norm);

                    // Pump target
                    let pump_target = coolant_temp_norm_pump
                        .then(map_to(&(50.0..100.0)))
                        .clamp(50.0, 100.0)
                        .then(f64_as_u8);

                    // CPU / Rear PWMs
                    let cpu_pwm = coolant_temp_norm
                        .then(map_to(&(0.0..100.0)))
                        .clamp(0.0, 100.0)
                        .then(f64_as_u8);

                    let rear_pwm = coolant_temp_norm
                        .then(map_to(&(0.0..100.0)))
                        .clamp(0.0, 100.0)
                        .then(f64_as_u8);

                    // System PWM
                    let system_pwm = general_temp_norm
                        .then(map_to(&(0.0..100.0)))
                        .clamp(0.0, 100.0)
                        .then(f64_as_u8);

                    // Write target files
                    tokio::fs::write("/tmp/pump-target", format!("{pump_target:}\n")).await?;
                    tokio::fs::write("/tmp/fan1-target", format!("{system_pwm:}\n")).await?;
                    tokio::fs::write("/tmp/fan2-target", format!("{rear_pwm:}\n")).await?;
                    tokio::fs::write("/tmp/fan3-target", format!("{cpu_pwm:}\n")).await?;
                    tokio::fs::write("/tmp/fan4-target", format!("{cpu_pwm:}\n")).await?;
                    tokio::fs::write("/tmp/fan5-target", format!("{cpu_pwm:}\n")).await?;

                    // Write GPU PWM
                    let gpu_pwm = gpu_temp_norm
                        .then(map_to(&(35.0..255.0)))
                        .clamp(35.0, 255.0)
                        .then(f64_as_u8);
                    pwm_gpu.set_value(gpu_pwm).await?;
                }
                FansEvent::Exit => break,
            }
        }

        pwm_gpu
            .set_enable(AmdGpuPwmEnable::Automatic.into())
            .await?;

        // Done
        Ok(())
    }

    /*
    pub fn run_pid(self) -> Result<()> {
        let hwmon = Hwmon::new()?;

        let mut pid_gpu_temp = PidController::new(PidParameters {
            proportional_factor: 1.0,
            integral_factor: 0.0,
            derivative_factor: 0.0,
        });

        let mut pid_fan_speed = PidController::new(PidParameters {
            proportional_factor: 0.25,
            integral_factor: 0.3,
            derivative_factor: 1.2,
        });

        let mut ts_prev = Instant::now();
        loop {
            let now = Instant::now();
            let time_delta = now.duration_since(ts_prev).as_secs_f64();
            ts_prev = now;

            let gpu_temp = (&hwmon)
                .then(get_entry("amdgpu"))
                .and_then(get_temp(&2))
                .and_then(Temp::input)?
                .then(temp_input_to_f64);

            println!("GPU temp: {gpu_temp:?}");

            let gpu_temp_norm = gpu_temp.then(map_from(&(43.0..100.0)));

            pid_gpu_temp.inputs.measured_value = gpu_temp_norm;
            pid_gpu_temp.tick(time_delta);

            let pid_gpu_output = pid_gpu_temp.outputs.total();

            println!("GPU temp PID output: {pid_gpu_output:}",);

            pid_fan_speed.inputs.setpoint = -pid_gpu_output;

            println!("Fan RPM setpoint: {}", pid_fan_speed.inputs.setpoint);

            println!();

            let fan_rpm = (&hwmon)
                .then(get_entry("amdgpu"))
                .and_then(get_fan(&1))
                .and_then(Fan::input)?
                .then(u16_as_f64);

            println!("Fan RPM: {fan_rpm:}");

            let fan_rpm_norm = fan_rpm.then(map_from(&(600.0..3400.0)));

            println!("Fan RPM normalized: {fan_rpm_norm:}");

            pid_fan_speed.inputs.measured_value = fan_rpm_norm;
            pid_fan_speed.tick(time_delta);

            println!("Fan speed PID output: {:?}", pid_fan_speed.outputs.total());

            let pwm_value = pid_fan_speed
                .outputs
                .total()
                .clamp(0.0, 1.0)
                .then(map_to(&(35.0..255.0))) as u8;

            println!("PWM value: {pwm_value:}");

            (&hwmon)
                .then(get_entry("amdgpu"))
                .and_then(get_pwm(&1))?
                .set_value(pwm_value)?;

            println!();

            std::thread::sleep(self.tick)
        }
    }
    */
}

fn moving_average<const N: usize>(buf: &mut [f64; N]) -> impl FnMut(f64) -> f64 + '_ {
    move |x| {
        buf.copy_within(1.., 0);
        buf[N - 1] = x;
        buf.iter().sum::<f64>() / buf.len() as f64
    }
}

/// Map the provided number from the provided range to 0..1
fn map_from(from: &Range<f64>) -> impl Fn(f64) -> f64 + '_ {
    move |x| (x - from.start) / (from.end - from.start)
}

/// Map the provided number from 0..1 to the provided range
fn map_to(to: &Range<f64>) -> impl Fn(f64) -> f64 + '_ {
    move |x| to.start + (to.end - to.start) * x
}

/// Map the provided number from one range to another
fn linear_map<'a>(from: &'a Range<f64>, to: &'a Range<f64>) -> impl Fn(f64) -> f64 + 'a {
    move |x| x.then(map_from(from)).then(map_to(to))
}

/// Type that can wrap itself in a Result
pub trait Lift: Sized {
    fn lift(&self) -> Result<&Self> {
        Ok(self)
    }

    fn lift_mut(&mut self) -> Result<&mut Self> {
        Ok(self)
    }

    fn lift_into(self) -> Result<Self> {
        Ok(self)
    }
}

impl<T> Lift for T where T: Sized {}

fn get_entry(hwmon_name: &str) -> impl Fn(&Hwmon) -> Result<&HwmonEntry> + '_ {
    move |hwmon: &Hwmon| Ok(hwmon.get(hwmon_name).ok_or(anyhow!("Invalid hwmon"))?)
}

fn get_temp(temp_idx: &usize) -> impl Fn(&HwmonEntry) -> Result<&Temp<PathBuf>> + '_ {
    move |entry: &HwmonEntry| Ok(entry.temps.get(temp_idx).ok_or(anyhow!("Invalid temp"))?)
}

fn get_pwm(pwm_idx: &usize) -> impl Fn(&HwmonEntry) -> Result<&Pwm<PathBuf>> + '_ {
    move |entry: &HwmonEntry| Ok(entry.pwms.get(pwm_idx).ok_or(anyhow!("Invalid PWM"))?)
}

fn get_fan(fan_idx: &usize) -> impl Fn(&HwmonEntry) -> Result<&Fan<PathBuf>> + '_ {
    move |entry: &HwmonEntry| Ok(entry.fans.get(fan_idx).ok_or(anyhow!("Invalid fan"))?)
}

fn temp_input_to_f64(int: i32) -> f64 {
    int as f64 / 1000.0
}

fn pwm_value_to_f64(int: u8) -> f64 {
    int as f64 / 255.0
}

fn u16_as_f64(int: u16) -> f64 {
    int as f64
}

fn f64_as_u8(float: f64) -> u8 {
    float as u8
}
