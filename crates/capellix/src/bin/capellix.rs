use capellix::{
    then::Then,
    thread::{capellix::Capellix, exit_result, print_thread_result},
};
use clap::StructOpt;

fn main() -> ! {
    env_logger::init();
    Capellix::parse()
        .run()
        .then(print_thread_result("Main"))
        .then(exit_result)
}
