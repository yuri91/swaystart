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
use visit::LayoutVisitor;

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

struct LayoutBuilder {
    conn: Connection,
    events: EventStream,
    placeholder: placeholder::ClientHandle,
    matchers: Matchers,
}

impl LayoutBuilder {
    fn new() -> Result<LayoutBuilder> {
        let conn = Connection::new()?;
        let builder = LayoutBuilder {
            conn,
            events: Connection::new()?.subscribe(&[EventType::Window])?,
            placeholder: ClientHandle::new(),
            matchers: Matchers::new(),
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
    fn on_output(&mut self, output: &NodeLite) -> Result<()> {
        self.run(&format!(
            "focus output {}",
            output
                .name
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("output with no name"))?
        ))?;
        Ok(())
    }
    fn on_workspace(&mut self, workspace: &NodeLite) -> Result<()> {
        self.run(&format!(
            "workspace {}",
            workspace
                .name
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("workspace with no name"))?
        ))?;
        Ok(())
    }
    fn on_container_enter(&mut self, con: &NodeLite) -> Result<()> {
        self.run("splith")?;
        let layout = match con.layout {
            NodeLayout::SplitH => "splith",
            NodeLayout::SplitV => "splitv",
            NodeLayout::Tabbed => "tabbed",
            NodeLayout::Stacked => "stacked",
            _ => anyhow::bail!("usupported layout"),
        };
        self.run(&format!("layout {}", layout))?;
        Ok(())
    }
    fn on_container_exit(&mut self, con: &NodeLite) -> Result<()> {
        let node = self
            .conn
            .get_tree()
            .unwrap()
            .find_focused(|n| n.nodes.iter().any(|c| c.focused))
            .ok_or_else(|| anyhow::anyhow!("no focused window"))?;
        let (dim, tot_size) = match con.layout {
            NodeLayout::SplitV => ("height", node.rect.height),
            NodeLayout::SplitH => ("width", node.rect.width),
            NodeLayout::Tabbed | NodeLayout::Stacked => {
                self.run("focus parent")?;
                return Ok(());
            }
            _ => anyhow::bail!("usupported layout"),
        };

        for c in con.nodes.iter().rev() {
            let perc = c
                .percent
                .ok_or_else(|| anyhow::anyhow!("missing percent field"))?;
            let size = (perc * (tot_size as f64)).floor() as i32;
            self.run(&format!("resize set {} {} px", dim, size))?;
            self.run("focus prev sibling")?;
        }
        self.run("focus parent")?;
        Ok(())
    }
    fn on_view(&mut self, view: &NodeLite) -> Result<()> {
        self.placeholder.new_window("swaystart", "swaystart");
        let node = wait_new_window(&mut self.events, "swaystart")?;
        wait_window_focus(&mut self.events, node.id)?;
        self.matchers.add(node.id, view.swallows.clone());
        Ok(())
    }
}

struct Swapper {
    conn: Connection,
    events: EventStream,
    matchers: Matchers,
}

impl Swapper {
    fn new(matchers: Matchers) -> Result<Self> {
        let swapper = Swapper {
            conn: Connection::new()?,
            events: Connection::new()?.subscribe(&[EventType::Window])?,
            matchers,
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
        while let Some(event) = self.events.next() {
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
                        println!("{:?}", w);
                        if let Some(id) = self.matchers.consume(&w.container) {
                            self.run(&format!(
                                "[con_id={id}] swap container with con_id {}",
                                w.container.id
                            ))?;
                            self.run(&format!("[con_id={id}] kill"))?;
                        } else {
                            self.run(&format!("[con_id={}] floating enable", w.container.id))?;
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
    let tree: NodeLite = serde_json::from_str(&conf)?;

    let mut builder = LayoutBuilder::new()?;
    builder.visit_node(&tree)?;

    let LayoutBuilder {
        placeholder,
        matchers,
        ..
    } = builder;

    let mut swapper = Swapper::new(matchers)?;
    swapper.swap()?;

    placeholder.wait_until_idle();

    Ok(())
}
