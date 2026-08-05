use super::{Item, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteFilter {
    Default,
    FindPaneTree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub name: String,
    pub title: String,
    pub grouped: bool,
    pub empty_text: String,
    pub theme: Theme,
    pub items: Vec<Item>,
    pub filter: PaletteFilter,
    pub initial_selected: Option<usize>,
}

impl Palette {
    pub fn new(name: impl Into<String>, title: impl Into<String>, items: Vec<Item>) -> Self {
        Self {
            name: name.into(),
            title: title.into(),
            grouped: true,
            empty_text: "No results".to_owned(),
            theme: crate::themes::default_theme(),
            items,
            filter: PaletteFilter::Default,
            initial_selected: None,
        }
    }

    pub fn filter_category(&mut self, category: &str) {
        self.items
            .retain(|item| item.category.as_deref() == Some(category));
        self.title = category.to_owned();
        self.grouped = false;
    }
}
