use anyhow::Result;
use swayipc::{NodeType, Node};

use crate::node::*;

macro_rules! impl_visitor {
    ($trait:ident, $node:ty) => {
        pub trait $trait {
            fn visit_node(&mut self, node: $node) -> Result<()> {
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
            fn on_container_enter(&mut self, _con: $node) -> Result<()> {
                Ok(())
            }
            fn on_container_exit(&mut self, _con: $node) -> Result<()> {
                Ok(())
            }
            fn on_view(&mut self, _view: $node) -> Result<()> {
                Ok(())
            }
            fn on_workspace(&mut self, _workspace: $node) -> Result<()> {
                Ok(())
            }
            fn on_output(&mut self, _output: $node) -> Result<()> {
                Ok(())
            }
        }
    };
}
impl_visitor!(LayoutLiteVisitor, &NodeLite);
impl_visitor!(LayoutVisitor, &Node);

pub trait Tree {
    fn children<'a>(&'a self) -> std::slice::Iter<'a, Self> where Self: Sized;
    fn is_leaf(&self) -> bool;
}
impl Tree for Node {
    fn children<'a>(&'a self) -> std::slice::Iter<'a, Self> {
        self.nodes.iter()
    }
    fn is_leaf(&self) -> bool {
        self.nodes.is_empty()
    }
}
impl Tree for NodeLite {
    fn children<'a>(&'a self) -> std::slice::Iter<'a, Self> {
        self.nodes.iter()
    }
    fn is_leaf(&self) -> bool {
        self.nodes.is_empty()
    }
}

pub fn iter_tree<'a, T: Tree>(t: &'a T) -> impl Iterator<Item = &'a T> {
    fn traverse_depth<'b, T: Tree>(start: &'b T, stack: &mut Vec<std::slice::Iter<'b, T>>) -> Option<std::slice::Iter<'b, T>> {
        let mut node = start;
        loop {
            if node.is_leaf() {
                break Some(node.children());
            } else {
                stack.push(node.children());
            }
            node = stack.last_mut().unwrap().next()?;
        }
    }
    let mut stack = Vec::new();
    let mut leaf = traverse_depth(t, &mut stack);
    std::iter::from_fn(move || loop {
        if let Some(next) = leaf.as_mut()?.next() {
            break Some(next);
        }
        if let Some(next) = stack.last_mut()?.next() {
            leaf = traverse_depth(next, &mut stack);
        } else {
            stack.pop();
        }
    })
}
