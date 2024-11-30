use serde::{Deserialize, Serialize};
use swayipc::NodeLayout;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Matcher {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}


#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct WindowPropertiesLite {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct NodeLite {
    /// The name of the node such as the output name or window title. For the
    /// scratchpad, this will be __i3_scratch for compatibility with i3.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The node type. It can be root, output, workspace, con, or floating_con.
    #[serde(rename = "type")]
    pub node_type: swayipc::NodeType,
    /// The node's layout.  It can either be splith, splitv, stacked, tabbed, or
    /// output.
    pub layout: NodeLayout,
    /// The percentage of the node's parent that it takes up or null for the
    /// root and other special nodes such as the scratchpad.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    /// The tiling children nodes for the node.
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<NodeLite>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num: Option<i32>, //workspace number if `node_type` == `NodeType::Workspace`
    /// (Only views) For an xdg-shell view, the name of the application, if set.
    /// Otherwise, null.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    /// (Only xwayland views) An object containing the title, class, instance,
    /// window_role, window_type, and transient_for for the view.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_properties: Option<WindowPropertiesLite>,
    #[serde(default)]
    pub swallows: Vec<Matcher>,
}

