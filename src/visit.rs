use anyhow::Result;
use swayipc::NodeType;

use crate::node::*;

pub trait LayoutVisitor {
    fn visit_node(&mut self, node: &NodeLite) -> Result<()> {
        for c in &node.nodes {
            let mut is_container = false;
            match c.node_type {
                NodeType::Output => self.on_output(c)?,
                NodeType::Workspace => self.on_workspace(c)?,
                NodeType::Con => {
                    if c.nodes.is_empty() {
                        self.on_view(c)?
                    } else {
                        is_container = true;
                        self.on_container_enter(c)?;
                    }
                }
                NodeType::Root => {
                    if c.name.as_deref() == Some("__i3") {
                        continue;
                    }
                }
                _ => {}
            }
            self.visit_node(c)?;
            if is_container {
                self.on_container_exit(c)?;
            }
        }
        Ok(())
    }
    fn on_container_enter(&mut self, _con: &NodeLite) -> Result<()> {
        Ok(())
    }
    fn on_container_exit(&mut self, _con: &NodeLite) -> Result<()> {
        Ok(())
    }
    fn on_view(&mut self, _view: &NodeLite) -> Result<()> {
        Ok(())
    }
    fn on_workspace(&mut self, _workspace: &NodeLite) -> Result<()> {
        Ok(())
    }
    fn on_output(&mut self, _output: &NodeLite) -> Result<()> {
        Ok(())
    }
}
