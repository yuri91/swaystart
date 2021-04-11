use anyhow::Result;
use gio::prelude::*;
use serde::Deserialize;
use std::path::PathBuf;
use structopt::StructOpt;
use swayipc::{Connection, Event, EventStream, EventType, Node, WindowChange};

fn wait_new_window(events: &mut EventStream) -> Result<Node> {
    log::debug!("wait for window:");
    while let Some(event) = events.next() {
        match event? {
            Event::Window(w) => match w.change {
                WindowChange::New => {
                    log::debug!("new window id={} app_id={:?}", w.container.id, w.container.app_id);
                    return Ok(w.container);
                }
                _ => {}
            },
            _ => {}
        }
    }
    anyhow::bail!("Event stream ended");
}

fn wait_window_focus(events: &mut EventStream, id: i64) -> Result<Node> {
    while let Some(event) = events.next() {
        match event? {
            Event::Window(w) => {
                if w.container.id != id {
                    continue;
                }
                match w.change {
                    WindowChange::Focus => {
                        log::debug!("focus window id={} app_id={:?}", w.container.id, w.container.app_id);
                        return Ok(w.container);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    anyhow::bail!("Event stream ended");
}

fn spawn(app: &str) -> Result<()> {
    log::debug!("spawn: '{}'", app);
    let app = gio::DesktopAppInfo::new(app).ok_or_else(|| anyhow::anyhow!("no app"))?;
    app.launch_uris(&[], Some(&gio::AppLaunchContext::new()))?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Output {
    name: String,
    workspaces: Vec<Workspace>,
}
#[derive(Debug, Deserialize)]
struct Workspace {
    name: String,
    style: LayoutStyle,
    layout: Layout,
}
#[derive(Debug, Deserialize)]
struct Layout {
    style: LayoutStyle,
    slots: Vec<Slot>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LayoutStyle {
    Tabbed,
    Splitv,
    Splith,
}
impl std::fmt::Display for LayoutStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let style = match self {
            LayoutStyle::Tabbed => "tabbed",
            LayoutStyle::Splitv => "splitv",
            LayoutStyle::Splith => "splith",
        };
        write!(f, "{}", style)
    }
}

const fn f64_one() -> f64 {
    1.
}
#[derive(Debug, Deserialize)]
struct Slot {
    #[serde(default = "f64_one")]
    size: f64,
    content: SlotContent,
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SlotContent {
    Container(Layout),
    App(String),
}

trait LayoutVisitor {
    fn visit_output(&mut self, output: &Output) -> Result<()> {
        self.on_output(output)?;
        for w in &output.workspaces {
            self.visit_workspace(w)?;
        }
        Ok(())
    }
    fn visit_workspace(&mut self, workspace: &Workspace) -> Result<()> {
        self.on_workspace(workspace)?;
        self.visit_layout(&workspace.layout)?;
        Ok(())
    }
    fn visit_layout(&mut self, layout: &Layout) -> Result<()> {
        if let [first, rest @ ..] = layout.slots.as_slice() {
            self.visit_slot(first)?;
            self.on_layout_enter(layout)?;
            for s in rest {
                self.visit_slot(s)?;
            }
            self.on_layout_exit(layout)?;
        }
        Ok(())
    }
    fn visit_slot(&mut self, slot: &Slot) -> Result<()> {
        self.on_slot(slot)?;
        match slot.content {
            SlotContent::Container(ref c) => {
                self.visit_layout(c)?;
            }
            SlotContent::App(ref a) => {
                self.visit_app(a)?;
            }
        }
        Ok(())
    }
    fn visit_app(&mut self, app: &str) -> Result<()> {
        self.on_app(app)?;
        Ok(())
    }
    fn on_slot(&mut self, _slot: &Slot) -> Result<()> {
        Ok(())
    }
    fn on_app(&mut self, _app: &str) -> Result<()> {
        Ok(())
    }
    fn on_layout_enter(&mut self, _layout: &Layout) -> Result<()> {
        Ok(())
    }
    fn on_layout_exit(&mut self, _layout: &Layout) -> Result<()> {
        Ok(())
    }
    fn on_workspace(&mut self, _workspace: &Workspace) -> Result<()> {
        Ok(())
    }
    fn on_output(&mut self, _output: &Output) -> Result<()> {
        Ok(())
    }
}

struct LayoutBuilder {
    conn: Connection,
    events: EventStream,
}

impl LayoutBuilder {
    fn new() -> Result<LayoutBuilder> {
        let subs = [
            EventType::Window,
            EventType::Workspace,
            EventType::Mode,
            EventType::BarConfigUpdate,
            EventType::Binding,
            EventType::Shutdown,
            EventType::Tick,
            EventType::BarStateUpdate,
            EventType::Input,
        ];
        let builder = LayoutBuilder {
            conn: Connection::new()?,
            events: Connection::new()?.subscribe(&subs)?,
        };
        Ok(builder)
    }
    fn run(&mut self, cmd: &str) -> Result<()> {
        log::debug!("cmd: '{}'", cmd);
        for res in self.conn.run_command(cmd)? {
            res?;
        }
        Ok(())
    }
}
impl LayoutVisitor for LayoutBuilder {
    fn on_output(&mut self, output: &Output) -> Result<()> {
        self.run(&format!("focus output {}", output.name))?;
        Ok(())
    }
    fn on_workspace(&mut self, workspace: &Workspace) -> Result<()> {
        self.run(&format!(
            "workspace {}; layout {}",
            workspace.name, workspace.style
        ))?;
        Ok(())
    }
    fn on_layout_enter(&mut self, layout: &Layout) -> Result<()> {
        self.run("splith")?;
        self.run(&format!("layout {}", layout.style))?;
        Ok(())
    }
    fn on_layout_exit(&mut self, layout: &Layout) -> Result<()> {
        let dim = match layout.style {
            LayoutStyle::Splitv => "height",
            LayoutStyle::Splith => "width",
            LayoutStyle::Tabbed => {
                self.run("focus parent")?;
                return Ok(());
            },
        };
        let denom = layout.slots.iter().fold(0., |acc, el| acc + el.size);
        let mut nodes = Vec::new();
        for s in layout.slots.iter().rev() {
            let node = self
                .conn
                .get_tree()
                .unwrap()
                .find_focused(|n| n.focused)
                .ok_or_else(|| anyhow::anyhow!("no focused window"))?;
            let size_px = match layout.style {
                LayoutStyle::Splitv => node.rect.height,
                LayoutStyle::Splith => node.rect.width,
                LayoutStyle::Tabbed => unreachable!(),
            } as f64;
            nodes.push((node.id, size_px, s.size));
            self.run("focus prev sibling")?;
        }
        let tot_size_px = nodes.iter().fold(0., |acc, el| acc + el.1);
        for n in nodes.iter().rev() {
            let size_px = ((n.2 / denom) * tot_size_px) as i32;
            self.run(&format!(
                "[con_id={}] focus; resize set {} {} px",
                n.0, dim, size_px
            ))?;
        }
        self.run("focus parent")?;
        Ok(())
    }
    fn on_app(&mut self, app: &str) -> Result<()> {
        spawn(&format!("{}.desktop", app))?;
        let node = wait_new_window(&mut self.events)?;
        wait_window_focus(&mut self.events, node.id)?;
        Ok(())
    }
}

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(short, long)]
    debug: bool,
    #[structopt(parse(from_os_str))]
    layout_file: PathBuf,
}

#[paw::main]
fn main(opt: Opt) -> Result<()> {
    let mut log_builder = pretty_env_logger::formatted_builder();
    if opt.debug {
        log_builder.filter_level(log::LevelFilter::Debug);
    }
    log_builder.init();

    let conf = std::fs::read_to_string(opt.layout_file)?;
    let output: Output = serde_json::from_str(&conf)?;

    let mut builder = LayoutBuilder::new()?;
    builder.visit_output(&output)?;

    Ok(())
}
