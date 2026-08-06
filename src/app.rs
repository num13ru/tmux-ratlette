use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::num::NonZeroU16;
use std::path::PathBuf;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::cli::{Invocation, Mode};
use crate::config::resolve_config_dir;
use crate::dispatch;
use crate::model::{
    Action, FindPaneRow, Item, ItemData, Palette, PaletteFilter, Theme, ThemeColor,
};
use crate::terminal::TerminalSession;
use crate::user_config::{EscapeBehavior, NavigationConfig, RuntimeConfig, SizingConfig};
use crate::{Result, palettes, themes};

const DEFAULT_EMPTY_HEIGHT: u16 = 7;
const DEFAULT_PAD_X: u16 = 3;
const UNBORDERED_CHROME_ROWS: u16 = 7;
const BORDERED_CHROME_ROWS: u16 = 5;
const PAGE_SIZE: isize = 10;

#[derive(Debug, Clone, Copy)]
struct ThemeStyles {
    panel: Style,
    header: Style,
    category: Style,
    item: Style,
    selected: Style,
    selected_title: Style,
    accent: Style,
    selected_accent: Style,
    muted: Style,
    selected_muted: Style,
    status: Style,
    alias: Style,
    pane_active: Style,
    selected_pane_active: Style,
}

impl ThemeStyles {
    fn new(theme: Theme) -> Self {
        let selected_fg = theme.selected_fg.unwrap_or(theme.fg);
        let selected_accent = theme.selected_fg.unwrap_or(theme.accent);
        Self {
            panel: themed_style(theme.fg, theme.panel),
            header: themed_style(theme.title_fg.unwrap_or(theme.fg), theme.panel)
                .add_modifier(Modifier::BOLD),
            category: themed_style(theme.accent, theme.panel).add_modifier(Modifier::BOLD),
            item: themed_style(theme.muted, theme.panel),
            selected: themed_style(selected_fg, theme.selected),
            selected_title: themed_style(selected_fg, theme.selected).add_modifier(Modifier::BOLD),
            accent: themed_style(theme.accent, theme.panel),
            selected_accent: themed_style(selected_accent, theme.selected),
            muted: themed_style(theme.muted, theme.panel),
            selected_muted: themed_style(theme.muted, theme.selected),
            status: themed_style(theme.accent, theme.panel),
            alias: themed_style(theme.fg, theme.bg),
            pane_active: Style::new()
                .fg(Color::Rgb(166, 227, 161))
                .bg(theme.panel.ratatui()),
            selected_pane_active: Style::new()
                .fg(Color::Rgb(166, 227, 161))
                .bg(theme.selected.ratatui()),
        }
    }
}

fn themed_style(foreground: ThemeColor, background: ThemeColor) -> Style {
    Style::new()
        .fg(foreground.ratatui())
        .bg(background.ratatui())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    pub rows: u16,
    pub width: u16,
    pub pad_x: u16,
    pub border: String,
    pub body_style: String,
    pub border_style: String,
}

impl std::fmt::Display for Measurement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.rows, self.width, self.pad_x, self.border, self.body_style, self.border_style
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Category(String),
    Item(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutOptions {
    pad_x: u16,
    bordered: bool,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            pad_x: DEFAULT_PAD_X,
            bordered: false,
        }
    }
}

impl LayoutOptions {
    fn from_env(default_pad_x: u16) -> Self {
        Self::from_values(
            std::env::var_os("TMUX_PALETTE_PADX").as_deref(),
            std::env::var_os("TMUX_PALETTE_BORDERED").as_deref(),
            default_pad_x,
        )
    }

    fn from_values(pad_x: Option<&OsStr>, bordered: Option<&OsStr>, default_pad_x: u16) -> Self {
        let pad_x = pad_x
            .and_then(OsStr::to_str)
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(default_pad_x);
        Self {
            pad_x,
            bordered: bordered == Some(OsStr::new("1")),
        }
    }

    fn chrome_rows(self) -> u16 {
        if self.bordered {
            BORDERED_CHROME_ROWS
        } else {
            UNBORDERED_CHROME_ROWS
        }
    }
}

fn nonempty_environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[derive(Debug)]
struct App {
    palette: Palette,
    selected: Option<usize>,
    scroll: usize,
    filter: String,
    filter_cursor: usize,
    layout: LayoutOptions,
    status: Option<String>,
    dispatch_path: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    navigation: NavigationConfig,
    escape: EscapeBehavior,
    sizing: SizingConfig,
    popup_wrapper: Option<String>,
    tmux_binary: String,
    initial_palette: String,
    initial_category: Option<String>,
    relaunch_no_mouse: bool,
    relaunch_debug: bool,
    relaunch_client_width: Option<NonZeroU16>,
    relaunch_client_height: Option<NonZeroU16>,
    stack: Vec<NavState>,
    should_quit: bool,
}

#[derive(Debug)]
struct NavState {
    palette: Palette,
    selected: Option<usize>,
    scroll: usize,
    filter: String,
    filter_cursor: usize,
    status: Option<String>,
}

impl App {
    #[cfg(test)]
    fn new(palette: Palette, dispatch_path: Option<PathBuf>) -> Self {
        Self::new_with_layout(palette, dispatch_path, LayoutOptions::default())
    }

    #[cfg(test)]
    fn new_with_layout(
        palette: Palette,
        dispatch_path: Option<PathBuf>,
        layout: LayoutOptions,
    ) -> Self {
        Self::new_with_context(palette, dispatch_path, layout, None)
    }

    fn new_with_context(
        palette: Palette,
        dispatch_path: Option<PathBuf>,
        layout: LayoutOptions,
        config_dir: Option<PathBuf>,
    ) -> Self {
        Self::new_with_runtime(
            palette,
            dispatch_path,
            layout,
            config_dir,
            RuntimeConfig::default(),
        )
    }

    fn new_with_runtime(
        mut palette: Palette,
        dispatch_path: Option<PathBuf>,
        layout: LayoutOptions,
        config_dir: Option<PathBuf>,
        runtime: RuntimeConfig,
    ) -> Self {
        let RuntimeConfig {
            navigation,
            sizing,
            warnings,
        } = runtime;
        palette.warnings.extend(warnings);
        let initial_palette = palette.name.clone();
        let selected = palette
            .initial_selected
            .filter(|index| {
                palette
                    .items
                    .get(*index)
                    .is_some_and(|item| item.selectable)
            })
            .or_else(|| first_selectable(&palette.items));
        let status = palette.warnings.first().cloned();
        let mut app = Self {
            palette,
            selected,
            scroll: 0,
            filter: String::new(),
            filter_cursor: 0,
            layout,
            status,
            dispatch_path,
            config_dir,
            navigation,
            escape: sizing.escape,
            sizing,
            popup_wrapper: nonempty_environment("TMUX_PALETTE_WRAPPER"),
            tmux_binary: nonempty_environment("TMUX_PALETTE_TMUX_BIN")
                .unwrap_or_else(|| "tmux".to_owned()),
            initial_palette,
            initial_category: None,
            relaunch_no_mouse: false,
            relaunch_debug: false,
            relaunch_client_width: None,
            relaunch_client_height: None,
            stack: Vec::new(),
            should_quit: false,
        };
        app.preview_selected_theme();
        app
    }

    fn unsupported(
        name: &str,
        dispatch_path: Option<PathBuf>,
        layout: LayoutOptions,
        config_dir: Option<PathBuf>,
    ) -> Self {
        let title = display_palette_name(name);
        let mut palette = Palette::new(name, &title, Vec::new());
        palette.grouped = false;
        palette.empty_text = format!("The {title} palette has not been ported to Rust yet");
        let mut app = Self::new_with_context(palette, dispatch_path, layout, config_dir);
        app.status = Some("Esc closes this placeholder".to_owned());
        app
    }

    fn navigate_to(&mut self, palette: Palette) {
        let previous = NavState {
            palette: std::mem::replace(&mut self.palette, palette),
            selected: self.selected,
            scroll: self.scroll,
            filter: std::mem::take(&mut self.filter),
            filter_cursor: self.filter_cursor,
            status: self.status.take(),
        };
        self.stack.push(previous);
        self.selected = self
            .palette
            .initial_selected
            .filter(|index| {
                self.palette
                    .items
                    .get(*index)
                    .is_some_and(|item| item.selectable)
            })
            .or_else(|| first_selectable(&self.palette.items));
        self.scroll = 0;
        self.filter_cursor = 0;
        self.status = self.palette.warnings.first().cloned();
        self.preview_selected_theme();
    }

    fn navigate_back(&mut self, committed_theme: Option<Theme>) -> bool {
        let Some(mut previous) = self.stack.pop() else {
            return false;
        };
        if let Some(theme) = committed_theme {
            previous.palette.theme = theme;
            for state in &mut self.stack {
                state.palette.theme = theme;
            }
        }
        self.palette = previous.palette;
        self.selected = previous.selected;
        self.scroll = previous.scroll;
        self.filter = previous.filter;
        self.filter_cursor = previous.filter_cursor;
        self.status = previous.status;
        true
    }

    fn escape_or_back(&mut self) {
        if self.escape == EscapeBehavior::Exit || !self.navigate_back(None) {
            self.should_quit = true;
        }
    }

    fn preview_selected_theme(&mut self) {
        let Some(theme) = self
            .selected
            .and_then(|index| self.palette.items.get(index))
            .and_then(|item| match &item.data {
                ItemData::Theme(data) => Some(data.theme),
                ItemData::None | ItemData::FindPane(_) => None,
            })
        else {
            return;
        };
        self.palette.theme = theme;
    }

    fn visible_indices(&self) -> Vec<usize> {
        match self.palette.filter {
            PaletteFilter::Default => {
                crate::fuzzy::default_filter(&self.palette.items, &self.filter)
            }
            PaletteFilter::FindPaneTree => {
                palettes::filter_find_pane(&self.palette.items, &self.filter)
            }
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut last_category: Option<&str> = None;

        for index in self.visible_indices() {
            let item = &self.palette.items[index];
            if self.palette.grouped && self.filter.trim().is_empty() {
                if let Some(category) = item.category.as_deref() {
                    if last_category != Some(category) {
                        rows.push(Row::Category(category.to_owned()));
                        last_category = Some(category);
                    }
                }
            }
            rows.push(Row::Item(index));
        }
        rows
    }

    fn move_selection(&mut self, delta: isize) {
        let selectable = self
            .visible_indices()
            .into_iter()
            .filter(|index| self.palette.items[*index].selectable)
            .collect::<Vec<_>>();
        if selectable.is_empty() {
            self.selected = None;
            return;
        }

        let current = self
            .selected
            .and_then(|selected| selectable.iter().position(|index| *index == selected))
            .unwrap_or(0) as isize;
        let candidate = current.saturating_add(delta);
        let next = if self.navigation.wrap_at_list_ends {
            candidate.rem_euclid(selectable.len() as isize) as usize
        } else {
            candidate.clamp(0, selectable.len().saturating_sub(1) as isize) as usize
        };
        self.selected = Some(selectable[next]);
        self.status = None;
        self.preview_selected_theme();
    }

    fn filter_changed(&mut self) {
        self.selected = self
            .visible_indices()
            .into_iter()
            .find(|index| self.palette.items[*index].selectable);
        self.scroll = 0;
        self.status = None;
        self.preview_selected_theme();
    }

    fn ensure_selection_visible(&mut self, list_height: usize) {
        let rows = self.rows();
        if list_height == 0 || rows.is_empty() {
            self.scroll = 0;
            return;
        }

        if let Some(selected) = self.selected {
            if let Some(selected_row) = rows
                .iter()
                .position(|row| matches!(row, Row::Item(index) if *index == selected))
            {
                if selected_row < self.scroll {
                    self.scroll = selected_row;
                } else if selected_row >= self.scroll.saturating_add(list_height) {
                    self.scroll = selected_row + 1 - list_height;
                }
            }
        }

        self.scroll = self.scroll.min(rows.len().saturating_sub(list_height));
    }

    fn activate_selected(&mut self) {
        let Some(action) = self
            .selected
            .and_then(|selected| self.palette.items.get(selected))
            .map(|item| item.action.clone())
        else {
            self.status = Some(self.palette.empty_text.clone());
            return;
        };

        match action {
            Action::Tmux(_) | Action::Shell(_) => {
                let Some(path) = self.dispatch_path.as_deref() else {
                    self.status = Some(
                        "Command not dispatched: launch through bin/tmux-palette.sh".to_owned(),
                    );
                    return;
                };
                match dispatch::write_action(&action, path) {
                    Ok(true) => self.should_quit = true,
                    Ok(false) => {
                        self.status = Some("This action is not available yet".to_owned());
                    }
                    Err(error) => {
                        self.status = Some(format!("Could not queue command: {error}"));
                    }
                }
            }
            Action::Palette(name) => {
                if let Some(palette) = palettes::load(&name, self.config_dir.as_deref()) {
                    self.navigate_to(palette);
                } else {
                    self.status = Some(format!(
                        "The {} palette has not been ported to Rust yet",
                        display_palette_name(&name)
                    ));
                }
            }
            Action::Popup(action) => {
                let Some(path) = self.dispatch_path.as_deref() else {
                    self.status =
                        Some("Popup not dispatched: launch through bin/tmux-palette.sh".to_owned());
                    return;
                };
                let Some(wrapper) = self.popup_wrapper.as_deref() else {
                    self.status = Some(
                        "Popup not dispatched: palette wrapper path is unavailable".to_owned(),
                    );
                    return;
                };
                let relaunch_arguments = match self.popup_relaunch_arguments() {
                    Ok(arguments) => arguments,
                    Err(error) => {
                        self.status = Some(format!("Could not queue popup: {error}"));
                        return;
                    }
                };
                let shell_command = match dispatch::popup_shell_command(
                    &action,
                    dispatch::PopupContext {
                        sizing: &self.sizing,
                        theme: self.palette.theme,
                        tmux_binary: &self.tmux_binary,
                        wrapper,
                        relaunch_arguments: &relaunch_arguments,
                    },
                ) {
                    Ok(command) => command,
                    Err(error) => {
                        self.status = Some(format!("Could not queue popup: {error}"));
                        return;
                    }
                };
                match dispatch::write_action(&Action::Shell(shell_command), path) {
                    Ok(true) => self.should_quit = true,
                    Ok(false) => {
                        self.status = Some("Could not encode popup action".to_owned());
                    }
                    Err(error) => {
                        self.status = Some(format!("Could not queue popup: {error}"));
                    }
                }
            }
            Action::ApplyTheme(slug) => {
                let Some(config_dir) = self.config_dir.as_deref() else {
                    self.status = Some(
                        "Could not save theme: configuration directory unavailable".to_owned(),
                    );
                    return;
                };
                let selected_theme = self
                    .selected
                    .and_then(|index| self.palette.items.get(index))
                    .and_then(|item| match &item.data {
                        ItemData::Theme(data) if data.slug == slug => Some(data.theme),
                        ItemData::None | ItemData::FindPane(_) | ItemData::Theme(_) => None,
                    });
                let Some(selected_theme) = selected_theme else {
                    self.status =
                        Some("Could not save theme: selected theme data unavailable".to_owned());
                    return;
                };
                match themes::save_active_theme(config_dir, &slug) {
                    Ok(()) => {
                        if !self.navigate_back(Some(selected_theme)) {
                            self.should_quit = true;
                        }
                    }
                    Err(error) => self.status = Some(format!("Could not save theme: {error}")),
                }
            }
            Action::None => {
                self.status = Some("This item has no action".to_owned());
            }
        }
    }

    fn popup_relaunch_arguments(&self) -> std::result::Result<Vec<String>, String> {
        let mut arguments = vec![self.palette.name.clone()];
        if self.palette.name == self.initial_palette {
            if let Some(category) = self.initial_category.as_deref() {
                arguments.push(format!("--category={category}"));
            }
        }
        if let Some(config_dir) = self.config_dir.as_deref() {
            let Some(config_dir) = config_dir.to_str() else {
                return Err("configuration path is not valid UTF-8".to_owned());
            };
            arguments.push(format!("--config-dir={config_dir}"));
        }
        if self.relaunch_no_mouse {
            arguments.push("--no-mouse".to_owned());
        }
        if self.relaunch_debug {
            arguments.push("--debug".to_owned());
        }
        if let Some(width) = self.relaunch_client_width {
            arguments.push(format!("--client-width={width}"));
        }
        if let Some(height) = self.relaunch_client_height {
            arguments.push(format!("--client-height={height}"));
        }
        Ok(arguments)
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_selection(-1),
            MouseEventKind::ScrollDown => self.move_selection(1),
            MouseEventKind::Down(MouseButton::Left) => {
                if escape_rect(area, self.layout).is_some_and(|escape| {
                    mouse.row == escape.y
                        && mouse.column >= escape.x
                        && mouse.column < escape.right()
                }) {
                    self.escape_or_back();
                    return;
                }

                let Some(list) = list_rect(area, self.layout) else {
                    return;
                };
                if mouse.row < list.y
                    || mouse.row >= list.bottom()
                    || mouse.column < list.x
                    || mouse.column >= list.right()
                {
                    return;
                }

                self.ensure_selection_visible(list.height as usize);
                let row_index = self
                    .scroll
                    .saturating_add(mouse.row.saturating_sub(list.y) as usize);
                let Some(Row::Item(index)) = self.rows().get(row_index).cloned() else {
                    return;
                };
                if !self.palette.items[index].selectable {
                    return;
                }
                self.selected = Some(index);
                self.status = None;
                self.preview_selected_theme();
                self.activate_selected();
            }
            MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::Moved
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
            | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle) => {}
        }
    }

    fn insert_character(&mut self, character: char) {
        let byte_index = byte_index_at_character(&self.filter, self.filter_cursor);
        self.filter.insert(byte_index, character);
        self.filter_cursor += 1;
        self.filter_changed();
    }

    fn insert_text(&mut self, text: &str) {
        let text = text
            .chars()
            .filter(|character| !character.is_control())
            .collect::<String>();
        if text.is_empty() {
            return;
        }
        let byte_index = byte_index_at_character(&self.filter, self.filter_cursor);
        self.filter.insert_str(byte_index, &text);
        self.filter_cursor += text.chars().count();
        self.filter_changed();
    }

    fn backspace(&mut self) {
        if self.filter_cursor == 0 {
            return;
        }
        let start = byte_index_at_character(&self.filter, self.filter_cursor - 1);
        let end = byte_index_at_character(&self.filter, self.filter_cursor);
        self.filter.replace_range(start..end, "");
        self.filter_cursor -= 1;
        self.filter_changed();
    }

    fn delete(&mut self) {
        if self.filter_cursor >= self.filter.chars().count() {
            return;
        }
        let start = byte_index_at_character(&self.filter, self.filter_cursor);
        let end = byte_index_at_character(&self.filter, self.filter_cursor + 1);
        self.filter.replace_range(start..end, "");
        self.filter_changed();
    }

    fn delete_word_back(&mut self) {
        let start_cursor = word_back(&self.filter, self.filter_cursor);
        if start_cursor == self.filter_cursor {
            return;
        }
        let start = byte_index_at_character(&self.filter, start_cursor);
        let end = byte_index_at_character(&self.filter, self.filter_cursor);
        self.filter.replace_range(start..end, "");
        self.filter_cursor = start_cursor;
        self.filter_changed();
    }

    fn kill_to_start(&mut self) {
        if self.filter_cursor == 0 {
            return;
        }
        let end = byte_index_at_character(&self.filter, self.filter_cursor);
        self.filter.replace_range(..end, "");
        self.filter_cursor = 0;
        self.filter_changed();
    }

    fn kill_to_end(&mut self) {
        let start = byte_index_at_character(&self.filter, self.filter_cursor);
        if start == self.filter.len() {
            return;
        }
        self.filter.truncate(start);
        self.filter_changed();
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }

        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => self.escape_or_back(),
            KeyCode::Char('c' | 'C') if control => self.should_quit = true,
            KeyCode::Char('k' | 'K') if control && self.navigation.vim_keys => {
                self.move_selection(-1);
            }
            KeyCode::Char('j' | 'J') if control && self.navigation.vim_keys => {
                self.move_selection(1);
            }
            KeyCode::Char('u' | 'U') if control && self.navigation.vim_keys => {
                self.move_selection(-PAGE_SIZE);
            }
            KeyCode::Char('d' | 'D') if control && self.navigation.vim_keys => {
                self.move_selection(PAGE_SIZE);
            }
            KeyCode::Up | KeyCode::Char('p' | 'P') if control => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('n' | 'N') if control => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-PAGE_SIZE),
            KeyCode::PageDown => self.move_selection(PAGE_SIZE),
            KeyCode::Left if control || alt => {
                self.filter_cursor = word_back(&self.filter, self.filter_cursor);
            }
            KeyCode::Right if control || alt => {
                self.filter_cursor = word_forward(&self.filter, self.filter_cursor);
            }
            KeyCode::Left => self.filter_cursor = self.filter_cursor.saturating_sub(1),
            KeyCode::Right => {
                self.filter_cursor = (self.filter_cursor + 1).min(self.filter.chars().count());
            }
            KeyCode::Home => self.filter_cursor = 0,
            KeyCode::Char('a' | 'A') if control => self.filter_cursor = 0,
            KeyCode::End => self.filter_cursor = self.filter.chars().count(),
            KeyCode::Char('e' | 'E') if control => {
                self.filter_cursor = self.filter.chars().count();
            }
            KeyCode::Backspace if control || alt => self.delete_word_back(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Char('w' | 'W') if control => self.delete_word_back(),
            KeyCode::Char('u' | 'U') if control => self.kill_to_start(),
            KeyCode::Char('k' | 'K') if control => self.kill_to_end(),
            KeyCode::Enter => self.activate_selected(),
            KeyCode::Char(character) if !control && !alt => self.insert_character(character),
            _ => {}
        }
    }
}

fn byte_index_at_character(value: &str, character_index: usize) -> usize {
    value
        .char_indices()
        .nth(character_index)
        .map_or(value.len(), |(byte_index, _)| byte_index)
}

fn word_back(value: &str, from: usize) -> usize {
    let characters = value.chars().collect::<Vec<_>>();
    let mut index = from.min(characters.len());
    while index > 0 && characters[index - 1].is_whitespace() {
        index -= 1;
    }
    while index > 0 && !characters[index - 1].is_whitespace() {
        index -= 1;
    }
    index
}

fn word_forward(value: &str, from: usize) -> usize {
    let characters = value.chars().collect::<Vec<_>>();
    let mut index = from.min(characters.len());
    while index < characters.len() && characters[index].is_whitespace() {
        index += 1;
    }
    while index < characters.len() && !characters[index].is_whitespace() {
        index += 1;
    }
    index
}

pub fn run(invocation: Invocation) -> Result<()> {
    let config_dir = resolve_config_dir(invocation.config_dir.as_deref())?;
    let runtime = crate::user_config::runtime(&config_dir);

    match invocation.mode {
        Mode::Measure => {
            println!(
                "{}",
                measure_palette(
                    &invocation.palette,
                    invocation.category.as_deref(),
                    invocation.client_width,
                    invocation.client_height,
                    Some(&config_dir),
                    &runtime.sizing,
                )
            );
            Ok(())
        }
        Mode::Interactive => run_interactive(&invocation, config_dir, runtime),
    }
}

pub fn measure(client_width: Option<NonZeroU16>, client_height: Option<NonZeroU16>) -> Measurement {
    measure_palette(
        "commands",
        None,
        client_width,
        client_height,
        None,
        &SizingConfig::default(),
    )
}

fn measure_palette(
    palette_name: &str,
    category: Option<&str>,
    client_width: Option<NonZeroU16>,
    client_height: Option<NonZeroU16>,
    config_dir: Option<&std::path::Path>,
    sizing: &SizingConfig,
) -> Measurement {
    let (rows, theme) = palettes::load(palette_name, config_dir)
        .map(|mut palette| {
            if let Some(category) = category {
                palette.filter_category(category);
            }
            (desired_height(&palette, sizing.max_height), palette.theme)
        })
        .unwrap_or((
            DEFAULT_EMPTY_HEIGHT.min(sizing.max_height),
            themes::default_theme(),
        ));
    let mut measurement = Measurement {
        rows,
        width: sizing.width,
        pad_x: sizing.pad_x,
        border: sizing.border.clone(),
        body_style: sizing
            .body_style
            .clone()
            .unwrap_or_else(|| theme.tmux_body_style()),
        border_style: sizing
            .border_style
            .clone()
            .unwrap_or_else(|| theme.tmux_border_style()),
    };

    if let Some(width) = client_width.map(NonZeroU16::get) {
        if sizing.mobile_width > 0 && width < sizing.mobile_width {
            measurement.width = width;
            measurement.pad_x = 1;
            if let Some(height) = client_height.map(NonZeroU16::get) {
                measurement.rows = measurement.rows.max(height);
            }
        }
    }

    measurement
}

fn desired_height(palette: &Palette, max_height: u16) -> u16 {
    let categories = if palette.grouped {
        palette
            .items
            .iter()
            .filter_map(|item| item.category.as_deref())
            .collect::<BTreeSet<_>>()
            .len()
    } else {
        0
    };
    let content_rows = palette.items.len().saturating_add(categories);
    u16::try_from(content_rows)
        .unwrap_or(u16::MAX)
        .saturating_add(UNBORDERED_CHROME_ROWS)
        .max(DEFAULT_EMPTY_HEIGHT)
        .min(max_height)
}

fn run_interactive(
    invocation: &Invocation,
    config_dir: PathBuf,
    runtime: RuntimeConfig,
) -> Result<()> {
    let dispatch_path = std::env::var_os("TMUX_PALETTE_CMD").map(PathBuf::from);
    let layout = LayoutOptions::from_env(runtime.sizing.pad_x);
    let mut app = match palettes::load(&invocation.palette, Some(&config_dir)) {
        Some(mut palette) => {
            if let Some(category) = invocation.category.as_deref() {
                palette.filter_category(category);
            }
            App::new_with_runtime(
                palette,
                dispatch_path,
                layout,
                Some(config_dir.clone()),
                runtime,
            )
        }
        None => {
            let mut app =
                App::unsupported(&invocation.palette, dispatch_path, layout, Some(config_dir));
            app.navigation = runtime.navigation;
            app.escape = runtime.sizing.escape;
            if app.status.is_none() {
                app.status = runtime.warnings.first().cloned();
            }
            app
        }
    };
    app.initial_palette = invocation.palette.clone();
    app.initial_category = invocation.category.clone();
    app.relaunch_no_mouse = invocation.no_mouse;
    app.relaunch_debug = invocation.debug;
    app.relaunch_client_width = invocation.client_width;
    app.relaunch_client_height = invocation.client_height;
    let mut terminal = TerminalSession::enter(!invocation.no_mouse)?;

    while !app.should_quit {
        let area = terminal.draw(|frame| render(frame, &mut app))?;

        match event::read().map_err(crate::Error::Terminal)? {
            Event::Key(key) => app.handle_key(key),
            Event::Mouse(mouse) => app.handle_mouse(mouse, area),
            Event::Resize(_, _) => {}
            Event::Paste(text) => app.insert_text(&text),
            Event::FocusGained | Event::FocusLost => {}
        }
    }
    Ok(())
}

fn render(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    if area.is_empty() {
        return;
    }
    let styles = ThemeStyles::new(app.palette.theme);
    frame.render_widget(Block::default().style(styles.panel), area);

    let outer_padding = u16::from(!app.layout.bordered);
    if let Some(header) = row_at(area, outer_padding) {
        render_header(frame, header, &app.palette.title, app.layout, &styles);
    }
    if let Some(search) = row_at(area, outer_padding.saturating_add(1)) {
        render_search(frame, search, app, &styles);
    }

    if let Some(list) = list_rect(area, app.layout) {
        render_list(frame, list, app, &styles);

        let footer_offset = area.height.saturating_sub(outer_padding.saturating_add(1));
        if let Some(footer) = row_at(area, footer_offset) {
            render_footer(frame, footer, app, &styles);
        }
    }
}

fn list_rect(area: Rect, layout: LayoutOptions) -> Option<Rect> {
    (area.height >= layout.chrome_rows()).then(|| {
        let outer_padding = u16::from(!layout.bordered);
        Rect::new(
            area.x,
            area.y.saturating_add(outer_padding.saturating_add(3)),
            area.width,
            area.height.saturating_sub(layout.chrome_rows()),
        )
    })
}

fn escape_rect(area: Rect, layout: LayoutOptions) -> Option<Rect> {
    let outer_padding = u16::from(!layout.bordered);
    let header = row_at(area, outer_padding)?;
    let content = content_rect(header, layout.pad_x);
    let width = width_as_u16(&truncate_width("esc", content.width as usize));
    (width > 0).then(|| Rect::new(content.right().saturating_sub(width), content.y, width, 1))
}

fn row_at(area: Rect, offset: u16) -> Option<Rect> {
    (offset < area.height).then(|| Rect::new(area.x, area.y + offset, area.width, 1))
}

fn content_rect(area: Rect, requested_padding: u16) -> Rect {
    let padding = requested_padding.min(area.width.saturating_sub(1) / 2);
    Rect::new(
        area.x.saturating_add(padding),
        area.y,
        area.width.saturating_sub(padding.saturating_mul(2)),
        area.height,
    )
}

fn render_header(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    layout: LayoutOptions,
    styles: &ThemeStyles,
) {
    let content = content_rect(area, layout.pad_x);
    let escape = truncate_width("esc", content.width as usize);
    let available = content.width.saturating_sub(width_as_u16(&escape)) as usize;
    let title = truncate_width(title, available);
    let gap = content
        .width
        .saturating_sub(width_as_u16(&title))
        .saturating_sub(width_as_u16(&escape)) as usize;
    let line = Line::from(vec![
        Span::styled(title, styles.header),
        Span::styled(" ".repeat(gap), styles.panel),
        Span::styled(escape, styles.muted),
    ]);
    frame.render_widget(Paragraph::new(line).style(styles.panel), content);
}

fn render_search(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let content_area = content_rect(area, app.layout.pad_x);
    if content_area.is_empty() {
        return;
    }
    let text_width = content_area.width.saturating_sub(2) as usize;
    let (visible, cursor_offset) = search_window(&app.filter, app.filter_cursor, text_width);
    let content = if app.filter.is_empty() {
        Span::styled(truncate_width("Search", text_width), styles.muted)
    } else {
        Span::styled(visible, styles.panel)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("▌", styles.accent),
            Span::styled(" ", styles.panel),
            content,
        ]))
        .style(styles.panel),
        content_area,
    );

    let cursor_x = content_area
        .x
        .saturating_add(2)
        .saturating_add(u16::try_from(cursor_offset).unwrap_or(u16::MAX))
        .min(content_area.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, content_area.y));
}

fn search_window(value: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }

    let mut boundaries = value
        .char_indices()
        .map(|(byte_index, _)| byte_index)
        .collect::<Vec<_>>();
    boundaries.push(value.len());
    let cursor = cursor.min(boundaries.len().saturating_sub(1));
    let cursor_byte = boundaries[cursor];
    let cursor_budget = width.saturating_sub(1);
    let start_byte = boundaries
        .iter()
        .copied()
        .take(cursor.saturating_add(1))
        .find(|start| display_width(&value[*start..cursor_byte]) <= cursor_budget)
        .unwrap_or(cursor_byte);
    let mut end_byte = cursor_byte;
    for candidate in boundaries.iter().copied().skip(cursor) {
        if display_width(&value[start_byte..candidate]) <= width {
            end_byte = candidate;
        }
    }
    let cursor_offset = display_width(&value[start_byte..cursor_byte]);

    (value[start_byte..end_byte].to_owned(), cursor_offset)
}

fn render_list(frame: &mut Frame<'_>, area: Rect, app: &mut App, styles: &ThemeStyles) {
    if area.is_empty() {
        app.scroll = 0;
        return;
    }

    app.ensure_selection_visible(area.height as usize);
    let rows = app.rows();
    if rows.is_empty() {
        let content = content_rect(area, app.layout.pad_x);
        frame.render_widget(
            Paragraph::new(truncate_width(
                &app.palette.empty_text,
                content.width as usize,
            ))
            .style(styles.muted),
            content,
        );
        return;
    }

    for (screen_row, row) in rows
        .iter()
        .skip(app.scroll)
        .take(area.height as usize)
        .enumerate()
    {
        let row_area = Rect::new(
            area.x,
            area.y.saturating_add(screen_row as u16),
            area.width,
            1,
        );
        match row {
            Row::Category(category) => {
                let content = content_rect(row_area, app.layout.pad_x);
                frame.render_widget(
                    Paragraph::new(Span::styled(category.as_str(), styles.category))
                        .style(styles.panel),
                    content,
                );
            }
            Row::Item(index) => render_item(
                frame,
                row_area,
                &app.palette.items[*index],
                app.selected == Some(*index),
                app.layout,
                styles,
            ),
        }
    }
}

fn render_item(
    frame: &mut Frame<'_>,
    area: Rect,
    item: &Item,
    selected: bool,
    layout: LayoutOptions,
    styles: &ThemeStyles,
) {
    if let ItemData::FindPane(row) = &item.data {
        render_find_pane_item(frame, area, item, row, selected, layout, styles);
        return;
    }

    let style = if selected {
        styles.selected
    } else {
        styles.item
    };
    let accent = if let ItemData::Theme(data) = &item.data {
        Style::new()
            .fg(data.theme.accent.ratatui())
            .bg(style.bg.unwrap_or(Color::Reset))
    } else if let Some(color) = item.icon_color.as_deref().and_then(ThemeColor::parse) {
        Style::new()
            .fg(color.ratatui())
            .bg(style.bg.unwrap_or(Color::Reset))
    } else if selected {
        styles.selected_accent
    } else {
        styles.accent
    };
    let title = if selected {
        styles.selected_title
    } else {
        styles.item
    };
    frame.render_widget(Block::default().style(style), area);
    let content = content_rect(area, layout.pad_x);
    if content.is_empty() {
        return;
    }
    let marker = if selected { "▌" } else { " " };
    let icon = item.icon.as_deref().unwrap_or(" ");
    let mut spans = vec![
        Span::styled(marker, accent),
        Span::styled(" ", style),
        Span::styled(icon, accent),
        Span::styled("  ", style),
        Span::styled(item.title.as_str(), title),
    ];
    if let Some(alias) = item.aliases.first() {
        spans.push(Span::styled("  ", style));
        spans.push(Span::styled(format!(" {alias} "), styles.alias));
    }
    if let Some(description) = item.description.as_deref() {
        spans.push(Span::styled(
            format!(" - {description}"),
            if selected {
                styles.selected_muted
            } else {
                styles.muted
            },
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).style(style), content);

    if let Some(shortcut) = item.shortcut.as_deref() {
        let shortcut = truncate_width(shortcut, content.width.saturating_sub(1) as usize);
        let shortcut_width = u16::try_from(display_width(&shortcut).saturating_add(1))
            .unwrap_or(u16::MAX)
            .min(content.width);
        let shortcut_area = Rect::new(
            content.right().saturating_sub(shortcut_width),
            content.y,
            shortcut_width,
            1,
        );
        let shortcut_style = if selected {
            styles.selected_accent
        } else {
            styles.muted
        };
        frame.render_widget(
            Paragraph::new(Span::styled(format!(" {shortcut}"), shortcut_style))
                .style(style)
                .alignment(Alignment::Right),
            shortcut_area,
        );
    }
}

fn render_find_pane_item(
    frame: &mut Frame<'_>,
    area: Rect,
    item: &Item,
    row: &FindPaneRow,
    selected: bool,
    layout: LayoutOptions,
    styles: &ThemeStyles,
) {
    let row_style = if selected {
        styles.selected
    } else {
        styles.panel
    };
    frame.render_widget(Block::default().style(row_style), area);
    let content = content_rect(area, layout.pad_x);
    if content.is_empty() {
        return;
    }

    match row {
        FindPaneRow::Session {
            count,
            path,
            is_current,
            ..
        } => {
            let marker = if *is_current { "▶ " } else { "  " };
            let marker_style = if *is_current {
                styles.accent
            } else {
                row_style
            };
            let path = shorten_home(path);
            let mut spans = vec![
                Span::styled(marker, marker_style),
                Span::styled(
                    item.title.as_str(),
                    styles.accent.add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" ({count})"), styles.muted),
            ];
            if !path.is_empty() {
                spans.push(Span::styled(format!("  {path}"), styles.muted));
            }
            frame.render_widget(Paragraph::new(Line::from(spans)).style(row_style), content);
        }
        FindPaneRow::Window { tree_prefix, .. } => {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(tree_prefix.as_str(), styles.muted),
                    Span::styled(item.title.as_str(), styles.panel),
                ]))
                .style(row_style),
                content,
            );
        }
        FindPaneRow::Pane {
            window_index,
            pane_index,
            tree_prefix,
            agent,
            pane_active,
            is_current,
            ..
        } => {
            let muted_style = if selected {
                styles.selected_muted
            } else {
                styles.muted
            };
            let accent_style = if selected {
                styles.selected_accent
            } else {
                styles.accent
            };
            let marker_style = if *is_current {
                accent_style
            } else if *pane_active {
                if selected {
                    styles.selected_pane_active
                } else {
                    styles.pane_active
                }
            } else {
                muted_style
            };
            let marker = if *is_current {
                "▶"
            } else if *pane_active {
                "●"
            } else {
                "○"
            };
            let title_style = if selected {
                styles.selected_title
            } else if *is_current {
                styles.panel
            } else {
                styles.muted
            };
            let right = format!("{window_index}.{pane_index}");
            let right_width = width_as_u16(&right).min(content.width);
            let left_width = content.width.saturating_sub(right_width.saturating_add(1));
            let left_area = Rect::new(content.x, content.y, left_width, 1);
            let right_area = Rect::new(
                content.right().saturating_sub(right_width),
                content.y,
                right_width,
                1,
            );
            let mut spans = vec![
                Span::styled(tree_prefix.as_str(), muted_style),
                Span::styled(marker, marker_style),
                Span::styled(" ", row_style),
                Span::styled(item.title.as_str(), title_style),
            ];
            if !agent.is_empty() {
                spans.push(Span::styled(format!("  {agent}"), muted_style));
            }
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(row_style),
                left_area,
            );
            frame.render_widget(
                Paragraph::new(Span::styled(right, muted_style))
                    .style(row_style)
                    .alignment(Alignment::Right),
                right_area,
            );
        }
    }
}

fn shorten_home(path: &str) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.to_owned();
    };
    let home = home.to_string_lossy();
    path.strip_prefix(home.as_ref())
        .map_or_else(|| path.to_owned(), |suffix| format!("~{suffix}"))
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let (text, style) = match app.status.as_deref() {
        Some(status) => (status.to_owned(), styles.status),
        None => {
            let count = app
                .visible_indices()
                .into_iter()
                .filter(|index| app.palette.items[*index].selectable)
                .count();
            if count == 0 {
                (app.palette.empty_text.clone(), styles.muted)
            } else {
                let noun = if count == 1 { "command" } else { "commands" };
                (
                    format!("enter select   up/down move   {count} {noun}"),
                    styles.muted,
                )
            }
        }
    };
    let content = content_rect(area, app.layout.pad_x);
    frame.render_widget(
        Paragraph::new(truncate_width(&text, content.width as usize))
            .style(style)
            .alignment(Alignment::Left),
        content,
    );
}

fn first_selectable(items: &[Item]) -> Option<usize> {
    items.iter().position(|item| item.selectable)
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn width_as_u16(value: &str) -> u16 {
    u16::try_from(display_width(value)).unwrap_or(u16::MAX)
}

fn truncate_width(value: &str, max_width: usize) -> String {
    let mut end = 0;
    for (start, character) in value.char_indices() {
        let candidate = start.saturating_add(character.len_utf8());
        if display_width(&value[..candidate]) <= max_width {
            end = candidate;
        }
    }
    value[..end].to_owned()
}

fn display_palette_name(name: &str) -> String {
    name.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().chain(characters).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    fn test_item(title: &str) -> Item {
        Item::new(title, Action::tmux(format!("display-message '{title}'")))
    }

    fn test_palette(items: Vec<Item>) -> Palette {
        Palette::new("test", "Test", items)
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn control(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn temp_file() -> PathBuf {
        let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "tmux-ratlette-app-test-{}-{sequence}",
            std::process::id()
        ))
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn buffer_row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer();
        (buffer.area.x..buffer.area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn default_measurement_fits_the_commands_palette_cap() {
        let result = measure(None, None);

        assert_eq!(
            result.to_string(),
            "28\t90\t3\tnone\tbg=#2d2b55\tfg=#fad000,bg=default"
        );
    }

    #[test]
    fn category_measurement_uses_only_matching_commands() {
        let result = measure_palette(
            "commands",
            Some("System"),
            None,
            None,
            None,
            &SizingConfig::default(),
        );

        assert_eq!(result.rows, 8);
    }

    #[test]
    fn empty_palette_measurement_uses_only_the_seven_chrome_rows() {
        assert_eq!(
            desired_height(&test_palette(Vec::new()), 28),
            DEFAULT_EMPTY_HEIGHT
        );
    }

    #[test]
    fn narrow_clients_use_full_width_and_height() {
        let result = measure(NonZeroU16::new(60), NonZeroU16::new(30));

        assert_eq!(result.width, 60);
        assert_eq!(result.rows, 30);
        assert_eq!(result.pad_x, 1);
    }

    #[test]
    fn configured_sizing_controls_measurement_and_can_disable_mobile_mode() {
        let sizing = SizingConfig {
            width: 72,
            max_height: 12,
            pad_x: 2,
            mobile_width: 0,
            border: "rounded".to_owned(),
            body_style: Some("bg=#010203".to_owned()),
            border_style: Some("fg=blue".to_owned()),
            ..SizingConfig::default()
        };

        let result = measure_palette(
            "commands",
            None,
            NonZeroU16::new(60),
            NonZeroU16::new(30),
            None,
            &sizing,
        );

        assert_eq!(result.rows, 12);
        assert_eq!(result.width, 72);
        assert_eq!(result.pad_x, 2);
        assert_eq!(result.border, "rounded");
        assert_eq!(result.body_style, "bg=#010203");
        assert_eq!(result.border_style, "fg=blue");
    }

    #[test]
    fn short_mobile_clients_still_fit_the_palette_chrome() {
        let result = measure_palette(
            "missing",
            None,
            NonZeroU16::new(60),
            NonZeroU16::new(2),
            None,
            &SizingConfig::default(),
        );

        assert_eq!(result.rows, DEFAULT_EMPTY_HEIGHT);
    }

    #[test]
    fn layout_options_validate_environment_values() {
        assert_eq!(
            LayoutOptions::from_values(Some(OsStr::new("5")), Some(OsStr::new("1")), 3),
            LayoutOptions {
                pad_x: 5,
                bordered: true,
            }
        );
        assert_eq!(
            LayoutOptions::from_values(Some(OsStr::new("-1")), Some(OsStr::new("true")), 3),
            LayoutOptions::default()
        );

        let content = content_rect(Rect::new(0, 0, 4, 1), u16::MAX);
        assert_eq!(content, Rect::new(1, 0, 2, 1));
    }

    #[test]
    fn navigation_wraps_and_skips_non_selectable_items() {
        let mut disabled = test_item("Disabled");
        disabled.selectable = false;
        let mut app = App::new(
            test_palette(vec![test_item("First"), disabled, test_item("Last")]),
            None,
        );

        app.move_selection(1);
        assert_eq!(app.selected, Some(2));
        app.move_selection(1);
        assert_eq!(app.selected, Some(0));
        app.move_selection(-1);
        assert_eq!(app.selected, Some(2));
    }

    #[test]
    fn leading_plugin_failure_row_is_skipped_and_cannot_block_static_actions() {
        let path = temp_file();
        let mut failure = Item::new("Plugin command failed", Action::None).icon("!");
        failure.selectable = false;
        let mut app = App::new(
            test_palette(vec![failure, test_item("Static fallback")]),
            Some(path.clone()),
        );

        assert_eq!(app.selected, Some(1));
        app.handle_key(press(KeyCode::Enter));

        assert!(app.should_quit);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "tmux:display-message 'Static fallback'"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn navigation_config_clamps_and_enables_vim_control_keys() {
        let runtime = RuntimeConfig {
            navigation: NavigationConfig {
                wrap_at_list_ends: false,
                vim_keys: true,
            },
            ..RuntimeConfig::default()
        };
        let mut app = App::new_with_runtime(
            test_palette(vec![
                test_item("First"),
                test_item("Middle"),
                test_item("Final"),
            ]),
            None,
            LayoutOptions::default(),
            None,
            runtime,
        );

        app.move_selection(-1);
        assert_eq!(app.selected, Some(0));
        app.handle_key(control(KeyCode::Char('d')));
        assert_eq!(app.selected, Some(2));
        app.handle_key(control(KeyCode::Char('j')));
        assert_eq!(app.selected, Some(2));
        app.filter = "i".to_owned();
        app.filter_cursor = 1;
        app.handle_key(control(KeyCode::Char('u')));
        assert_eq!(app.selected, Some(0));
        assert_eq!(app.filter, "i");
        app.handle_key(control(KeyCode::Char('k')));
        assert_eq!(app.selected, Some(0));
    }

    #[test]
    fn sizing_escape_exit_closes_instead_of_popping_navigation_stack() {
        let sizing = SizingConfig {
            escape: EscapeBehavior::Exit,
            ..SizingConfig::default()
        };
        let runtime = RuntimeConfig {
            sizing,
            ..RuntimeConfig::default()
        };
        let mut app = App::new_with_runtime(
            test_palette(vec![test_item("Root")]),
            None,
            LayoutOptions::default(),
            None,
            runtime,
        );
        app.navigate_to(Palette::new(
            "nested",
            "Nested",
            vec![test_item("Nested item")],
        ));

        app.handle_key(press(KeyCode::Esc));

        assert!(app.should_quit);
        assert_eq!(app.palette.name, "nested");
        assert_eq!(app.stack.len(), 1);
    }

    #[test]
    fn palette_initial_selection_is_used_when_selectable_and_falls_back_when_invalid() {
        let mut palette = test_palette(vec![test_item("First"), test_item("Current")]);
        palette.initial_selected = Some(1);
        assert_eq!(App::new(palette, None).selected, Some(1));

        let mut palette = test_palette(vec![test_item("First"), test_item("Disabled")]);
        palette.items[1].selectable = false;
        palette.initial_selected = Some(1);
        assert_eq!(App::new(palette, None).selected, Some(0));
    }

    #[test]
    fn mouse_wheel_wraps_and_skips_non_selectable_items() {
        let mut disabled = test_item("Disabled");
        disabled.selectable = false;
        let mut app = App::new(
            test_palette(vec![test_item("First"), disabled, test_item("Last")]),
            None,
        );
        let area = Rect::new(0, 0, 30, 10);

        app.handle_mouse(mouse(MouseEventKind::ScrollDown, 29, 9), area);
        assert_eq!(app.selected, Some(2));
        app.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0), area);
        assert_eq!(app.selected, Some(0));
        app.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, 0), area);
        assert_eq!(app.selected, Some(2));
        app.handle_mouse(mouse(MouseEventKind::ScrollLeft, 0, 0), area);
        assert_eq!(app.selected, Some(2));
    }

    #[test]
    fn mouse_click_activates_the_correct_scrolled_row_with_categories() {
        let path = temp_file();
        let items = (0..9)
            .map(|index| test_item(&format!("Item {index}")).category("Group"))
            .collect();
        let mut app = App::new(test_palette(items), Some(path.clone()));
        app.selected = Some(8);
        let area = Rect::new(0, 0, 30, 10);
        app.ensure_selection_visible(list_rect(area, app.layout).unwrap().height as usize);

        assert_eq!(app.scroll, 7);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 4), area);

        assert_eq!(app.selected, Some(6));
        assert!(app.should_quit);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "tmux:display-message 'Item 6'"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn mouse_click_ignores_categories_disabled_rows_and_non_press_events() {
        let mut disabled = test_item("Disabled").category("Group");
        disabled.selectable = false;
        let mut app = App::new(
            test_palette(vec![test_item("First").category("Group"), disabled]),
            None,
        );
        let area = Rect::new(0, 0, 30, 12);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 3, 4), area);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 3, 6), area);
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 3, 5), area);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), 3, 5), area);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 3, 10), area);

        assert_eq!(app.selected, Some(0));
        assert_eq!(app.status, None);
        assert!(!app.should_quit);
    }

    #[test]
    fn mouse_escape_hit_tracks_bordered_and_unbordered_headers() {
        let area = Rect::new(0, 0, 30, 10);
        let mut unbordered = App::new(test_palette(vec![test_item("Run")]), None);
        unbordered.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 24, 1), area);
        assert!(unbordered.should_quit);

        let mut bordered = App::new_with_layout(
            test_palette(vec![test_item("Run")]),
            None,
            LayoutOptions {
                pad_x: 1,
                bordered: true,
            },
        );
        bordered.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 26, 0), area);
        assert!(bordered.should_quit);
    }

    #[test]
    fn mouse_events_are_safe_for_empty_and_tiny_layouts() {
        let mut app = App::new(test_palette(Vec::new()), None);
        let area = Rect::new(0, 0, 1, 1);

        app.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0), area);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0), area);

        assert_eq!(app.selected, None);
        assert_eq!(app.status, None);
        assert!(!app.should_quit);
    }

    #[test]
    fn empty_palette_navigation_and_activation_are_safe() {
        let mut app = App::new(test_palette(Vec::new()), None);

        app.move_selection(1);
        app.activate_selected();

        assert_eq!(app.selected, None);
        assert_eq!(app.status.as_deref(), Some("No results"));
        assert!(!app.should_quit);
    }

    #[test]
    fn scrolling_keeps_the_selected_item_visible_with_category_rows() {
        let items = (0..8)
            .map(|index| test_item(&format!("Item {index}")).category("Group"))
            .collect();
        let mut app = App::new(test_palette(items), None);
        app.move_selection(6);

        app.ensure_selection_visible(3);

        assert_eq!(app.selected, Some(6));
        assert_eq!(app.scroll, 5);
    }

    #[test]
    fn activation_queues_a_tmux_command_and_exits() {
        let path = temp_file();
        let mut app = App::new(test_palette(vec![test_item("Run")]), Some(path.clone()));

        app.activate_selected();

        assert!(app.should_quit);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "tmux:display-message 'Run'"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn popup_activation_queues_launch_and_relaunch_then_exits() {
        let path = temp_file();
        let item = Item::new("Logs", Action::popup("tail -f app.log"));
        let mut app = App::new(test_palette(vec![item]), Some(path.clone()));
        app.popup_wrapper = Some("/tmp/tmux-palette.sh".to_owned());
        app.tmux_binary = "/opt/tmux".to_owned();
        app.config_dir = Some(PathBuf::from("/tmp/config's"));
        app.initial_category = Some("Tools & logs".to_owned());
        app.relaunch_no_mouse = true;
        app.relaunch_debug = true;
        app.relaunch_client_width = NonZeroU16::new(120);
        app.relaunch_client_height = NonZeroU16::new(40);

        app.activate_selected();

        assert!(app.should_quit);
        let queued = fs::read_to_string(&path).unwrap();
        assert!(queued.starts_with("shell:'/opt/tmux' display-popup -E"));
        assert!(queued.contains("'tail -f app.log'"));
        assert!(queued.contains("run-shell -b"));
        assert!(queued.contains("/tmp/tmux-palette.sh"));
        assert!(queued.contains("'test'"));
        assert!(queued.contains("category"));
        assert!(queued.contains("Tools & logs"));
        assert!(queued.contains("config"));
        assert!(queued.contains("--no-mouse"));
        assert!(queued.contains("--debug"));
        assert!(queued.contains("--client-width=120"));
        assert!(queued.contains("--client-height=40"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn popup_relaunch_does_not_apply_the_initial_category_to_a_nested_palette() {
        let mut app = App::new(test_palette(Vec::new()), None);
        app.initial_category = Some("Tools".to_owned());
        app.palette.name = "nested".to_owned();

        assert_eq!(app.popup_relaunch_arguments().unwrap(), ["nested"]);
    }

    #[test]
    fn popup_activation_stays_open_when_wrapper_context_is_missing() {
        let path = temp_file();
        let item = Item::new("Logs", Action::popup("tail -f app.log"));
        let mut app = App::new(test_palette(vec![item]), Some(path.clone()));
        app.popup_wrapper = None;

        app.activate_selected();

        assert!(!app.should_quit);
        assert!(!path.exists());
        assert!(app.status.unwrap().contains("wrapper path is unavailable"));
    }

    #[test]
    fn direct_runs_do_not_dispatch_without_the_wrapper_file() {
        let mut app = App::new(test_palette(vec![test_item("Run")]), None);

        app.activate_selected();

        assert!(!app.should_quit);
        assert!(app.status.unwrap().contains("launch through"));
    }

    #[test]
    fn nested_palette_actions_report_the_unavailable_feature() {
        let item = Item::new("Missing", Action::palette("missing"));
        let mut app = App::new(test_palette(vec![item]), None);

        app.activate_selected();

        assert!(!app.should_quit);
        assert!(app.status.unwrap().contains("Missing palette"));
    }

    #[test]
    fn available_nested_palette_actions_replace_the_current_palette_and_reset_state() {
        let item = Item::new("Commands", Action::palette("commands"));
        let mut app = App::new(test_palette(vec![item]), None);
        app.filter = "stale".to_owned();
        app.filter_cursor = 5;
        app.scroll = 3;
        app.status = Some("stale".to_owned());

        app.activate_selected();

        assert_eq!(app.palette.name, "commands");
        assert_eq!(app.selected, Some(0));
        assert!(app.filter.is_empty());
        assert_eq!(app.filter_cursor, 0);
        assert_eq!(app.scroll, 0);
        assert_eq!(app.status, None);
        assert_eq!(app.stack.len(), 1);
        assert!(!app.should_quit);
    }

    #[test]
    fn theme_preview_changes_with_selection_and_escape_restores_previous_palette_state() {
        let directory = temp_file();
        fs::create_dir(&directory).unwrap();
        let original_theme = themes::default_theme();
        let item = Item::new("Themes", Action::palette("themes"));
        let mut app = App::new_with_context(
            test_palette(vec![item]),
            None,
            LayoutOptions::default(),
            Some(directory.clone()),
        );
        app.filter = "saved query".to_owned();
        app.filter_cursor = 11;

        app.activate_selected();
        let first_preview = app.palette.theme;
        app.move_selection(1);

        assert_eq!(app.palette.name, "themes");
        assert_ne!(first_preview, app.palette.theme);
        assert_ne!(app.palette.theme, original_theme);
        app.handle_key(press(KeyCode::Esc));
        assert_eq!(app.palette.name, "test");
        assert_eq!(app.palette.theme, original_theme);
        assert_eq!(app.filter, "saved query");
        assert_eq!(app.filter_cursor, 11);
        assert!(!app.should_quit);
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn applying_a_theme_persists_and_returns_with_the_committed_theme() {
        let directory = temp_file();
        fs::create_dir(&directory).unwrap();
        let item = Item::new("Themes", Action::palette("themes"));
        let mut app = App::new_with_context(
            test_palette(vec![item]),
            None,
            LayoutOptions::default(),
            Some(directory.clone()),
        );

        app.activate_selected();
        let committed = app.palette.theme;
        app.activate_selected();

        assert_eq!(app.palette.name, "test");
        assert_eq!(app.palette.theme, committed);
        assert!(app.stack.is_empty());
        assert_eq!(
            fs::read_to_string(directory.join("theme.json")).unwrap(),
            "{\n  \"name\": \"ayu-dark\"\n}\n"
        );
        assert!(!app.should_quit);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn theme_save_failure_stays_in_the_picker_with_an_actionable_status() {
        let config_file = temp_file();
        fs::write(&config_file, "not a directory").unwrap();
        let mut app = App::new_with_context(
            palettes::load("themes", None).unwrap(),
            None,
            LayoutOptions::default(),
            Some(config_file.clone()),
        );

        app.activate_selected();

        assert_eq!(app.palette.name, "themes");
        assert!(app.status.unwrap().contains("Could not save theme"));
        assert!(!app.should_quit);
        fs::remove_file(config_file).unwrap();
    }

    #[test]
    fn typing_filters_and_ranks_commands_by_alias() {
        let mut app = App::new(palettes::load("commands", None).unwrap(), None);

        app.handle_key(press(KeyCode::Char('n')));
        app.handle_key(press(KeyCode::Char('s')));

        let visible = app.visible_indices();
        assert_eq!(app.filter, "ns");
        assert_eq!(app.filter_cursor, 2);
        assert_eq!(app.palette.items[visible[0]].title, "New Session");
        assert_eq!(app.palette.items[visible[1]].title, "Next Session");
        assert_eq!(app.selected, Some(visible[0]));
        assert!(app.rows().iter().all(|row| matches!(row, Row::Item(_))));
    }

    #[test]
    fn printable_q_filters_instead_of_quitting() {
        let mut app = App::new(test_palette(vec![test_item("Quit")]), None);

        app.handle_key(press(KeyCode::Char('q')));

        assert_eq!(app.filter, "q");
        assert!(!app.should_quit);
    }

    #[test]
    fn search_edits_are_utf8_safe() {
        let mut app = App::new(test_palette(vec![test_item("Résumé")]), None);
        app.insert_text("résumé");

        app.handle_key(press(KeyCode::Left));
        app.handle_key(press(KeyCode::Backspace));
        app.handle_key(press(KeyCode::Delete));

        assert_eq!(app.filter, "résu");
        assert_eq!(app.filter_cursor, 4);
    }

    #[test]
    fn word_navigation_and_deletion_use_character_indices() {
        let mut app = App::new(test_palette(vec![test_item("Split Pane")]), None);
        app.insert_text("split pane");

        app.handle_key(control(KeyCode::Left));
        assert_eq!(app.filter_cursor, 6);
        app.handle_key(control(KeyCode::Char('w')));

        assert_eq!(app.filter, "pane");
        assert_eq!(app.filter_cursor, 0);
    }

    #[test]
    fn no_search_results_clear_selection_and_do_not_activate() {
        let mut app = App::new(test_palette(vec![test_item("Run")]), None);
        app.insert_text("zzzz");

        app.activate_selected();

        assert!(app.visible_indices().is_empty());
        assert_eq!(app.selected, None);
        assert_eq!(app.status.as_deref(), Some("No results"));
        assert!(!app.should_quit);
    }

    #[test]
    fn rendering_shows_commands_and_selection() {
        let mut terminal = Terminal::new(TestBackend::new(50, 12)).unwrap();
        let mut app = App::new(
            test_palette(vec![
                test_item("First").category("Group"),
                test_item("Second").category("Group"),
            ]),
            None,
        );

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let text = buffer_text(&terminal);

        assert!(text.contains("Test"));
        assert!(text.contains("Group"));
        assert!(text.contains("First"));
        assert!(text.contains("Second"));
        assert!(text.contains("2 commands"));
    }

    #[test]
    fn rendering_live_previews_the_selected_theme_and_its_accent_dot() {
        let mut terminal = Terminal::new(TestBackend::new(50, 12)).unwrap();
        let mut app = App::new(palettes::load("themes", None).unwrap(), None);

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(app.palette.items[0].title, "Ayu Dark");
        assert_eq!(buffer[(0, 0)].bg, Color::Rgb(36, 46, 65));
        assert_eq!(buffer[(5, 4)].fg, Color::Rgb(83, 189, 250));
        assert_eq!(buffer[(0, 4)].bg, Color::Rgb(63, 80, 114));
    }

    #[test]
    fn rendering_shows_find_pane_hierarchy_markers_agent_and_indices() {
        let mut session = Item::new("work", Action::None);
        session.selectable = false;
        session.data = ItemData::FindPane(Box::new(FindPaneRow::Session {
            session: "work".to_owned(),
            count: 2,
            path: "/project".to_owned(),
            is_current: true,
        }));
        let mut window = Item::new("editor", Action::None);
        window.selectable = false;
        window.data = ItemData::FindPane(Box::new(FindPaneRow::Window {
            session: "work".to_owned(),
            window_index: "0".to_owned(),
            tree_prefix: "  └─ ".to_owned(),
        }));
        let mut pane = Item::new("task", Action::None);
        pane.data = ItemData::FindPane(Box::new(FindPaneRow::Pane {
            session: "work".to_owned(),
            window_index: "0".to_owned(),
            pane_index: "1".to_owned(),
            window_name: "editor".to_owned(),
            tree_prefix: "      └─ ".to_owned(),
            command: "codex".to_owned(),
            path: "/project".to_owned(),
            target: "work:0.1".to_owned(),
            agent: "codex".to_owned(),
            pane_active: true,
            is_current: true,
        }));
        let mut palette = Palette::new("find-pane", "Find Pane", vec![session, window, pane]);
        palette.grouped = false;
        palette.filter = PaletteFilter::FindPaneTree;
        palette.initial_selected = Some(2);
        let mut app = App::new(palette, None);
        let mut terminal = Terminal::new(TestBackend::new(50, 12)).unwrap();

        terminal.draw(|frame| render(frame, &mut app)).unwrap();

        assert!(buffer_row(&terminal, 4).contains("▶ work (2)  /project"));
        assert!(buffer_row(&terminal, 5).contains("└─ editor"));
        assert!(buffer_row(&terminal, 6).contains("└─ ▶ task  codex"));
        assert!(buffer_row(&terminal, 6).ends_with("0.1   "));
        assert_eq!(
            terminal.backend().buffer()[(0, 6)].bg,
            ratatui::style::Color::Rgb(80, 77, 122)
        );
    }

    #[test]
    fn rendering_shows_the_query_and_omits_categories_while_filtering() {
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        let mut app = App::new(
            test_palette(vec![
                test_item("First").category("Group"),
                test_item("Second").category("Group"),
            ]),
            None,
        );
        app.insert_text("fi");

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let text = buffer_text(&terminal);
        let buffer = terminal.backend().buffer();

        assert!(text.contains("fi"));
        assert!(text.contains("First"));
        assert!(!text.contains("Group"));
        assert!(!text.contains("Second"));
        assert_eq!(buffer[(5, 2)].symbol(), "f");
        assert_eq!(buffer[(0, 4)].bg, ratatui::style::Color::Rgb(80, 77, 122));
    }

    #[test]
    fn search_window_keeps_a_long_query_cursor_visible() {
        assert_eq!(search_window("abcdefgh", 8, 4), ("fgh".to_owned(), 3));
        assert_eq!(search_window("éclair", 2, 4), ("écla".to_owned(), 2));
        assert_eq!(search_window("anything", 4, 0), (String::new(), 0));
    }

    #[test]
    fn width_helpers_handle_wide_combining_and_joined_text() {
        let family = "👨‍👩‍👧‍👦";

        assert_eq!(display_width("界"), 2);
        assert_eq!(display_width("e\u{301}"), 1);
        assert_eq!(display_width(family), 2);
        assert_eq!(truncate_width("a界b", 2), "a");
        assert_eq!(truncate_width("a界b", 3), "a界");
        assert_eq!(truncate_width("e\u{301}x", 1), "e\u{301}");
        assert_eq!(truncate_width(family, 2), family);
        assert_eq!(truncate_width("界", 1), "");
    }

    #[test]
    fn search_window_uses_terminal_cells_for_content_and_cursor() {
        assert_eq!(search_window("界abc", 1, 4), ("界ab".to_owned(), 2));
        assert_eq!(search_window("a界bc", 4, 4), ("bc".to_owned(), 2));
        assert_eq!(
            search_window("e\u{301}abc", 2, 3),
            ("e\u{301}ab".to_owned(), 1)
        );
    }

    #[test]
    fn rendering_positions_wide_header_shortcut_and_search_cursor_by_cells() {
        let mut terminal = Terminal::new(TestBackend::new(16, 10)).unwrap();
        let mut item = test_item("界a Run");
        item.shortcut = Some("界".to_owned());
        let mut palette = test_palette(vec![item]);
        palette.title = "界面".to_owned();
        palette.grouped = false;
        let mut app = App::new(palette, None);
        app.insert_text("界a");

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(10, 1)].symbol(), "e");
        assert_eq!(buffer[(11, 1)].symbol(), "s");
        assert_eq!(buffer[(12, 1)].symbol(), "c");
        assert_eq!(buffer[(11, 4)].symbol(), "界");
        assert_eq!(buffer[(8, 2)].symbol(), " ");
        terminal.backend_mut().assert_cursor_position((8, 2));
    }

    #[test]
    fn rendering_applies_the_default_bundled_theme() {
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        let mut app = App::new(
            test_palette(vec![test_item("First").category("Group")]),
            None,
        );

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Rgb(45, 43, 85));
        assert_eq!(buffer[(3, 1)].fg, ratatui::style::Color::Rgb(255, 255, 255));
        assert_eq!(buffer[(3, 4)].fg, ratatui::style::Color::Rgb(250, 208, 0));
        assert_eq!(buffer[(0, 5)].bg, ratatui::style::Color::Rgb(80, 77, 122));
        assert_eq!(buffer[(3, 5)].fg, ratatui::style::Color::Rgb(250, 208, 0));
        assert_eq!(buffer[(8, 5)].fg, ratatui::style::Color::Rgb(255, 255, 255));
    }

    #[test]
    fn rendering_matches_padding_alias_description_and_shortcut_layout() {
        let mut terminal = Terminal::new(TestBackend::new(50, 12)).unwrap();
        let mut item = test_item("Run").category("Tools").description("tool");
        item.aliases.push("r".to_owned());
        item.shortcut = Some("C-r".to_owned());
        let mut app = App::new(test_palette(vec![item]), None);

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let row = buffer_row(&terminal, 5);

        assert!(buffer_row(&terminal, 1).starts_with("   Test"));
        assert!(row.contains("Run   r  - tool"));
        assert_eq!(buffer[(13, 5)].bg, ratatui::style::Color::Rgb(30, 29, 64));
        assert_eq!(buffer[(44, 5)].symbol(), "C");
        assert_eq!(buffer[(45, 5)].symbol(), "-");
        assert_eq!(buffer[(46, 5)].symbol(), "r");
        assert_eq!(buffer[(44, 5)].fg, ratatui::style::Color::Rgb(250, 208, 0));
        assert_eq!(buffer[(47, 5)].bg, ratatui::style::Color::Rgb(80, 77, 122));
        assert!(!buffer[(3, 5)].modifier.contains(Modifier::BOLD));
        assert!(buffer[(8, 5)].modifier.contains(Modifier::BOLD));
        assert!(!buffer[(17, 5)].modifier.contains(Modifier::BOLD));
        assert!(!buffer[(44, 5)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn rendering_repeats_the_empty_state_in_the_list_and_footer() {
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        let mut app = App::new(test_palette(vec![test_item("Run")]), None);
        app.insert_text("zzzz");

        terminal.draw(|frame| render(frame, &mut app)).unwrap();

        assert!(buffer_row(&terminal, 4).starts_with("   No results"));
        assert!(buffer_row(&terminal, 10).starts_with("   No results"));
    }

    #[test]
    fn bordered_layout_omits_outer_padding_rows() {
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        let mut palette = test_palette(vec![test_item("Run")]);
        palette.grouped = false;
        let mut app = App::new_with_layout(
            palette,
            None,
            LayoutOptions {
                pad_x: 1,
                bordered: true,
            },
        );

        terminal.draw(|frame| render(frame, &mut app)).unwrap();

        assert!(buffer_row(&terminal, 0).starts_with(" Test"));
        assert!(buffer_row(&terminal, 1).starts_with(" ▌ Search"));
        assert!(buffer_row(&terminal, 2).trim().is_empty());
        assert!(buffer_row(&terminal, 3).contains("Run"));
        assert!(buffer_row(&terminal, 9).starts_with(" enter select"));
    }

    #[test]
    fn narrow_layout_uses_mobile_padding_and_full_frame_rows() {
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        let mut palette = test_palette(vec![test_item("Run")]);
        palette.grouped = false;
        let mut app = App::new_with_layout(
            palette,
            None,
            LayoutOptions {
                pad_x: 1,
                bordered: false,
            },
        );

        terminal.draw(|frame| render(frame, &mut app)).unwrap();

        assert!(buffer_row(&terminal, 0).trim().is_empty());
        assert!(buffer_row(&terminal, 1).starts_with(" Test"));
        assert!(buffer_row(&terminal, 1).ends_with("esc "));
        assert!(buffer_row(&terminal, 2).starts_with(" ▌ Search"));
        assert!(buffer_row(&terminal, 4).contains("Run"));
        assert!(buffer_row(&terminal, 10).starts_with(" enter select"));
        assert!(buffer_row(&terminal, 11).trim().is_empty());
        terminal.backend_mut().assert_cursor_position((3, 2));
    }

    #[test]
    fn bordered_content_height_replaces_the_two_outer_padding_rows() {
        let unbordered = list_rect(
            Rect::new(0, 0, 60, 12),
            LayoutOptions {
                pad_x: 1,
                bordered: false,
            },
        )
        .unwrap();
        let bordered = list_rect(
            Rect::new(0, 0, 58, 10),
            LayoutOptions {
                pad_x: 1,
                bordered: true,
            },
        )
        .unwrap();

        assert_eq!(unbordered, Rect::new(0, 4, 60, 5));
        assert_eq!(bordered, Rect::new(0, 3, 58, 5));
    }

    #[test]
    fn rendering_tiny_terminals_does_not_panic() {
        for (width, height) in [(1, 1), (2, 2), (1, 3)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(test_palette(vec![test_item("Run")]), None);

            terminal.draw(|frame| render(frame, &mut app)).unwrap();
        }
    }

    #[test]
    fn formats_palette_names_for_messages() {
        assert_eq!(display_palette_name("find-pane"), "Find Pane");
        assert_eq!(display_palette_name("commands"), "Commands");
    }
}
