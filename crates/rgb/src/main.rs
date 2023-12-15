use futures::stream::{FuturesUnordered, StreamExt};
use openrgb::{
    data::{Color as OpenRGBColor, Controller},
    OpenRGB, OpenRGBError,
};
use std::{
    collections::BTreeMap,
    error::Error,
    ops::{Add, Div, Mul, Sub},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::net::TcpStream;

const LOCATION_RAM_LEFT: &'static str = "I2C: /dev/i2c-8, address 0x5A";
const LOCATION_RAM_RIGHT: &'static str = "I2C: /dev/i2c-8, address 0x5B";
const LOCATION_GPU: &'static str = "HID: /dev/hidraw4";
const LOCATION_SYSTEM_FANS: &'static str = "HID: /dev/hidraw9";
const LOCATION_RADIATOR_FANS: &'static str = "HID: /dev/hidraw11";

const LOCATIONS: [&'static str; 5] = [
    LOCATION_RAM_LEFT,
    LOCATION_RAM_RIGHT,
    LOCATION_GPU,
    LOCATION_SYSTEM_FANS,
    LOCATION_RADIATOR_FANS,
];

const SLEEP_DURATION: Duration = Duration::from_millis(16);

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ControllerId {
    RamLeft,
    RamRight,
    Gpu,
    SystemFans,
    RadiatorFans,
}

impl TryFrom<&str> for ControllerId {
    type Error = &'static str;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            LOCATION_RAM_LEFT => Ok(ControllerId::RamLeft),
            LOCATION_RAM_RIGHT => Ok(ControllerId::RamRight),
            LOCATION_GPU => Ok(ControllerId::Gpu),
            LOCATION_SYSTEM_FANS => Ok(ControllerId::SystemFans),
            LOCATION_RADIATOR_FANS => Ok(ControllerId::RadiatorFans),
            _ => Err("Invalid controller ID"),
        }
    }
}

struct Controllers(BTreeMap<ControllerId, Controller>);

impl Controllers {
    pub async fn new(client: &OpenRGB<TcpStream>) -> Result<Self, OpenRGBError> {
        let count = client.get_controller_count().await?;
        let controllers = (0..count)
            .into_iter()
            .map(|i| client.get_controller(i))
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .flat_map(Result::ok)
            .filter_map(|controller| {
                if LOCATIONS.contains(&controller.location.as_str()) {
                    Some((
                        ControllerId::try_from(controller.location.as_str()).unwrap(),
                        controller,
                    ))
                } else {
                    None
                }
            })
            .collect::<BTreeMap<_, _>>();

        Ok(Controllers(controllers))
    }

    pub fn get(&self, id: &ControllerId) -> &Controller {
        self.0.get(id).unwrap()
    }

    pub fn ram_left(&self) -> &Controller {
        self.get(&ControllerId::RamLeft)
    }

    pub fn ram_right(&self) -> &Controller {
        self.get(&ControllerId::RamRight)
    }

    pub fn gpu(&self) -> &Controller {
        self.get(&ControllerId::Gpu)
    }

    pub fn system_fans(&self) -> &Controller {
        self.get(&ControllerId::SystemFans)
    }

    pub fn radiator_fans(&self) -> &Controller {
        self.get(&ControllerId::RadiatorFans)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Setup OpenRGB client
    let client = OpenRGB::connect().await?;
    client.set_name("rgb").await?;

    let controllers = Controllers::new(&client).await?;

    println!("RAM Left: {}", controllers.ram_left().name);
    println!("RAM Right: {}", controllers.ram_right().name);
    println!("GPU: {}", controllers.gpu().name);
    println!("System Fans: {}", controllers.system_fans().name);
    println!("Radiator Fans: {}", controllers.radiator_fans().name);

    // Main loop
    let running = Arc::new(AtomicBool::new(true));
    {
        let running = running.clone();
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })
        .expect("Failed to set Ctrl-C handler");
    }

    let gradient_a = Gradient::new(vec![
        (0.0, Color::new(0.2, 0.2, 0.2)),
        (1.0, Color::new(1.0, 0.0, 0.0)),
        (2.0, Color::new(0.0, 1.0, 0.0)),
        (3.0, Color::new(1.0, 1.0, 1.0)),
        (4.0, Color::new(0.2, 0.2, 0.2)),
    ]);

    let gradient_b = Gradient::new(vec![
        (0.0, Color::new(0.2, 0.2, 0.2)),
        (1.0, Color::new(1.0, 1.0, 0.0)),
        (2.0, Color::new(1.0, 0.0, 1.0)),
        (3.0, Color::new(0.0, 1.0, 1.0)),
        (4.0, Color::new(0.2, 0.2, 0.2)),
    ]);

    let left_ram_samplers = linear(&gradient_a, 12);
    let right_ram_samplers = linear(&gradient_b, 12);
    let gpu_sampler = offset(&gradient_a, 0.0);
    let system_fan_samplers = linear(&gradient_a, 33 + (34 * 6));
    let cpu_fan_samplers = linear(&gradient_b, 58 + 34);

    let start_ts = Instant::now();

    while running.load(Ordering::SeqCst) {
        let total_time = Instant::now().duration_since(start_ts).as_secs_f32();
        println!("Total time: {total_time:}");

        for (i, sampler) in left_ram_samplers.iter().enumerate() {
            let color = OpenRGBColor::from(sampler(total_time));
            println!("Left RAM Color {i:}: {color:?}");
        }

        for (i, sampler) in right_ram_samplers.iter().enumerate() {
            let color = OpenRGBColor::from(sampler(total_time));
            println!("Right RAM Color {i:}: {color:?}");
        }

        let color = OpenRGBColor::from(gpu_sampler(total_time));
        println!("GPU Color: {color:?}");

        for (i, sampler) in system_fan_samplers.iter().enumerate() {
            let color = OpenRGBColor::from(sampler(total_time));
            println!("System Fan Color {i:}: {color:?}");
        }

        for (i, sampler) in cpu_fan_samplers.iter().enumerate() {
            let color = OpenRGBColor::from(sampler(total_time));
            println!("CPU Fan Color{i:}: {color:?}");
        }

        std::thread::sleep(SLEEP_DURATION);
    }

    Ok(())
}

// LED layout
fn linear(gradient: &Gradient, count: usize) -> Vec<impl Sample + '_> {
    let mut i = 0.0;
    std::iter::from_fn(|| {
        let f: Box<dyn Sample> = Box::new(offset(&gradient, i));
        i += 1.0 / gradient.max_t();
        Some(f)
    })
    .take(count)
    .collect()
}

// Sampler combinators
fn unit(gradient: &Gradient) -> impl Sample + '_ {
    move |time| gradient.sample(time)
}

fn offset(gradient: &Gradient, offset: f32) -> impl Sample + '_ {
    move |time| gradient.sample(time + offset)
}

fn add(lhs: impl Sample, rhs: impl Sample) -> impl Sample {
    move |time| lhs(time) + rhs(time)
}

fn sub(lhs: impl Sample, rhs: impl Sample) -> impl Sample {
    move |time| lhs(time) - rhs(time)
}

fn mul(lhs: impl Sample, rhs: impl Sample) -> impl Sample {
    move |time| lhs(time) * rhs(time)
}

fn div(lhs: impl Sample, rhs: impl Sample) -> impl Sample {
    move |time| lhs(time) / rhs(time)
}

fn min(lhs: impl Sample, rhs: impl Sample) -> impl Sample {
    move |time| lhs(time).min(rhs(time))
}

fn max(lhs: impl Sample, rhs: impl Sample) -> impl Sample {
    move |time| lhs(time).max(rhs(time))
}

/// Trait for sampling a color from a gradient
trait Sample: Fn(f32) -> Color {}
impl<T> Sample for T where T: Fn(f32) -> Color {}

/// A multi-point color gradient
#[derive(Debug, Default, Clone)]
struct Gradient(Vec<(f32, Color)>);

impl Gradient {
    pub fn new(mut colors: Vec<(f32, Color)>) -> Self {
        colors.sort_unstable_by(|(lhs, _), (rhs, _)| lhs.partial_cmp(rhs).unwrap());
        Gradient(colors)
    }

    pub fn max_t(&self) -> f32 {
        self.0.last().unwrap().0
    }

    pub fn sample(&self, f: f32) -> Color {
        let f = f % self.max_t();

        let to_idx = if let Some(to_idx) = self.0.iter().position(|(x, _)| f <= *x) {
            to_idx
        } else {
            return self
                .0
                .get(0)
                .expect("Can't sample a gradient with no colors")
                .1;
        };

        let from_idx = if to_idx > 0 {
            to_idx - 1
        } else {
            return self.0[to_idx].1;
        };

        let (from_x, from_color) = self.0[from_idx];
        let (to_x, to_color) = self.0[to_idx];

        let width = to_x - from_x;
        let fac = (f - from_x) / width;

        from_color.lerp(to_color, fac)
    }
}

/// An RGB f32 color
#[derive(Debug, Default, Copy, Clone, PartialEq, PartialOrd)]
struct Color {
    r: f32,
    g: f32,
    b: f32,
}

impl From<Color> for OpenRGBColor {
    fn from(c: Color) -> OpenRGBColor {
        OpenRGBColor {
            r: (c.r * 255.0) as u8,
            g: (c.g * 255.0) as u8,
            b: (c.b * 255.0) as u8,
        }
    }
}

impl Add for Color {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Color {
            r: self.r + rhs.r,
            g: self.g + rhs.g,
            b: self.b + rhs.b,
        }
    }
}

impl Sub for Color {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Color {
            r: self.r - rhs.r,
            g: self.g - rhs.g,
            b: self.b - rhs.b,
        }
    }
}

impl Mul for Color {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Color {
            r: self.r * rhs.r,
            g: self.g * rhs.g,
            b: self.b * rhs.b,
        }
    }
}

impl Div for Color {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        Color {
            r: self.r / rhs.r,
            g: self.g / rhs.g,
            b: self.b / rhs.b,
        }
    }
}

impl Add<f32> for Color {
    type Output = Self;

    fn add(self, rhs: f32) -> Self::Output {
        Color {
            r: self.r + rhs,
            g: self.g + rhs,
            b: self.b + rhs,
        }
    }
}

impl Sub<f32> for Color {
    type Output = Self;

    fn sub(self, rhs: f32) -> Self::Output {
        Color {
            r: self.r - rhs,
            g: self.g - rhs,
            b: self.b - rhs,
        }
    }
}

impl Mul<f32> for Color {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Color {
            r: self.r * rhs,
            g: self.g * rhs,
            b: self.b * rhs,
        }
    }
}

impl Div<f32> for Color {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Color {
            r: self.r / rhs,
            g: self.g / rhs,
            b: self.b / rhs,
        }
    }
}

impl Color {
    pub fn new(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b }
    }

    pub fn lerp(self, with: Self, fac: f32) -> Self {
        self + (with - self) * fac
    }

    pub fn min(self, rhs: Color) -> Self {
        Color {
            r: self.r.min(rhs.r),
            g: self.g.min(rhs.g),
            b: self.b.min(rhs.b),
        }
    }

    pub fn max(self, rhs: Color) -> Self {
        Color {
            r: self.r.max(rhs.r),
            g: self.g.max(rhs.g),
            b: self.b.max(rhs.b),
        }
    }
}
