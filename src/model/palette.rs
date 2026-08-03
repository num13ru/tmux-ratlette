use super::Item;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub name: String,
    pub title: String,
    pub grouped: bool,
    pub empty_text: String,
    pub items: Vec<Item>,
}

impl Palette {
    pub fn new(name: impl Into<String>, title: impl Into<String>, items: Vec<Item>) -> Self {
        Self {
            name: name.into(),
            title: title.into(),
            grouped: true,
            empty_text: "No commands".to_owned(),
            items,
        }
    }

    pub fn filter_category(&mut self, category: &str) {
        self.items
            .retain(|item| item.category.as_deref() == Some(category));
        self.title = category.to_owned();
        self.grouped = false;
    }
}
