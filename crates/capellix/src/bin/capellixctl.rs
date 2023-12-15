use capellix::{
    then::Then,
    thread::{capellixctl::CapellixCtl, exit_result, print_thread_result},
};
use clap::StructOpt;

fn main() -> ! {
    env_logger::init();
    CapellixCtl::parse()
        .run()
        .then(print_thread_result("Main"))
        .then(exit_result)
}
