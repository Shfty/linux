use futures::StreamExt;

use std::{path::PathBuf, str::FromStr};

use anyhow::{anyhow, Error, Result};
use inotify::{EventOwned, Inotify, WatchMask};
use log::{info, warn};
use tokio::{
    fs::{read_to_string, write},
    sync::{mpsc, watch},
};

use crate::hid::validate_fan_speed;

#[derive(Debug, Copy, Clone)]
pub enum Fan {
    Pump,
    Fan(u8),
}

impl From<Fan> for u8 {
    fn from(fan: Fan) -> Self {
        match fan {
            Fan::Pump => 0,
            Fan::Fan(idx) => idx + 1,
        }
    }
}

impl TryFrom<u8> for Fan {
    type Error = Error;

    fn try_from(fan: u8) -> std::result::Result<Self, Self::Error> {
        match fan {
            0 => Ok(Fan::Pump),
            i if i <= 7 => Ok(Fan::Fan(i - 1)),
            _ => Err(anyhow!("Invalid Fan")),
        }
    }
}

impl FromStr for Fan {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pump" => Ok(Fan::Pump),
            "fan1" => Ok(Fan::Fan(0)),
            "fan2" => Ok(Fan::Fan(1)),
            "fan3" => Ok(Fan::Fan(2)),
            "fan4" => Ok(Fan::Fan(3)),
            "fan5" => Ok(Fan::Fan(4)),
            "fan6" => Ok(Fan::Fan(5)),
            _ => Err(anyhow!("Invalid Fan")),
        }
    }
}

#[derive(Debug)]
pub struct FanTargetThread {
    set_pump_speed_tx: mpsc::Sender<(Fan, u16)>,
    exit_rx: watch::Receiver<bool>,
    fan: Fan,
    path: PathBuf,
}

enum FanTargetEvent {
    Modify(std::io::Result<EventOwned>),
    RunningChanged(bool),
}

impl FanTargetThread {
    pub fn new(
        set_pump_speed_tx: mpsc::Sender<(Fan, u16)>,
        exit_tx: watch::Receiver<bool>,
        fan: Fan,
        path: PathBuf,
    ) -> Self {
        FanTargetThread {
            set_pump_speed_tx,
            exit_rx: exit_tx,
            fan,
            path,
        }
    }

    pub async fn run(self) -> Result<()> {
        let on_change = || {
            let fan = self.fan;
            let path = self.path.clone();
            let set_pump_speed_tx = self.set_pump_speed_tx.clone();
            async move {
                let file_string = read_to_string(&path).await?;
                let file_string = file_string.strip_suffix('\n').unwrap_or(&file_string);
                if let Ok(speed) = file_string.parse::<u16>() {
                    let speed = validate_fan_speed(speed);
                    set_pump_speed_tx.send((fan, speed)).await?;
                } else {
                    warn!("invalid pump speed, resetting file");
                    write(&path, "100\n").await?;
                }

                Ok(()) as Result<()>
            }
        };

        if !self.path.exists() {
            info!("Fan target file {:?} does not exist, creating...", &self.path);
            write(&self.path, "100\n").await?;
        }

        let mut inotify = Inotify::init()?;
        inotify.add_watch(&self.path, WatchMask::MODIFY)?;

        let mut buf = [0; 1024];
        let events = inotify.event_stream(&mut buf)?;

        let exit = tokio_stream::wrappers::WatchStream::new(self.exit_rx);

        let mut events = futures::stream_select!(
            events.map(FanTargetEvent::Modify),
            exit.map(FanTargetEvent::RunningChanged),
        );

        on_change().await?;

        while let Some(event) = events.next().await {
            match event {
                FanTargetEvent::Modify(event) => {
                    event?;
                    on_change().await?
                }
                FanTargetEvent::RunningChanged(running) => {
                    if !running {
                        info!("PumpTargetThread got exit event");
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}
