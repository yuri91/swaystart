use anyhow::Result;
use clap::Parser;
use gio::prelude::*;
use placeholder::ClientHandle;
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};
use swayipc::{Connection, Event, EventStream, EventType, Node, WindowChange};

mod placeholder;

fn wait_new_window(events: &mut EventStream, app_id: &str) -> Result<Node> {
    log::debug!("wait for window:");
    while let Some(event) = events.next() {
        match event? {
            Event::Window(w) => match w.change {
                WindowChange::New if w.container.app_id.as_deref() == Some(app_id) => {
                    log::debug!(
                        "new window id={} app_id={:?}",
                        w.container.id,
                        w.container.app_id
                    );
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
                        log::debug!(
                            "focus window id={} app_id={:?}",
                            w.container.id,
                            w.container.app_id
                        );
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
    let app = gio::DesktopAppInfo::new(app).ok_or_else(|| anyhow::anyhow!("no app: {app}"))?;
    let ctx = gio::AppLaunchContext::new();
    log::debug!("env: {:?}", ctx.environment());
    app.launch_uris(&[], Some(&ctx))?;
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
    AppWithId { app: String, id: String },
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
                self.visit_app(a, a)?;
            }
            SlotContent::AppWithId { ref app, ref id } => {
                self.visit_app(&app, &id)?;
            }
        }
        Ok(())
    }
    fn visit_app(&mut self, app: &str, id: &str) -> Result<()> {
        self.on_app(app, id)?;
        Ok(())
    }
    fn on_slot(&mut self, _slot: &Slot) -> Result<()> {
        Ok(())
    }
    fn on_app(&mut self, _app: &str, _id: &str) -> Result<()> {
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
    placeholder: placeholder::ClientHandle,
    mapping: HashMap<String, Vec<i64>>,
}

impl LayoutBuilder {
    fn new() -> Result<LayoutBuilder> {
        let builder = LayoutBuilder {
            conn: Connection::new()?,
            events: Connection::new()?.subscribe(&[EventType::Window])?,
            placeholder: ClientHandle::new(),
            mapping: HashMap::new(),
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
            }
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
    fn on_app(&mut self, app: &str, id: &str) -> Result<()> {
        let app_info = gio::DesktopAppInfo::new(&format!("{app}.desktop"))
            .ok_or_else(|| anyhow::anyhow!("no app: {}", app))?;
        let placeholder_app_id = format!("swaystart-{}", id);
        self.placeholder
            .new_window(app_info.display_name().as_str(), &placeholder_app_id);
        let node = wait_new_window(&mut self.events, &placeholder_app_id)?;
        self.mapping.entry(id.to_owned()).or_default().push(node.id);
        wait_window_focus(&mut self.events, node.id)?;
        Ok(())
    }
}

struct Spawner {}
impl LayoutVisitor for Spawner {
    fn on_app(&mut self, app: &str, _id: &str) -> Result<()> {
        spawn(&format!("{}.desktop", app))?;
        Ok(())
    }
}

struct Swapper {
    conn: Connection,
    events: EventStream,
    mapping: HashMap<String, Vec<i64>>,
}

impl Swapper {
    fn new(mapping: HashMap<String, Vec<i64>>) -> Result<Self> {
        let swapper = Swapper {
            conn: Connection::new()?,
            events: Connection::new()?.subscribe(&[EventType::Window])?,
            mapping,
        };
        Ok(swapper)
    }
    fn run(&mut self, cmd: &str) -> Result<()> {
        log::debug!("cmd: '{}'", cmd);
        for res in self.conn.run_command(cmd)? {
            res?;
        }
        Ok(())
    }
    fn swap(&mut self) -> Result<()> {
        let mut count = 0;
        for v in self.mapping.values() {
            count += v.len();
        }
        while let Some(event) = self.events.next() {
            log::debug!("{:?}", event);
            match event? {
                Event::Window(w) => match w.change {
                    WindowChange::Close => {
                        if let Some(app_id) = w.container.app_id.as_deref() {
                            if let Some(id) = app_id.strip_prefix("swaystart-") {
                                if let Some(v) = self.mapping.get_mut(id) {
                                    let idx = v.iter().position(|i| *i == w.container.id);
                                    if let Some(idx) = idx {
                                        v.swap_remove(idx);
                                        count -= 1;
                                        if count == 0 {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    WindowChange::New => {
                        println!("{:?}", w);
                        let matcher = if w.container.floating.is_some() {
                            None
                        } else if w
                            .container
                            .window_properties
                            .as_ref()
                            .is_some_and(|p| p.window_type.as_deref() != Some("normal"))
                        {
                            None
                        } else if let Some(props) = w.container.window_properties.as_ref() {
                            props.class.as_deref()
                        } else {
                            w.container.app_id.as_deref()
                        };
                        if let Some(m) = matcher {
                            if let Some(v) = self.mapping.get_mut(m) {
                                if let Some(con_id) = v.pop() {
                                    self.run(&format!(
                                        "[con_id={con_id}] swap container with con_id {}",
                                        w.container.id
                                    ))?;
                                    self.run(&format!("[con_id={con_id}] kill"))?;
                                    count -= 1;
                                    if count == 0 {
                                        break;
                                    }
                                    continue;
                                }
                            }
                        }
                        self.run(&format!("[con_id={}] floating enable", w.container.id))?;
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        Ok(())
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "false")]
    debug: bool,
    #[arg(short, long, default_value = "false")]
    spawn: bool,
    #[arg(short, long)]
    layout_file: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut log_builder = pretty_env_logger::formatted_builder();
    if args.debug {
        log_builder.filter_level(log::LevelFilter::Debug);
    }
    log_builder.init();

    let conf = std::fs::read_to_string(args.layout_file)?;
    let output: Output = serde_json::from_str(&conf)?;

    if let Some(home) = dirs::home_dir() {
        std::env::set_current_dir(home)?;
    }

    let mut builder = LayoutBuilder::new()?;
    builder.visit_output(&output)?;

    let LayoutBuilder {
        placeholder,
        mapping,
        ..
    } = builder;

    if args.spawn {
        let mut spawner = Spawner {};
        spawner.visit_output(&output)?;
    }
    let mut swapper = Swapper::new(mapping)?;
    swapper.swap()?;

    placeholder.wait_until_idle();

    Ok(())
}
