use std::str::FromStr;
use workflow::Workflows;

#[derive(Debug, Copy, Clone)]
enum Command {
    Build,
    Test,
    Run,
    Deploy,
    Debug,
}

impl FromStr for Command {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "build" => Ok(Command::Build),
            "test" => Ok(Command::Test),
            "run" => Ok(Command::Run),
            "deploy" => Ok(Command::Deploy),
            "debug" => Ok(Command::Debug),
            _ => Err(()),
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next() {
        Some(arg) => match Command::from_str(&arg) {
            Ok(command) => run(command, args.collect()),
            Err(_) => {
                println!("Invalid argument");
                std::process::exit(1);
            }
        },
        None => println!("Usage: workflow-run <command>"),
    }
}

fn run(command: Command, args: String) {
    let workflows = Workflows::new("/home/josh/.config/workflow").unwrap();

    let workspace_path = std::fs::read_to_string("/home/josh/.local/state/workspace")
        .unwrap()
        .replace("\n", "");

    let workflow = if let Some(workflow) = workflows.workflow(&workspace_path) {
        workflow
    } else {
        println!("No valid workflow");
        std::process::exit(1);
    };

    let commands = workflow
        .commands
        .as_ref()
        .expect("Workflow has no commands");

    let command = if let Some(command) = match command {
        Command::Build => &commands.build,
        Command::Test => &commands.test,
        Command::Run => &commands.run,
        Command::Deploy => &commands.deploy,
        Command::Debug => &commands.debug,
    } {
        command
    } else {
        println!("Workflow has no such command");
        std::process::exit(1);
    };

    let mut commands: Vec<String> = command.split("\n").map(ToOwned::to_owned).collect();
    commands
        .last_mut()
        .map(|command| command.extend(std::iter::once(' ').chain(args.chars())));

    for command in commands {
        let (command, args) = command.split_once(" ").unwrap();
        let args = args.split_whitespace().collect::<Vec<_>>();

        std::process::Command::new(command)
            .args(args)
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
    }

    println!("\nPress any key to continue...");
    std::io::stdin().read_line(&mut String::new()).unwrap();
}
