pub mod capellix;
pub mod capellixctl;
pub mod pump_target;
pub mod server_thread;
pub mod socket;

use anyhow::Result;
use log::{error, info};

pub fn print_thread_result<T>(thread_name: &str) -> impl FnOnce(Result<T>) -> Result<T> + '_ {
    move |result| {
        match &result {
            Ok(_) => info!("{thread_name:} finalizing"),
            Err(e) => error!("{thread_name:} error: {e:}"),
        }
        result
    }
}

/// Exit with code 0 if the provided result is an Ok(_) variant, or with code 1 if it's an Err(_)
pub fn exit_result<T>(result: Result<T>) -> ! {
    std::process::exit(match result {
        Ok(_) => 0,
        Err(_) => 1,
    })
}
