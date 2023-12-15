use std::path::Path;

use anyhow::Result;

use evdev::{EventType, Key};
use futures::StreamExt;

use log::{debug, error, info};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::mpsc,
};
use tokio_stream::wrappers::{ReceiverStream, SignalStream};

trait Then<R>: Sized {
    fn then(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}

impl<R, T> Then<R> for T {}

#[derive(Debug, Clone)]
pub enum HotasEvent {
    EvdevInput { events: Vec<evdev::InputEvent> },
    Exit,
}

fn main() -> ! {
    env_logger::init();

    std::process::exit(match run() {
        Ok(_) => 0,
        Err(e) => {
            error!("{e:}");
            1
        }
    })
}

fn run() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _handle = runtime.enter();
    runtime.block_on(run_async())
}

async fn run_async() -> Result<()> {
    // Setup exit handler
    let (running_tx, running_rx) = tokio::sync::watch::channel(true);

    // Setup communication channels
    let (stick_tx, stick_rx) = mpsc::channel::<Vec<evdev::InputEvent>>(1);

    // Start evdev tasks
    debug!("Starting evdev tasks");
    let stick_handle = main_evdev::run(
        Path::new(
            "/dev/input/by-id/usb-Mad_Catz__Inc._MadCatz_FightStick_Neo_114A9328-event-joystick",
        ),
        running_rx.clone(),
        stick_tx,
    );

    // Event stream
    debug!("Creating evdev event streams");
    let stick_stream = ReceiverStream::new(stick_rx);

    let evdev_events = stick_stream.map(|events| HotasEvent::EvdevInput { events });

    // Exit stream
    debug!("Creating exit event stream");
    let exit = futures::stream_select!(
        SignalStream::new(signal(SignalKind::interrupt())?),
        SignalStream::new(signal(SignalKind::hangup())?),
        SignalStream::new(signal(SignalKind::terminate())?),
    )
    .map(|_| HotasEvent::Exit);

    // Hotas stream
    let mut events = futures::stream_select!(evdev_events, exit);

    // Main loop
    info!("Entering main loop");

    while let Some(event) = events.next().await {
        match event {
            HotasEvent::EvdevInput { events } => {
                debug!("Evdev input: {events:?}");

                let event = events.into_iter().find_map(|event| {
                    match event.event_type() {
                        EventType::KEY => {
                            match Key(event.code()) {
                                // Main cluster
                                Key::BTN_MODE => Some(event.value()),

                                // Unhandled
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                });

                if let Some(1) = event {
                    info!("Home pressed");

                    let mut command = std::process::Command::new("pgrep");
                    command.arg("-x").arg("retroarch");
                    let output = if let Ok(output) = command.output() {
                        String::from_utf8(output.stdout).unwrap()
                    } else {
                        continue;
                    };

                    let output = output.strip_suffix("\n").unwrap_or(&output);

                    if output.is_empty() {
                        std::thread::spawn(|| {
                            std::process::Command::new("adaptive-sync-run").arg("retroarch")
                                .spawn()
                                .unwrap()
                                .wait()
                                .unwrap()
                        });
                    }
                }
            }
            HotasEvent::Exit => break,
        }
    }

    info!("Hotas finalizing");
    running_tx.send(false)?;

    debug!("Joining threads");
    if let Err(e) = stick_handle.await {
        error!("Failed to join thread: {e:?}");
    }

    debug!("Done");

    Ok(())
}

mod main_evdev {
    use std::{path::Path, time::Duration};

    use evdev::EventType;
    use tokio::{
        sync::{mpsc::Sender, watch},
        task::{self, JoinHandle},
    };
    use tokio_stream::wrappers::WatchStream;

    use anyhow::{anyhow, Result};

    use futures::StreamExt;
    use log::{debug, error, info};

    enum EvdevEvent {
        Input(std::result::Result<evdev::InputEvent, std::io::Error>),
        RunningChanged(bool),
    }

    pub fn run<P: AsRef<Path>>(
        path: P,
        exit: watch::Receiver<bool>,
        sender: Sender<Vec<evdev::InputEvent>>,
    ) -> JoinHandle<()> {
        let path = path.as_ref().to_owned();
        task::spawn(async move {
            loop {
                let exit = WatchStream::new(exit.clone());
                match run_async(&path, exit, sender.clone()).await {
                    Ok(_) => return,
                    Err(e) => error!("Thread Error: {e:}"),
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        })
    }

    pub async fn run_async(
        path: &Path,
        exit: WatchStream<bool>,
        sender: Sender<Vec<evdev::InputEvent>>,
    ) -> Result<()> {
        debug!("Opening {path:?}");
        let device = evdev::Device::open(path)?;
        info!("Opened at {path:?}");

        debug!("Fetching driver version");
        let (major, minor, patch) = device.driver_version();
        info!("Driver version {major:}.{minor:}.{patch:}");

        debug!("Fetching name");
        let name = device.name().ok_or(anyhow!("Device has no name"))?;
        info!("Device name: {name:}");

        let evdev_events = device.into_event_stream()?;

        let mut events = futures::stream_select!(
            evdev_events.map(|event| EvdevEvent::Input(event)),
            exit.map(|exit| { EvdevEvent::RunningChanged(exit) })
        );

        let mut events_buf = vec![];
        while let Some(event) = events.next().await {
            match event {
                EvdevEvent::Input(event) => {
                    let event = event?;
                    events_buf.push(event);
                    match event.event_type() {
                        EventType::KEY | EventType::ABSOLUTE => {
                            events_buf.push(event);
                        }
                        EventType::SYNCHRONIZATION => {
                            if let Err(e) =
                                sender.send(events_buf.drain(..).collect::<Vec<_>>()).await
                            {
                                info!("Evdev thread failed to send: {e:}");
                                break;
                            }
                        }
                        _ => (),
                    }
                }
                EvdevEvent::RunningChanged(running) => {
                    if !running {
                        debug!("Thread received exit event");
                        break;
                    }
                }
            }
        }

        info!("Thread finalizing");

        Ok(())
    }
}
