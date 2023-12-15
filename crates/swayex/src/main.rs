use std::time::Duration;

use anyhow::Result;
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::Deserializer;

type NodeId = usize;
type WindowId = usize;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum NodeLayout {
    #[serde(rename(deserialize = "splith"))]
    SplitH,
    #[serde(rename(deserialize = "splitv"))]
    SplitV,
    #[serde(rename(deserialize = "stacked"))]
    Stacked,
    #[serde(rename(deserialize = "tabbed"))]
    Tabbed,
    #[serde(rename(deserialize = "output"))]
    Output,
    #[serde(rename(deserialize = "none"))]
    None,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum NodeType {
    #[serde(rename(deserialize = "root"))]
    Root,
    #[serde(rename(deserialize = "output"))]
    Output,
    #[serde(rename(deserialize = "workspace"))]
    Workspace,
    #[serde(rename(deserialize = "con"))]
    Con,
    #[serde(rename(deserialize = "floating_con"))]
    FloatingCon,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum NodeOrientation {
    #[serde(rename(deserialize = "vertical"))]
    Vertical,
    #[serde(rename(deserialize = "horizontal"))]
    Horizontal,
    #[serde(rename(deserialize = "none"))]
    None,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Border {
    #[serde(rename(deserialize = "normal"))]
    Normal,
    #[serde(rename(deserialize = "none"))]
    None,
    #[serde(rename(deserialize = "pixel"))]
    Pixel,
    #[serde(rename(deserialize = "csd"))]
    CSD,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum WindowChange {
    #[serde(rename(deserialize = "new"))]
    New,
    #[serde(rename(deserialize = "close"))]
    Close,
    #[serde(rename(deserialize = "focus"))]
    Focus,
    #[serde(rename(deserialize = "title"))]
    Title,
    #[serde(rename(deserialize = "fullscreen_mode"))]
    FullscreenMode,
    #[serde(rename(deserialize = "move"))]
    Move,
    #[serde(rename(deserialize = "floating"))]
    Floating,
    #[serde(rename(deserialize = "urgent"))]
    Urgent,
    #[serde(rename(deserialize = "mark"))]
    Mark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IdleInhibitors {
    user: String,
    application: String,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct Rect {
    x: isize,
    y: isize,
    width: usize,
    height: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Container {
    id: usize,
    name: Option<String>,
    r#type: NodeType,
    border: Border,
    current_border_width: usize,
    layout: NodeLayout,
    orientation: NodeOrientation,
    percent: Option<f32>,
    rect: Rect,
    window_rect: Rect,
    deco_rect: Rect,
    geometry: Rect,
    urgent: bool,
    sticky: bool,
    marks: Vec<()>,
    focused: bool,
    focus: Vec<NodeId>,
    nodes: Vec<NodeId>,
    floating_nodes: Vec<NodeId>,
    fullscreen_mode: usize,
    pid: usize,
    app_id: Option<String>,
    visible: bool,
    shell: String,
    inhibit_idle: bool,
    idle_inhibitors: IdleInhibitors,
    window: Option<WindowId>,
    max_render_time: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WindowEvent {
    change: WindowChange,
    container: Container,
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
    info!("swayex starting");

    let stdin = std::io::stdin();
    for event in Deserializer::from_reader(stdin).into_iter::<WindowEvent>() {
        let event = event?;
        if event.container.name.as_deref() == Some("MW5Mercs  ") {
            match event.change {
                WindowChange::New | WindowChange::FullscreenMode => {
                    info!("Setting MechWarrior 5 to triple ultrawide");
                    std::thread::sleep(Duration::from_millis(200));
                    std::process::Command::new("swaymsg")
                        .arg("fullscreen disable;")
                        .arg("floating enable;")
                        .arg("resize set width 7168;")
                        .arg("resize set height 1440;")
                        .arg("move absolute position 0 0")
                        .spawn()?;
                }
                _ => (),
            }
        }
    }
    info!("swayex exiting");

    Ok(())
}
