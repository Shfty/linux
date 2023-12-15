use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, Result};
use clap::Parser;
use log::{debug, error, info};

use futures::StreamExt;
use tokio::{
    fs::write,
    runtime::Runtime,
    signal::unix::{self, SignalKind},
    sync,
    task::{spawn, JoinHandle},
    time::interval,
};
use tokio_stream::wrappers::{IntervalStream, ReceiverStream, SignalStream, WatchStream};

use crate::{
    hid::{
        command::{set_controller_state, GET_FIRMWARE_INFO},
        request,
        state::{HARDWARE, SOFTWARE},
        Hid, LED_COUNT_TOTAL,
    },
    then::Then,
    thread::{
        print_thread_result,
        pump_target::{Fan, FanTargetThread},
        server_thread::ServerThread,
    },
};

pub type Colors = Box<[[u8; 3]; LED_COUNT_TOTAL]>;

#[derive(Debug)]
pub struct SharedState {
    pub coolant_temp: AtomicU16,
    pub pump_speed: AtomicU16,
    pub fan_targets: [AtomicU16; 7],
}

impl Default for SharedState {
    fn default() -> Self {
        const DEFAULT_FAN: AtomicU16 = AtomicU16::new(50);
        SharedState {
            coolant_temp: AtomicU16::new(312),
            pump_speed: AtomicU16::new(2268),
            fan_targets: [DEFAULT_FAN; 7],
        }
    }
}

enum CapellixEvent {
    TempTick,
    SpeedTick,
    SetFanSpeed(Fan, u16),
    SetColors(Colors),
    Exit,
}

/// Userspace driver for the Corsair Commander Core / H150i Elite Capellix
#[derive(Parser)]
pub struct Capellix {
    /// Duration in seconds to wait between temperature readings
    #[clap(long, parse(try_from_str = Self::tick_from_str), default_value = "0.25")]
    temp_tick_duration: Duration,

    /// Duration in seconds to wait between speed readings
    #[clap(long, parse(try_from_str = Self::tick_from_str), default_value = "0.25")]
    speed_tick_duration: Duration,

    /// Duration in seconds to wait between color updates
    #[clap(long, parse(try_from_str = Self::tick_from_str), default_value = "0.03333333333")]
    color_tick_duration: Duration,

    /// If set, the provided files will be watched for changes and used to update the fans' speed targets
    #[clap(long)]
    fan_target_files: Vec<PathBuf>,

    /// If set, fan speed will be written to the provided files each tick
    #[clap(long)]
    fan_speed_files: Vec<PathBuf>,

    /// If set, coolant temperature will be written to the provided file each tick
    #[clap(long)]
    coolant_temp_file: Option<PathBuf>,

    /// If set, start a TCP server to listen for commands
    #[clap(long)]
    listen: bool,

    /// Socket address to listen on when starting a TCP server
    #[clap(long, default_value = "127.0.0.1:27359")]
    listen_address: SocketAddr,

    /// Subtracts a factor of the provided offset from temperature readings relative to LED brightness
    #[clap(long)]
    led_temp_offset: Option<f32>,

    /// Subtracts a factor of the provided offset from temperature readings relative to pump speed
    #[clap(long)]
    pump_speed_temp_offset: Option<f32>,

    /// If set, don't exit when encountering a newer than expected device firmware
    ///
    /// Warning: In the event of a protocol change,
    /// sending unexpected commands to the controller may result in a soft brick.
    ///
    /// If this occurs, the device can be recovered by reflashing its firmware through iCue.
    #[clap(long)]
    unrecognized_firmware: bool,

    #[clap(skip = Hid::new().expect("Failed to initialize HID"))]
    hid: Hid,

    #[clap(skip)]
    state: Arc<SharedState>,

    #[clap(skip = [[0;3]; LED_COUNT_TOTAL])]
    colors: Colors,
}

impl Capellix {
    pub fn run(self) -> Result<()> {
        let runtime = Runtime::new()?;
        let _guard = runtime.enter();
        runtime.block_on(self.run_async())
    }

    pub async fn run_async(mut self) -> Result<()> {
        // Flush any pending reads to make sure the device is in sync
        self.hid.flush_read(50)?;

        // Run HID initialization
        info!("Setting controller to software mode");
        self.hid.command(&set_controller_state(SOFTWARE))?;

        info!("Fetching firmware version");
        self.hid.command(GET_FIRMWARE_INFO)?;
        let (major, minor, patch) = (self.hid.buffer[3], self.hid.buffer[4], self.hid.buffer[5]);
        info!("Firmware version {major:}.{minor:}.{patch:}");

        if major < 2 || minor < 10 || patch < 219 {
            return Err(anyhow!(
                "Firmware versions prior to 2.10.219 are not supported."
            ));
        } else if !self.unrecognized_firmware && (major > 2 || minor > 10 || patch > 219) {
            return Err(anyhow!("Expected firmware version 2.10.219, stopping.\nTo skip this check, pass the --unrecognized-firmware flag."));
        }

        info!("Enabling direct lighting");
        self.hid.request(request::enable_direct_lighting())?;

        info!("Setting fan types to 6x QL");
        self.hid.request(request::set_fan_types_6x_ql())?;

        // Setup threads
        let (set_fan_speed_tx, set_fan_speed_rx) = sync::mpsc::channel::<(Fan, u16)>(14);

        let (set_colors_tx, set_colors_rx) =
            sync::watch::channel::<Colors>(Box::new([[0; 3]; LED_COUNT_TOTAL]));
        let set_colors_tx = Arc::new(set_colors_tx);

        let (exit_tx, exit_rx) = sync::watch::channel(true);

        let server_join_handle = if self.listen {
            let state = self.state.clone();
            let set_pump_speed_tx = set_fan_speed_tx.clone();
            let exit_rx = exit_rx.clone();
            Some(spawn(async move {
                ServerThread::new(
                    state,
                    set_pump_speed_tx,
                    set_colors_tx,
                    exit_rx,
                    self.listen_address,
                )
                .run()
                .await
                .then(print_thread_result("ServerThread"))
                .ok();
            }))
        } else {
            None
        };

        let fan_join_handles = self
            .fan_target_files
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let fan = Fan::try_from(i as u8)?;
                info!("Starting thread for {fan:?}");

                let set_fan_speed_tx = set_fan_speed_tx.clone();
                let exit_rx = exit_rx.clone();
                let path = path.clone();

                Ok(spawn(async move {
                    FanTargetThread::new(set_fan_speed_tx, exit_rx, fan, path)
                        .run()
                        .await
                        .then(print_thread_result("PumpTargetThread"))
                        .ok();
                })) as Result<JoinHandle<()>>
            })
            .collect::<Vec<_>>();

        // Create event streams
        let temp_tick =
            IntervalStream::new(interval(self.temp_tick_duration)).map(|_| CapellixEvent::TempTick);
        let speed_tick = IntervalStream::new(interval(self.speed_tick_duration))
            .map(|_| CapellixEvent::SpeedTick);
        let set_pump_speed_rx = ReceiverStream::new(set_fan_speed_rx)
            .map(|(fan, speed)| CapellixEvent::SetFanSpeed(fan, speed));
        let set_colors_rx = WatchStream::new(set_colors_rx).map(CapellixEvent::SetColors);

        let exit = futures::stream_select!(
            SignalStream::new(unix::signal(SignalKind::interrupt())?),
            SignalStream::new(unix::signal(SignalKind::hangup())?),
            SignalStream::new(unix::signal(SignalKind::terminate())?),
        )
        .map(|_| CapellixEvent::Exit);

        // Main loop
        info!("Entering main loop");
        let mut events = futures::stream_select!(
            temp_tick,
            speed_tick,
            set_pump_speed_rx,
            set_colors_rx,
            exit,
        );

        while let Some(event) = events.next().await {
            match event {
                CapellixEvent::TempTick => {
                    self.temp_tick().await?;
                }
                CapellixEvent::SpeedTick => {
                    self.speed_tick().await?;
                }
                CapellixEvent::SetFanSpeed(fan, speed) => {
                    self.write_fan_target(fan, speed)?;
                }
                CapellixEvent::SetColors(colors) => {
                    self.write_colors(colors)?;
                }
                CapellixEvent::Exit => break,
            }
        }

        exit_tx.send(false)?;

        info!("Joining threads");
        for handle in fan_join_handles {
            handle?.await?;
        }

        if let Some(handle) = server_join_handle {
            handle.await?;
        }

        info!("Setting controller to hardware mode");
        self.hid.request([set_controller_state(HARDWARE)])?;

        Ok(())
    }

    fn tick_from_str(s: &str) -> Result<Duration> {
        Ok(Duration::from_secs_f32(s.parse::<f32>()?))
    }

    async fn temp_tick(&mut self) -> Result<()> {
        debug!("Temp tick");

        self.hid.request(request::get_temp())?;

        let temp = u16::from_le_bytes([self.hid.buffer[7], self.hid.buffer[8]]);

        let temp = if let Some(offset) = self.led_temp_offset {
            let offset = offset * 10.0;

            let total = self
                .colors
                .iter()
                .take(29)
                .flatten()
                .copied()
                .map(|v| v as f32 / 255.0)
                .sum::<f32>();

            let total = total / (29.0 * 3.0);
            let offset = (offset * total) as u16;

            temp - offset
        } else {
            temp
        };

        let temp = if let Some(offset) = self.pump_speed_temp_offset {
            let offset = offset * 10.0;

            let total = (((self.state.pump_speed.load(Ordering::Relaxed) as f32 / 2700.0) - 0.75)
                / 0.25)
                .clamp(0.0, 1.0);

            let offset = (offset * total) as u16;

            temp - offset
        } else {
            temp
        };

        debug!("Temp: {}", temp);

        self.state.coolant_temp.store(temp, Ordering::Relaxed);

        if let Some(path) = &self.coolant_temp_file {
            if let Err(e) = write(path, format!("{}\n", temp * 100)).await {
                error!("{e:}");
            }
        }

        Ok(())
    }

    async fn speed_tick(&mut self) -> Result<()> {
        debug!("Speed tick");

        self.hid.request(request::get_speeds())?;

        let pump_speed = u16::from_le_bytes([self.hid.buffer[6], self.hid.buffer[7]]);
        let fan1_speed = u16::from_le_bytes([self.hid.buffer[8], self.hid.buffer[9]]);
        let fan2_speed = u16::from_le_bytes([self.hid.buffer[10], self.hid.buffer[11]]);
        let fan3_speed = u16::from_le_bytes([self.hid.buffer[12], self.hid.buffer[13]]);
        let fan4_speed = u16::from_le_bytes([self.hid.buffer[14], self.hid.buffer[15]]);
        let fan5_speed = u16::from_le_bytes([self.hid.buffer[16], self.hid.buffer[17]]);
        let fan6_speed = u16::from_le_bytes([self.hid.buffer[18], self.hid.buffer[19]]);

        let speeds = [
            pump_speed, fan1_speed, fan2_speed, fan3_speed, fan4_speed, fan5_speed, fan6_speed,
        ];

        debug!("Speeds: {:?}", speeds);

        self.state.pump_speed.store(pump_speed, Ordering::Relaxed);

        for (path, speed) in self.fan_speed_files.iter().zip(speeds) {
            if let Err(e) = write(path, format!("{}\n", speed)).await {
                error!("Speed tick error: {e:}");
            }
        }

        Ok(())
    }

    fn write_fan_target(&mut self, in_fan: Fan, in_speed: u16) -> Result<()> {
        if in_speed
            != self.state.fan_targets[u8::try_from(in_fan)? as usize].load(Ordering::Relaxed)
        {
            info!("Set fan {in_fan:?} target to {in_speed:}");

            let idx = u8::try_from(in_fan)?;
            self.state.fan_targets[idx as usize].store(in_speed, Ordering::Relaxed);

            let mut speeds = [0; 7];
            for (i, target) in self.state.fan_targets.iter().enumerate() {
                speeds[i] = target.load(Ordering::Relaxed);
            }

            self.hid.request(request::set_speeds(speeds))?;
        }
        Ok(())
    }

    fn write_colors(&mut self, in_colors: Colors) -> Result<()> {
        debug!("Set colors");
        let mut colors = [[0; 3]; LED_COUNT_TOTAL];
        colors.copy_from_slice(&*in_colors);
        self.hid.request(request::set_colors(colors))?;
        self.colors.copy_from_slice(&colors);
        Ok(())
    }
}
