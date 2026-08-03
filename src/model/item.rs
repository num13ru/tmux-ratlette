use super::Action;

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
