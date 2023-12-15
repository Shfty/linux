use std::{collections::BTreeMap, fmt::Display, ops::Range, path::Path};

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
    event::{
        absolute::{Hat, Position, Wheel},
        controller::JoyStick,
    },
    Event,
};

trait Then<R>: Sized {
    fn then(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}

impl<R, T> Then<R> for T {}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HotasDevice {
    T16000,
    TWCS,
    TFRP,
}

#[derive(Debug, Clone)]
pub enum HotasEvent {
    EvdevInput {
        device: HotasDevice,
        events: Vec<(HotasDevice, evdev::InputEvent)>,
    },
    Exit,
}

impl Display for HotasDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HotasDevice::T16000 => "T16000",
            HotasDevice::TWCS => "TWCS",
            HotasDevice::TFRP => "TFRP",
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Mode {
    MechWarrior5,
    GPolice,
}

const MODE: Mode = Mode::GPolice;

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

const T16000_RANGE_XY: i32 = 16383;
const T16000_RANGE_THROTTLE: i32 = 255;
const TWCS_RANGE_XY: i32 = 1023;
const TWCS_RANGE_Z: i32 = 65535;
const TWCS_RANGE_RUDDER: i32 = 1023;
const TWCS_RANGE_WHEEL: i32 = 1023;
const TFRP_RANGE_XY: i32 = 1023;

async fn run_async() -> Result<()> {
    // Setup exit handler
    let (running_tx, running_rx) = tokio::sync::watch::channel(true);

    // Setup communication channels
    let (t16k_tx, t16k_rx) = mpsc::channel::<Vec<(HotasDevice, evdev::InputEvent)>>(1);
    let (twcs_tx, twcs_rx) = mpsc::channel::<Vec<(HotasDevice, evdev::InputEvent)>>(1);
    let (tfrp_tx, tfrp_rx) = mpsc::channel::<Vec<(HotasDevice, evdev::InputEvent)>>(1);

    // Start evdev tasks
    debug!("Starting evdev tasks");
    let t16k_handle = main_evdev::run(
        HotasDevice::T16000,
        Path::new("/dev/input/by-id/usb-Thrustmaster_T.16000M-event-joystick"),
        running_rx.clone(),
        t16k_tx,
    );

    let twcs_handle = main_evdev::run(
        HotasDevice::TWCS,
        Path::new("/dev/input/by-id/usb-Thrustmaster_TWCS_Throttle-event-joystick"),
        running_rx.clone(),
        twcs_tx,
    );

    let tfrp_handle = main_evdev::run(
        HotasDevice::TFRP,
        Path::new("/dev/input/by-id/usb-Thrustmaster_T-Rudder-event-if00"),
        running_rx.clone(),
        tfrp_tx,
    );

    // Setup uinput
    debug!("Setting up uinput");
    let mut device = uinput_tokio::default()
        .map_err(map_uinput_error)?
        .bus(3)
        .vendor(0x3)
        .product(0x3)
        .then(setup_name("HOTAS"))?;

    let absolute_axes: [(Event, _, _, _, _); 13] = if MODE == Mode::MechWarrior5 {
        [
            (Position::X.into(), 0, T16000_RANGE_XY * 2, 0, 0),
            (Position::Y.into(), 0, T16000_RANGE_XY * 2, 0, 0),
            (Position::Z.into(), 0, TWCS_RANGE_Z * 2, 0, 0),
            (Position::RX.into(), 0, TWCS_RANGE_XY * 2, 0, 0),
            (Position::RY.into(), 0, TWCS_RANGE_XY * 2, 0, 0),
            (Position::RZ.into(), 0, TFRP_RANGE_XY * 2 * 2, 0, 0),
            (Wheel::Throttle.into(), 0, T16000_RANGE_THROTTLE * 2, 0, 0),
            (Wheel::Rudder.into(), 0, TWCS_RANGE_RUDDER, 0, 0),
            (Wheel::Position.into(), 0, TWCS_RANGE_WHEEL, 0, 0),
            (Hat::X0.into(), 0, 5, 0, 0),
            (Hat::Y0.into(), 0, 5, 0, 0),
            (Hat::X1.into(), 0, 5, 0, 0),
            (Hat::Y1.into(), 0, 5, 0, 0),
        ]
    } else {
        [
            (Position::X.into(), 0, T16000_RANGE_XY, 0, 0),
            (Position::Y.into(), 0, T16000_RANGE_XY, 0, 0),
            (Position::Z.into(), 0, TWCS_RANGE_Z, 0, 0),
            (Position::RX.into(), 0, TWCS_RANGE_XY, 0, 0),
            (Position::RY.into(), 0, TWCS_RANGE_XY, 0, 0),
            (Position::RZ.into(), 0, TFRP_RANGE_XY, 0, 0),
            (Wheel::Throttle.into(), 0, T16000_RANGE_THROTTLE, 0, 0),
            (Wheel::Rudder.into(), 0, TWCS_RANGE_RUDDER, 0, 0),
            (Wheel::Position.into(), 0, TWCS_RANGE_WHEEL, 0, 0),
            (Hat::X0.into(), 0, 5, 0, 0),
            (Hat::Y0.into(), 0, 5, 0, 0),
            (Hat::X1.into(), 0, 5, 0, 0),
            (Hat::Y1.into(), 0, 5, 0, 0),
        ]
    };

    let buttons: [Event; 13] = [
        JoyStick::Trigger.into(),
        JoyStick::Thumb.into(),
        JoyStick::Thumb2.into(),
        JoyStick::Top.into(),
        JoyStick::Top2.into(),
        JoyStick::Pinkie.into(),
        JoyStick::Base.into(),
        JoyStick::Base2.into(),
        JoyStick::Base3.into(),
        JoyStick::Base4.into(),
        JoyStick::Base5.into(),
        JoyStick::Base6.into(),
        JoyStick::Dead.into(),
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
    let t16k_stream = ReceiverStream::new(t16k_rx);
    let twcs_stream = ReceiverStream::new(twcs_rx);
    let tfrp_stream = ReceiverStream::new(tfrp_rx);

    let evdev_events = futures::stream_select!(
        t16k_stream.map(|events| (HotasDevice::T16000, events)),
        twcs_stream.map(|events| (HotasDevice::TWCS, events)),
        tfrp_stream.map(|events| (HotasDevice::TFRP, events))
    )
    .map(|(device, events)| HotasEvent::EvdevInput { device, events });

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

    let mut left_pedal = 0;
    let mut right_pedal = 0;

    while let Some(event) = events.next().await {
        match event {
            HotasEvent::EvdevInput {
                device: device_ty,
                events,
            } => {
                debug!("Evdev input from {device_ty:}: {events:?}");

                let events = events
                    .into_iter()
                    .flat_map(|event| remap_event(event, &mut left_pedal, &mut right_pedal))
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
    for (device, handle) in [
        (HotasDevice::T16000, t16k_handle),
        (HotasDevice::TWCS, twcs_handle),
        (HotasDevice::TFRP, tfrp_handle),
    ] {
        if let Err(e) = handle.await {
            error!("Failed to join {device:} thread: {e:?}");
        }
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

    use crate::HotasDevice;
    use anyhow::{anyhow, Result};

    use futures::StreamExt;
    use log::{debug, error, info};

    enum EvdevEvent {
        Input(std::result::Result<evdev::InputEvent, std::io::Error>),
        RunningChanged(bool),
    }

    pub fn run<P: AsRef<Path>>(
        device_ty: HotasDevice,
        path: P,
        exit: watch::Receiver<bool>,
        sender: Sender<Vec<(HotasDevice, evdev::InputEvent)>>,
    ) -> JoinHandle<()> {
        let path = path.as_ref().to_owned();
        let exit = WatchStream::new(exit);
        task::spawn(async move {
            match run_async(device_ty, &path, exit, sender).await {
                Ok(_) => (),
                Err(e) => error!("TFRP Thread Error: {e:}"),
            }
        })
    }

    pub async fn run_async(
        device_ty: HotasDevice,
        path: &Path,
        exit: WatchStream<bool>,
        sender: Sender<Vec<(HotasDevice, evdev::InputEvent)>>,
    ) -> Result<()> {
        debug!("{device_ty:}: Opening {path:?}");
        let mut device = evdev::Device::open(path)?;
        info!("{device_ty:} opened at {path:?}");

        debug!("{device_ty:}: Fetching driver version");
        let (major, minor, patch) = device.driver_version();
        info!("{device_ty:} driver version {major:}.{minor:}.{patch:}");

        debug!("{device_ty:}: Fetching name");
        let name = device.name().ok_or(anyhow!("Device has no name"))?;
        info!("{device_ty:} device name: {name:}");

        debug!("{device_ty:}: Grabbing");
        device.grab()?;
        info!("{device_ty:} Grabbed");

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
                    events_buf.push((device_ty, event));
                    match event.event_type() {
                        EventType::KEY | EventType::ABSOLUTE => {
                            events_buf.push((device_ty, event));
                        }
                        EventType::SYNCHRONIZATION => {
                            if let Err(e) =
                                sender.send(events_buf.drain(..).collect::<Vec<_>>()).await
                            {
                                info!("{device_ty:} evdev thread failed to send: {e:}");
                                break;
                            }
                        }
                        _ => (),
                    }
                }
                EvdevEvent::RunningChanged(running) => {
                    if !running {
                        debug!("{device_ty:} thread received exit event");
                        break;
                    }
                }
            }
        }

        info!("{device_ty:} thread finalizing");

        debug!("{device_ty:} ungrabbing");
        let mut device = evdev::Device::open(path)?;
        device.ungrab()?;
        info!("{device_ty:} ungrabbed");

        Ok(())
    }
}

fn remap_event(
    (device, event): (HotasDevice, evdev::InputEvent),
    left_pedal: &mut i32,
    right_pedal: &mut i32,
) -> Option<(uinput_tokio::Event, i32)> {
    match (device, event.event_type()) {
        (HotasDevice::T16000, EventType::ABSOLUTE) => remap_t16000m_absolute(event),
        (HotasDevice::T16000, EventType::KEY) => remap_t16000m_key(event),
        (HotasDevice::TWCS, EventType::ABSOLUTE) => remap_twcs_absolute(event),
        (HotasDevice::TWCS, EventType::KEY) => remap_twcs_key(event),
        (HotasDevice::TFRP, EventType::ABSOLUTE) => {
            remap_tfrp_absolute(event, left_pedal, right_pedal)
        }

        _ => None,
    }
}

fn remaps_t16000m() -> BTreeMap<u16, Event> {
    [
        (AbsoluteAxisType::ABS_X.0, Position::X.into()),
        (AbsoluteAxisType::ABS_Y.0, Position::Y.into()),
        (AbsoluteAxisType::ABS_THROTTLE.0, Wheel::Throttle.into()),
        (AbsoluteAxisType::ABS_HAT0X.0, Hat::X0.into()),
        (AbsoluteAxisType::ABS_HAT0Y.0, Hat::Y0.into()),
    ]
    .into_iter()
    .collect()
}

fn remap_t16000m_absolute(event: evdev::InputEvent) -> Option<(uinput_tokio::Event, i32)> {
    let value = event.value();
    let value = match MODE {
        Mode::MechWarrior5 => {
            if event.code() == AbsoluteAxisType::ABS_X.0
                || event.code() == AbsoluteAxisType::ABS_Y.0
            {
                value + T16000_RANGE_XY
            } else if event.code() == AbsoluteAxisType::ABS_THROTTLE.0 {
                value + T16000_RANGE_THROTTLE
            } else if event.code() == AbsoluteAxisType::ABS_HAT0X.0
                || event.code() == AbsoluteAxisType::ABS_HAT0Y.0
            {
                value + 3
            } else {
                value
            }
        }
        Mode::GPolice => {
            let range = T16000_RANGE_XY as f32;

            if event.code() == AbsoluteAxisType::ABS_X.0
                || event.code() == AbsoluteAxisType::ABS_Y.0
            {
                // Convert to float
                let value = value as f32;

                // Map to 0..1 range
                let value = value.map_from(0.0..range);

                // Map to 0..1 range
                let value = value.map_to(-1.0..1.0);

                let value = if event.code() == AbsoluteAxisType::ABS_Y.0 {
                    // Apply trim
                    value + 0.01
                } else {
                    value
                };

                // Calculate sign
                let sign = value.signum();

                // Apply curve
                let value = value.abs().powf(2.0) * sign;

                // Apply inverse deadzone
                let value = value.abs().map_to(
                    if event.code() == AbsoluteAxisType::ABS_X.0 {
                        0.199
                    } else {
                        0.2
                    }..1.0,
                ) * sign;

                // Map to unit range
                let value = value.map_from(-1.0..1.0);

                // Map to output range
                let value = value.map_to(0.0..range).round() as i32;

                if event.code() == AbsoluteAxisType::ABS_X.0 {
                    info!("Value: {value:}");
                }

                value
            } else {
                value
            }
        }
    };

    remaps_t16000m().get(&event.code()).map(|ty| (*ty, value))
}

fn remaps_twcs() -> BTreeMap<u16, Event> {
    [
        (AbsoluteAxisType::ABS_X.0, Position::RX.into()),
        (AbsoluteAxisType::ABS_Y.0, Position::RY.into()),
        (AbsoluteAxisType::ABS_Z.0, Position::Z.into()),
        (AbsoluteAxisType::ABS_RUDDER.0, Wheel::Position.into()),
        (AbsoluteAxisType::ABS_HAT0X.0, Hat::X1.into()),
        (AbsoluteAxisType::ABS_HAT0Y.0, Hat::Y1.into()),
    ]
    .into_iter()
    .collect()
}

fn remap_twcs_absolute(event: evdev::InputEvent) -> Option<(uinput_tokio::Event, i32)> {
    let value = event.value();
    let value = match MODE {
        Mode::MechWarrior5 => {
            if event.code() == AbsoluteAxisType::ABS_X.0
                || event.code() == AbsoluteAxisType::ABS_Y.0
            {
                value + TWCS_RANGE_XY
            } else if event.code() == AbsoluteAxisType::ABS_Z.0 {
                value + TWCS_RANGE_Z
            } else if event.code() == AbsoluteAxisType::ABS_RUDDER.0 {
                value + TWCS_RANGE_RUDDER
            } else if event.code() == AbsoluteAxisType::ABS_HAT0X.0
                || event.code() == AbsoluteAxisType::ABS_HAT0Y.0
            {
                value + 3
            } else {
                value
            }
        }
        Mode::GPolice => {
            if event.code() == AbsoluteAxisType::ABS_Z.0 {
                let range = TWCS_RANGE_Z as f32;
                let value = value as f32;

                // Map to 0..1 range
                let value = value.map_from(0.0..range);

                // Map to -1..1 range
                let value = value.map_to(-1.0..1.0);

                // Calculate sign
                let sign = value.signum();

                // Apply curve
                let value = value.abs().powf(2.0) * sign;

                // Apply inverse deadzone
                let value = value.abs().map_to(0.38..1.0) * sign;

                // Map to unit range
                let value = value.map_from(-1.0..1.0);

                // Map to output range
                let value = value.map_to(0.0..range).round() as i32;

                value
            } else {
                value
            }
        }
    };

    remaps_twcs().get(&event.code()).map(|ty| (*ty, value))
}

fn remap_tfrp_absolute(
    event: evdev::InputEvent,
    left_pedal: &mut i32,
    right_pedal: &mut i32,
) -> Option<(uinput_tokio::Event, i32)> {
    match AbsoluteAxisType(event.code()) {
        AbsoluteAxisType::ABS_X => {
            *right_pedal = event.value();
        }
        AbsoluteAxisType::ABS_Y => {
            *left_pedal = event.value();
        }
        _ => (),
    };

    let val = if MODE == Mode::MechWarrior5 {
        (TFRP_RANGE_XY * 3) + *left_pedal - *right_pedal
    } else {
        (TFRP_RANGE_XY + *left_pedal - *right_pedal) / 2
    };
    debug!("Pedal value: {val:}");

    Some((Position::RZ.into(), val))
}

fn remap_t16000m_key(event: evdev::InputEvent) -> Option<(uinput_tokio::Event, i32)> {
    let ty = match Key(event.code()) {
        // Main cluster
        Key::BTN_TRIGGER => JoyStick::Trigger.into(),
        Key::BTN_THUMB => JoyStick::Thumb.into(),
        Key::BTN_THUMB2 => JoyStick::Thumb2.into(),
        Key::BTN_TOP => JoyStick::Top.into(),

        /*
        // Left cluster
        Key::BTN_TOP2 => TriggerHappy::_5.into(),
        Key::BTN_PINKIE => TriggerHappy::_6.into(),
        Key::BTN_BASE => TriggerHappy::_7.into(),
        Key::BTN_BASE2 => TriggerHappy::_8.into(),
        Key::BTN_BASE3 => TriggerHappy::_9.into(),
        Key::BTN_BASE4 => TriggerHappy::_10.into(),

        // Right cluster
        Key::BTN_BASE5 => TriggerHappy::_11.into(),
        Key::BTN_BASE6 => TriggerHappy::_12.into(),
        Key(0x12c) => TriggerHappy::_13.into(),
        Key(0x12d) => TriggerHappy::_14.into(),
        Key(0x12e) => TriggerHappy::_15.into(),
        Key::BTN_DEAD => TriggerHappy::_16.into(),
        */
        // Unhandled
        _ => return None,
    };

    Some((ty, event.value()))
}

fn remap_twcs_key(event: evdev::InputEvent) -> Option<(uinput_tokio::Event, i32)> {
    let ty = match Key(event.code()) {
        // Thumb button
        Key::BTN_TRIGGER => JoyStick::Top2.into(),

        // Pinky and ring buttons
        Key::BTN_THUMB => JoyStick::Pinkie.into(),
        Key::BTN_THUMB2 => JoyStick::Base.into(),

        // Rocker switch
        Key::BTN_TOP => JoyStick::Base2.into(),
        Key::BTN_TOP2 => JoyStick::Base3.into(),

        // Mini stick click
        //Key::BTN_PINKIE => TriggerHappy::_22.into(),

        // Middle hat
        Key::BTN_BASE => JoyStick::Base4.into(),
        Key::BTN_BASE2 => JoyStick::Base5.into(),
        Key::BTN_BASE3 => JoyStick::Base6.into(),
        Key::BTN_BASE4 => JoyStick::Dead.into(),

        /*
        // Bottom hat
        Key::BTN_BASE5 => TriggerHappy::_27.into(),
        Key::BTN_BASE6 => TriggerHappy::_28.into(),
        Key(0x12c) => TriggerHappy::_29.into(),
        Key(0x12d) => TriggerHappy::_30.into(),
        */
        // Unhandled
        _ => return None,
    };

    Some((ty, event.value()))
}

pub trait MapFrom: Sized {
    /// Map the provided number from the provided range to 0..1
    fn map_from(self, from: Range<Self>) -> Self;
}

impl MapFrom for f32 {
    fn map_from(self, from: Range<Self>) -> Self {
        (self - from.start) / (from.end - from.start)
    }
}

pub trait MapTo: Sized {
    /// Map the provided number from 0..1 to the provided range
    fn map_to(self, to: Range<Self>) -> Self;
}

impl MapTo for f32 {
    fn map_to(self, to: Range<Self>) -> Self {
        to.start + (to.end - to.start) * self
    }
}

pub trait LinearMap: MapFrom + MapTo {
    /// Map the provided number from one range to another
    fn linear_map(self, from: Range<Self>, to: Range<Self>) -> Self {
        self.map_from(from).map_to(to)
    }
}

impl LinearMap for f32 {}
