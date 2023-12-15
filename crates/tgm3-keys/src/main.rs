use std::{fmt::Display, path::Path};

use anyhow::{anyhow, Result};

use evdev::{AbsoluteAxisType, EventType, Key};
use futures::StreamExt;

use log::{debug, error, info};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::mpsc,
};
use tokio_stream::wrappers::{ReceiverStream, SignalStream};
use uinput_tokio::{
    device,
    event::{absolute::Hat, Keyboard},
    Event,
};

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
    let t16k_handle = main_evdev::run(
        Path::new(
            "/dev/input/by-id/usb-Mad_Catz__Inc._MadCatz_FightStick_Neo_114A9328-event-joystick",
        ),
        running_rx.clone(),
        stick_tx,
    );

    // Setup uinput
    debug!("Setting up uinput");
    let mut device = uinput_tokio::default()
        .map_err(map_uinput_error)?
        .bus(3)
        .vendor(0x3)
        .product(0x3)
        .then(setup_name("TGM3-Keys"))?;

    let absolute_axes: [(Event, _, _, _, _); 4] = [
        (Hat::X0.into(), 0, 5, 0, 0),
        (Hat::Y0.into(), 0, 5, 0, 0),
        (Hat::X1.into(), 0, 5, 0, 0),
        (Hat::Y1.into(), 0, 5, 0, 0),
    ];

    let buttons: [Event; 9] = [
        Keyboard::Key(uinput_tokio::event::keyboard::Key::Q).into(),
        Keyboard::Key(uinput_tokio::event::keyboard::Key::W).into(),
        Keyboard::Key(uinput_tokio::event::keyboard::Key::E).into(),
        Keyboard::Key(uinput_tokio::event::keyboard::Key::R).into(),
        Keyboard::Key(uinput_tokio::event::keyboard::Key::J).into(),
        Keyboard::Key(uinput_tokio::event::keyboard::Key::K).into(),
        Keyboard::Key(uinput_tokio::event::keyboard::Key::L).into(),
        Keyboard::Key(uinput_tokio::event::keyboard::Key::P).into(),
        Keyboard::Key(uinput_tokio::event::keyboard::Key::Enter).into(),
    ];

    for (axis, min, max, flat, fuzz) in absolute_axes.into_iter() {
        device = device.then(setup_absolute(axis, min, max, flat, fuzz))?;
    }

    for event in buttons {
        device = device.then(setup_event(event))?;
    }

    debug!("Creating uinput device");
    let mut device = device.create().await.map_err(into_anyhow)?;

    // Event stream
    debug!("Creating evdev event streams");
    let t16k_stream = ReceiverStream::new(stick_rx);

    let evdev_events = t16k_stream.map(|events| HotasEvent::EvdevInput { events });

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

    let mut hat_state = (0, 0);
    while let Some(event) = events.next().await {
        match event {
            HotasEvent::EvdevInput { events } => {
                debug!("Evdev input: {events:?}");

                let events = events
                    .into_iter()
                    .flat_map(|event| remap_event(&mut hat_state, event))
                    .collect::<Vec<_>>();

                for (event, value) in events {
                    device.send(event, value).await.map_err(into_anyhow)?;
                }
                device.synchronize().await.map_err(into_anyhow)?;
            }
            HotasEvent::Exit => break,
        }
    }

    info!("Hotas finalizing");
    running_tx.send(false)?;

    debug!("Dropping uinput device");
    drop(device);

    debug!("Joining threads");
    if let Err(e) = t16k_handle.await {
        error!("Failed to join thread: {e:?}");
    }

    debug!("Done");

    Ok(())
}

fn setup_name(name: &str) -> impl FnOnce(device::Builder) -> Result<device::Builder> + '_ {
    move |builder| builder.name(name).map_err(map_uinput_error)
}

fn setup_event<E: Into<Event>>(
    event: E,
) -> impl FnOnce(device::Builder) -> Result<device::Builder> {
    move |builder| builder.event(event).map_err(into_anyhow)
}

fn setup_absolute<E: Into<Event>>(
    event: E,
    min: i32,
    max: i32,
    flat: i32,
    fuzz: i32,
) -> impl FnOnce(device::Builder) -> Result<device::Builder> {
    move |builder| {
        Ok(builder
            .event(event)
            .map_err(into_anyhow)?
            .min(min)
            .max(max)
            .flat(flat)
            .fuzz(fuzz))
    }
}

fn into_anyhow<E: Display>(e: E) -> anyhow::Error {
    anyhow!("{e:}")
}

/// Uinput error transformer to avoid stack overflow when printing a NotFound variant
fn map_uinput_error(e: uinput_tokio::Error) -> anyhow::Error {
    match e {
        uinput_tokio::Error::Nix(e) => anyhow!("Nix error: {e:}"),
        uinput_tokio::Error::Nul(e) => anyhow!("Nul error: {e:}"),
        uinput_tokio::Error::Udev(e) => anyhow!("Udev error: {e:}"),
        uinput_tokio::Error::IoError(e) => anyhow!("I/O error: {e:}"),
        uinput_tokio::Error::NotFound => anyhow!("uinput file not found"),
    }
}

mod main_evdev {
    use std::path::Path;

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
        let exit = WatchStream::new(exit);
        task::spawn(async move {
            match run_async(&path, exit, sender).await {
                Ok(_) => (),
                Err(e) => error!("Thread Error: {e:}"),
            }
        })
    }

    pub async fn run_async(
        path: &Path,
        exit: WatchStream<bool>,
        sender: Sender<Vec<evdev::InputEvent>>,
    ) -> Result<()> {
        debug!("Opening {path:?}");
        let mut device = evdev::Device::open(path)?;
        info!("Opened at {path:?}");

        debug!("Fetching driver version");
        let (major, minor, patch) = device.driver_version();
        info!("Driver version {major:}.{minor:}.{patch:}");

        debug!("Fetching name");
        let name = device.name().ok_or(anyhow!("Device has no name"))?;
        info!("Device name: {name:}");

        debug!("Grabbing");
        device.grab()?;
        info!("Grabbed");

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

        debug!("Ungrabbing");
        let mut device = evdev::Device::open(path)?;
        device.ungrab()?;
        info!("Ungrabbed");

        Ok(())
    }
}

fn remap_event(
    hat_state: &mut (i32, i32),
    event: evdev::InputEvent,
) -> Option<(uinput_tokio::Event, i32)> {
    match event.event_type() {
        EventType::ABSOLUTE => remap_absolute(hat_state, event),
        EventType::KEY => remap_key(event),
        _ => None,
    }
}

fn remap_absolute(
    hat_state: &mut (i32, i32),
    event: evdev::InputEvent,
) -> Option<(uinput_tokio::Event, i32)> {
    Some(match AbsoluteAxisType(event.code()) {
        AbsoluteAxisType::ABS_HAT0X => {
            let out = match (hat_state.0, event.value()) {
                (0, -1) => (
                    Keyboard::Key(uinput_tokio::event::keyboard::Key::Q).into(),
                    1,
                ),
                (-1, 0) => (
                    Keyboard::Key(uinput_tokio::event::keyboard::Key::Q).into(),
                    0,
                ),
                (0, 1) => (
                    Keyboard::Key(uinput_tokio::event::keyboard::Key::R).into(),
                    1,
                ),
                (1, 0) => (
                    Keyboard::Key(uinput_tokio::event::keyboard::Key::R).into(),
                    0,
                ),
                _ => return None,
            };
            hat_state.0 = event.value();
            out
        }
        AbsoluteAxisType::ABS_HAT0Y => {
            let out = match (hat_state.1, event.value()) {
                (0, -1) => (
                    Keyboard::Key(uinput_tokio::event::keyboard::Key::W).into(),
                    1,
                ),
                (-1, 0) => (
                    Keyboard::Key(uinput_tokio::event::keyboard::Key::W).into(),
                    0,
                ),
                (0, 1) => (
                    Keyboard::Key(uinput_tokio::event::keyboard::Key::E).into(),
                    1,
                ),
                (1, 0) => (
                    Keyboard::Key(uinput_tokio::event::keyboard::Key::E).into(),
                    0,
                ),
                _ => return None,
            };
            hat_state.1 = event.value();
            out
        }
        _ => return None,
    })
}

fn remap_key(event: evdev::InputEvent) -> Option<(uinput_tokio::Event, i32)> {
    let ty = match Key(event.code()) {
        // Main cluster
        Key::BTN_SOUTH => Keyboard::Key(uinput_tokio::event::keyboard::Key::P).into(),
        Key::BTN_EAST => Keyboard::Key(uinput_tokio::event::keyboard::Key::J).into(),
        Key::BTN_NORTH => Keyboard::Key(uinput_tokio::event::keyboard::Key::K).into(),
        Key::BTN_WEST => Keyboard::Key(uinput_tokio::event::keyboard::Key::L).into(),
        Key::BTN_START => Keyboard::Key(uinput_tokio::event::keyboard::Key::Enter).into(),

        // Unhandled
        _ => return None,
    };

    Some((ty, event.value()))
}
