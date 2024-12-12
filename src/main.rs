use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use swayipc::{Connection, Event, EventStream, EventType, Node, NodeLayout, WindowChange};

mod matcher;
mod node;
mod placeholder;
mod visit;

use matcher::Matchers;
use node::*;
use placeholder::ClientHandle;
use visit::{LayoutLiteVisitor, LayoutVisitor};

struct Cmd {
    conn: Connection,
}
impl Cmd {
    fn new() -> Result<Cmd> {
        Ok(Self {
            conn: Connection::new()?,
        })
    }
    fn run(&mut self, cmd: &str) -> Result<()> {
        log::debug!("cmd: '{}'", cmd);
        for res in self.conn.run_command(cmd)? {
            res?;
        }
        Ok(())
    }
}

struct Events {
    inner: EventStream,
}
impl Events {
    fn new() -> Result<Events> {
        Ok(Events {
            inner: Connection::new()?.subscribe(&[EventType::Window])?,
        })
    }
    fn wait_new_window(&mut self, app_id: &str) -> Result<Node> {
        log::debug!("wait for window:");
        while let Some(event) = self.inner.next() {
            match event? {
                Event::Window(w) => match w.change {
                    WindowChange::New => {
                        if w.container.app_id.as_deref() == Some(app_id) {
                            log::debug!(
                                "new window id={} app_id={:?}",
                                w.container.id,
                                w.container.app_id
                            );
                            return Ok(w.container);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        anyhow::bail!("Event stream ended");
    }

    fn wait_window_focus(&mut self, id: i64) -> Result<Node> {
        while let Some(event) = self.inner.next() {
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
}

fn populate_swallows(n: &mut NodeLite) {
    if n.nodes.is_empty() {
        let wp = &n.window_properties;
        let m = Matcher {
            name: None,
            app_id: n.app_id.clone(),
            class: wp.as_ref().and_then(|w| w.class.clone()),
            instance: wp.as_ref().and_then(|w| w.instance.clone()),
        };
        n.swallows.push(m);
    } else {
        for c in &mut n.nodes {
            populate_swallows(c);
        }
    }
}

fn get_tree_lite(conn: &mut Connection) -> Result<NodeLite> {
    let tree = conn.get_tree()?;
    let json = serde_json::to_value(tree)?;
    let mut tree_lite: NodeLite = serde_json::from_value(json)?;
    tree_lite.nodes.remove(0);
    Ok(tree_lite)
}

struct WorkspaceFinder {
    workspaces: Vec<String>,
}
impl WorkspaceFinder {
    fn new() -> Self {
        Self { workspaces: vec![] }
    }
    fn get(self) -> Vec<String> {
        self.workspaces
    }
}
impl LayoutVisitor for WorkspaceFinder {
    fn on_workspace(&mut self, workspace: &Node) -> Result<()> {
        self.workspaces.push(
            workspace
                .name
                .clone()
                .ok_or_else(|| anyhow::anyhow!("workspace with no name"))?,
        );
        Ok(())
    }
}

struct WorkspaceDetacher<'a> {
    cmd: &'a mut Cmd,
    workspaces: Vec<String>,
    views: Vec<Node>,
}
impl<'a> WorkspaceDetacher<'a> {
    fn new(cmd: &'a mut Cmd, workspaces: Vec<String>) -> Self {
        Self {
            cmd,
            workspaces,
            views: vec![],
        }
    }
    fn get(self) -> Vec<Node> {
        self.views
    }
}
impl<'a> LayoutVisitor for WorkspaceDetacher<'a> {
    fn on_workspace(&mut self, workspace: &Node) -> Result<()> {
        for w in &self.workspaces {
            if Some(w) == workspace.name.as_ref() {
                return Ok(());
            }
        }
        for c in &workspace.nodes {
            self.cmd
                .run(&format!("[con_id={}] floating enable", c.id))?;
        }
        Ok(())
    }
    fn on_view(&mut self, view: &Node) -> Result<()> {
        self.views.push(view.clone());
        Ok(())
    }
}

struct LayoutBuilder<'a> {
    cmd: &'a mut Cmd,
    events: &'a mut Events,
    placeholder: placeholder::ClientHandle,
    matchers: Matchers,
}

impl<'a> LayoutBuilder<'a> {
    fn new(cmd: &'a mut Cmd, events: &'a mut Events) -> LayoutBuilder<'a> {
        LayoutBuilder {
            cmd,
            events,
            placeholder: ClientHandle::new(),
            matchers: Matchers::new(),
        }
    }
    fn get(self) -> (placeholder::ClientHandle, Matchers) {
        (self.placeholder, self.matchers)
    }
}
impl<'a> LayoutLiteVisitor for LayoutBuilder<'a> {
    fn on_output(&mut self, output: &NodeLite) -> Result<()> {
        self.cmd.run(&format!(
            "focus output {}",
            output
                .name
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("output with no name"))?
        ))?;
        Ok(())
    }
    fn on_workspace(&mut self, workspace: &NodeLite) -> Result<()> {
        self.cmd.run(&format!(
            "workspace {}",
            workspace
                .name
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("workspace with no name"))?
        ))?;
        Ok(())
    }
    fn on_container_enter(&mut self, con: &NodeLite) -> Result<()> {
        self.cmd.run("splith")?;
        let layout = match con.layout {
            NodeLayout::SplitH => "splith",
            NodeLayout::SplitV => "splitv",
            NodeLayout::Tabbed => "tabbed",
            NodeLayout::Stacked => "stacked",
            _ => anyhow::bail!("usupported layout"),
        };
        self.cmd.run(&format!("layout {}", layout))?;
        Ok(())
    }
    fn on_container_exit(&mut self, con: &NodeLite) -> Result<()> {
        let node = self
            .cmd
            .conn
            .get_tree()
            .unwrap()
            .find_focused(|n| n.nodes.iter().any(|c| c.focused))
            .ok_or_else(|| anyhow::anyhow!("no focused window"))?;
        let (dim, tot_size) = match con.layout {
            NodeLayout::SplitV => ("height", node.rect.height),
            NodeLayout::SplitH => ("width", node.rect.width),
            NodeLayout::Tabbed | NodeLayout::Stacked => {
                self.cmd.run("focus parent")?;
                return Ok(());
            }
            _ => anyhow::bail!("usupported layout"),
        };

        for c in con.nodes.iter().rev() {
            let perc = c
                .percent
                .ok_or_else(|| anyhow::anyhow!("missing percent field"))?;
            let size = (perc * (tot_size as f64)).floor() as i32;
            self.cmd.run(&format!("resize set {} {} px", dim, size))?;
            self.cmd.run("focus prev sibling")?;
        }
        self.cmd.run("focus parent")?;
        Ok(())
    }
    fn on_view(&mut self, view: &NodeLite) -> Result<()> {
        self.placeholder
            .new_window(view.name.as_deref().unwrap_or("swaystart"), "swaystart");
        let node = self.events.wait_new_window("swaystart")?;
        self.events.wait_window_focus(node.id)?;
        self.matchers.add(node.id, view.swallows.clone());
        Ok(())
    }
}

struct Swapper<'a> {
    cmd: &'a mut Cmd,
    events: &'a mut Events,
    matchers: Matchers,
}

impl<'a> Swapper<'a> {
    fn new(cmd: &'a mut Cmd, events: &'a mut Events, matchers: Matchers) -> Self {
        Swapper {
            cmd,
            events,
            matchers,
        }
    }
    fn do_swap(&mut self, id1: i64, id2: i64) -> Result<()> {
        self.cmd
            .run(&format!("[con_id={id1}] swap container with con_id {id2}"))?;
        self.cmd.run(&format!("[con_id={id1}] kill"))?;
        Ok(())
    }
    fn swap(&mut self, prev: &[Node]) -> Result<()> {
        for p in prev {
            if let Some(id) = self.matchers.consume(&p) {
                self.do_swap(id, p.id)?;
            }
        }
        while let Some(event) = self.events.inner.next() {
            log::debug!("{:?}", event);
            match event? {
                Event::Window(w) => match w.change {
                    WindowChange::Close => {
                        if Some("swaystart") == w.container.app_id.as_deref() {
                            self.matchers.remove(w.container.id);
                            if self.matchers.is_empty() {
                                break;
                            }
                        }
                    }
                    WindowChange::New => {
                        if let Some(id) = self.matchers.consume(&w.container) {
                            self.do_swap(id, w.container.id)?;
                        }
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
    #[arg(short, long)]
    layout_file: PathBuf,
    #[arg(short, long, default_value = "false")]
    save: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut log_builder = pretty_env_logger::formatted_builder();
    if args.debug {
        log_builder.filter_level(log::LevelFilter::Debug);
    }
    log_builder.init();

    if args.save {
        let mut conn = Connection::new()?;
        let mut tree = get_tree_lite(&mut conn)?;
        populate_swallows(&mut tree);
        let s = serde_json::to_string_pretty(&tree)?;
        std::fs::write(args.layout_file, s)?;
        return Ok(());
    }

    let conf = std::fs::read_to_string(args.layout_file)?;
    let conf_tree: NodeLite = serde_json::from_str(&conf)?;

    let mut cmd = Cmd::new()?;
    let mut events = Events::new()?;

    let tree = cmd.conn.get_tree()?;
    let mut finder = WorkspaceFinder::new();
    finder.visit_node(&tree)?;

    let mut detacher = WorkspaceDetacher::new(&mut cmd, finder.get());
    detacher.visit_node(&tree)?;
    let detached = detacher.get();

    let mut builder = LayoutBuilder::new(&mut cmd, &mut events);
    builder.visit_node(&conf_tree)?;

    let (placeholder, matchers) = builder.get();

    let mut swapper = Swapper::new(&mut cmd, &mut events, matchers);
    swapper.swap(&detached)?;

    placeholder.wait_until_idle();

    Ok(())
}
