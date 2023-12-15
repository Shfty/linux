mod app;
mod fan;
mod hwmon;
mod hwmon_entry;
mod pwm;
mod temp;
mod then;

pub use app::*;
pub use fan::*;
pub use hwmon::*;
pub use hwmon_entry::*;
pub use pwm::*;
pub use temp::*;
pub use then::*;

use clap::Parser;

fn main() {
    env_logger::init();

    std::process::exit(match Fans::parse().run() {
        Ok(_) => 0,
        Err(e) => {
            log::error!("{e:}");
            1
        }
    })
}
