use super::Action;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemData {
    None,
    FindPane(Box<FindPaneRow>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindPaneRow {
    Session {
        session: String,
        count: usize,
        path: String,
        is_current: bool,
    },
    Window {
        session: String,
        window_index: String,
        tree_prefix: String,
    },
    Pane {
        session: String,
        window_index: String,
        pane_index: String,
        window_name: String,
        tree_prefix: String,
        command: String,
        path: String,
        target: String,
        agent: String,
        pane_active: bool,
        is_current: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub shortcut: Option<String>,
    pub category: Option<String>,
    pub aliases: Vec<String>,
    pub action: Action,
    pub selectable: bool,
    pub data: ItemData,
}

impl Item {
    pub fn new(title: impl Into<String>, action: Action) -> Self {
        Self {
            icon: None,
            icon_color: None,
            title: title.into(),
            description: None,
            shortcut: None,
            category: None,
            aliases: Vec::new(),
            action,
            selectable: true,
            data: ItemData::None,
        }
    }

    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }
}
