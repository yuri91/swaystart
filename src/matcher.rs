use swayipc::Node;

use crate::node::Matcher;

pub struct Matchers {
    data: Vec<(i64, Vec<Matcher>)>,
}
impl Matchers {
    pub fn new() -> Self {
        Self { data: vec![] }
    }
    fn matches(&self, node: &Node) -> Option<usize> {
        self.data
            .iter()
            .position(|(_, v)| v.iter().any(|m| m.matches(node)))
    }
    pub fn consume(&mut self, node: &Node) -> Option<i64> {
        if let Some(idx) = self.matches(node) {
            let (id, _) = self.data.remove(idx);
            Some(id)
        } else {
            None
        }
    }
    pub fn remove(&mut self, id: i64) -> bool {
        if let Some(idx) = self.data.iter().position(|(i, _)| *i == id) {
            self.data.remove(idx);
            true
        } else {
            false
        }
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn add(&mut self, id: i64, ms: Vec<Matcher>) {
        self.data.push((id, ms));
    }
}

macro_rules! match_on {
    ($self:expr, $node:expr, $field:ident) => {
        match ($self.$field.as_deref(), $node.$field.as_deref()) {
            (Some(matcher), Some(target)) => {
                return Self::match_inner(matcher, target);
            }
            (Some(_), None) => {
                return false;
            }
            _ => {}
        }
    };
}
impl Matcher {
    fn matches(&self, node: &Node) -> bool {
        match_on!(self, node, app_id);
        match_on!(self, node, name);
        let wp = Matcher {
            app_id: None,
            name: None,
            class: node
                .window_properties
                .as_ref()
                .and_then(|p| p.class.clone()),
            instance: node
                .window_properties
                .as_ref()
                .and_then(|p| p.instance.clone()),
        };
        match_on!(self, wp, class);
        match_on!(self, wp, instance);
        true
    }
    fn match_inner(matcher: &str, target: &str) -> bool {
        matcher == target
    }
}
