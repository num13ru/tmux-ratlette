use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::num::NonZeroU16;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::cli::{Invocation, Mode};
use crate::config::resolve_config_dir;
use crate::dispatch;
use crate::model::{Action, Item, Palette, Theme, ThemeColor};
use crate::terminal::TerminalSession;
use crate::{Result, palettes, themes};

const DEFAULT_WIDTH: u16 = 90;
const DEFAULT_MAX_HEIGHT: u16 = 28;
const DEFAULT_EMPTY_HEIGHT: u16 = 7;
const DEFAULT_PAD_X: u16 = 3;
const DEFAULT_MOBILE_WIDTH: u16 = 80;
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
    accent: Style,
    selected_accent: Style,
    muted: Style,
    selected_muted: Style,
    status: Style,
    alias: Style,
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
            selected: themed_style(selected_fg, theme.selected).add_modifier(Modifier::BOLD),
            accent: themed_style(theme.accent, theme.panel),
            selected_accent: themed_style(selected_accent, theme.selected),
            muted: themed_style(theme.muted, theme.panel),
            selected_muted: themed_style(theme.muted, theme.selected),
            status: themed_style(theme.accent, theme.panel),
            alias: themed_style(theme.fg, theme.bg),
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
    pub border: &'static str,
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
    fn from_env() -> Self {
        Self::from_values(
            std::env::var_os("TMUX_PALETTE_PADX").as_deref(),
            std::env::var_os("TMUX_PALETTE_BORDERED").as_deref(),
        )
    }

    fn from_values(pad_x: Option<&OsStr>, bordered: Option<&OsStr>) -> Self {
        let pad_x = pad_x
            .and_then(OsStr::to_str)
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PAD_X);
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
    should_quit: bool,
}

impl App {
    #[cfg(test)]
    fn new(palette: Palette, dispatch_path: Option<PathBuf>) -> Self {
        Self::new_with_layout(palette, dispatch_path, LayoutOptions::default())
    }

    fn new_with_layout(
        palette: Palette,
        dispatch_path: Option<PathBuf>,
        layout: LayoutOptions,
    ) -> Self {
        let selected = first_selectable(&palette.items);
        Self {
            palette,
            selected,
            scroll: 0,
            filter: String::new(),
            filter_cursor: 0,
            layout,
            status: None,
            dispatch_path,
            should_quit: false,
        }
    }

    fn unsupported(name: &str, dispatch_path: Option<PathBuf>, layout: LayoutOptions) -> Self {
        let title = display_palette_name(name);
        let mut palette = Palette::new(name, &title, Vec::new());
        palette.grouped = false;
        palette.empty_text = format!("The {title} palette has not been ported to Rust yet");
        let mut app = Self::new_with_layout(palette, dispatch_path, layout);
        app.status = Some("Esc closes this placeholder".to_owned());
        app
    }

    fn visible_indices(&self) -> Vec<usize> {
        crate::fuzzy::default_filter(&self.palette.items, &self.filter)
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut last_category: Option<&str> = None;

        for index in self.visible_indices() {
            let item = &self.palette.items[index];
            if self.palette.grouped
                && self.filter.trim().is_empty()
                && let Some(category) = item.category.as_deref()
                && last_category != Some(category)
            {
                rows.push(Row::Category(category.to_owned()));
                last_category = Some(category);
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
        let next = (current + delta).rem_euclid(selectable.len() as isize) as usize;
        self.selected = Some(selectable[next]);
        self.status = None;
    }

    fn filter_changed(&mut self) {
        self.selected = self
            .visible_indices()
            .into_iter()
            .find(|index| self.palette.items[*index].selectable);
        self.scroll = 0;
        self.status = None;
    }

    fn ensure_selection_visible(&mut self, list_height: usize) {
        let rows = self.rows();
        if list_height == 0 || rows.is_empty() {
            self.scroll = 0;
            return;
        }

        if let Some(selected) = self.selected
            && let Some(selected_row) = rows
                .iter()
                .position(|row| matches!(row, Row::Item(index) if *index == selected))
        {
            if selected_row < self.scroll {
                self.scroll = selected_row;
            } else if selected_row >= self.scroll.saturating_add(list_height) {
                self.scroll = selected_row + 1 - list_height;
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
                self.status = Some(format!(
                    "The {} palette has not been ported to Rust yet",
                    display_palette_name(&name)
                ));
            }
            Action::Popup(_) => {
                self.status = Some("Popup actions have not been ported to Rust yet".to_owned());
            }
            Action::None => {
                self.status = Some("This item has no action".to_owned());
            }
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
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c' | 'C') if control => self.should_quit = true,
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
    let _config_dir = resolve_config_dir(invocation.config_dir.as_deref())?;

    match invocation.mode {
        Mode::Measure => {
            println!(
                "{}",
                measure_palette(
                    &invocation.palette,
                    invocation.category.as_deref(),
                    invocation.client_width,
                    invocation.client_height,
                )
            );
            Ok(())
        }
        Mode::Interactive => run_interactive(&invocation),
    }
}

pub fn measure(client_width: Option<NonZeroU16>, client_height: Option<NonZeroU16>) -> Measurement {
    measure_palette("commands", None, client_width, client_height)
}

fn measure_palette(
    palette_name: &str,
    category: Option<&str>,
    client_width: Option<NonZeroU16>,
    client_height: Option<NonZeroU16>,
) -> Measurement {
    let (rows, theme) = palettes::load(palette_name)
        .map(|mut palette| {
            if let Some(category) = category {
                palette.filter_category(category);
            }
            (desired_height(&palette), palette.theme)
        })
        .unwrap_or((DEFAULT_EMPTY_HEIGHT, themes::default_theme()));
    let mut measurement = Measurement {
        rows,
        width: DEFAULT_WIDTH,
        pad_x: DEFAULT_PAD_X,
        border: "none",
        body_style: theme.tmux_body_style(),
        border_style: theme.tmux_border_style(),
    };

    if let Some(width) = client_width.map(NonZeroU16::get)
        && width < DEFAULT_MOBILE_WIDTH
    {
        measurement.width = width;
        measurement.pad_x = 1;
        if let Some(height) = client_height.map(NonZeroU16::get) {
            measurement.rows = measurement.rows.max(height);
        }
    }

    measurement
}

fn desired_height(palette: &Palette) -> u16 {
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
    let content_rows = palette.items.len().saturating_add(categories).max(1);
    u16::try_from(content_rows)
        .unwrap_or(u16::MAX)
        .saturating_add(UNBORDERED_CHROME_ROWS)
        .clamp(DEFAULT_EMPTY_HEIGHT, DEFAULT_MAX_HEIGHT)
}

fn run_interactive(invocation: &Invocation) -> Result<()> {
    let dispatch_path = std::env::var_os("TMUX_PALETTE_CMD").map(PathBuf::from);
    let layout = LayoutOptions::from_env();
    let mut app = match palettes::load(&invocation.palette) {
        Some(mut palette) => {
            if let Some(category) = invocation.category.as_deref() {
                palette.filter_category(category);
            }
            App::new_with_layout(palette, dispatch_path, layout)
        }
        None => App::unsupported(&invocation.palette, dispatch_path, layout),
    };
    let mut terminal = TerminalSession::enter(!invocation.no_mouse)?;

    while !app.should_quit {
        terminal.draw(|frame| render(frame, &mut app))?;

        match event::read().map_err(crate::Error::Terminal)? {
            Event::Key(key) => app.handle_key(key),
            Event::Mouse(_) | Event::Resize(_, _) => {}
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

    if area.height >= app.layout.chrome_rows() {
        let list_offset = outer_padding.saturating_add(3);
        let list_height = area.height.saturating_sub(app.layout.chrome_rows());
        let list = Rect::new(
            area.x,
            area.y.saturating_add(list_offset),
            area.width,
            list_height,
        );
        render_list(frame, list, app, &styles);

        let footer_offset = area.height.saturating_sub(outer_padding.saturating_add(1));
        if let Some(footer) = row_at(area, footer_offset) {
            render_footer(frame, footer, app, &styles);
        }
    }
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
    let escape = truncate_chars("esc", content.width as usize);
    let available = content.width.saturating_sub(escape.chars().count() as u16) as usize;
    let title = truncate_chars(title, available);
    let gap = content
        .width
        .saturating_sub(title.chars().count() as u16)
        .saturating_sub(escape.chars().count() as u16) as usize;
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
        Span::styled(truncate_chars("Search", text_width), styles.muted)
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
    let characters = value.chars().collect::<Vec<_>>();
    let cursor = cursor.min(characters.len());
    let start = cursor.saturating_sub(width.saturating_sub(1));
    let end = (start + width).min(characters.len());
    (characters[start..end].iter().collect(), cursor - start)
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
            Paragraph::new(truncate_chars(
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
    let style = if selected {
        styles.selected
    } else {
        styles.item
    };
    let accent = if selected {
        styles.selected_accent
    } else {
        styles.accent
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
        Span::styled(item.title.as_str(), style),
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
        let shortcut = truncate_chars(shortcut, content.width.saturating_sub(1) as usize);
        let shortcut_width = u16::try_from(shortcut.chars().count().saturating_add(1))
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
        Paragraph::new(truncate_chars(&text, content.width as usize))
            .style(style)
            .alignment(Alignment::Left),
        content,
    );
}

fn first_selectable(items: &[Item]) -> Option<usize> {
    items.iter().position(|item| item.selectable)
}

fn truncate_chars(value: &str, width: usize) -> String {
    value.chars().take(width).collect()
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
        let result = measure_palette("commands", Some("System"), None, None);

        assert_eq!(result.rows, 8);
    }

    #[test]
    fn narrow_clients_use_full_width_and_height() {
        let result = measure(NonZeroU16::new(60), NonZeroU16::new(30));

        assert_eq!(result.width, 60);
        assert_eq!(result.rows, 30);
        assert_eq!(result.pad_x, 1);
    }

    #[test]
    fn short_mobile_clients_still_fit_the_palette_chrome() {
        let result = measure_palette("missing", None, NonZeroU16::new(60), NonZeroU16::new(2));

        assert_eq!(result.rows, DEFAULT_EMPTY_HEIGHT);
    }

    #[test]
    fn layout_options_validate_environment_values() {
        assert_eq!(
            LayoutOptions::from_values(Some(OsStr::new("5")), Some(OsStr::new("1"))),
            LayoutOptions {
                pad_x: 5,
                bordered: true,
            }
        );
        assert_eq!(
            LayoutOptions::from_values(Some(OsStr::new("-1")), Some(OsStr::new("true"))),
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
    fn direct_runs_do_not_dispatch_without_the_wrapper_file() {
        let mut app = App::new(test_palette(vec![test_item("Run")]), None);

        app.activate_selected();

        assert!(!app.should_quit);
        assert!(app.status.unwrap().contains("launch through"));
    }

    #[test]
    fn nested_palette_actions_report_the_unavailable_feature() {
        let item = Item::new("Themes", Action::palette("themes"));
        let mut app = App::new(test_palette(vec![item]), None);

        app.activate_selected();

        assert!(!app.should_quit);
        assert!(app.status.unwrap().contains("Themes palette"));
    }

    #[test]
    fn typing_filters_and_ranks_commands_by_alias() {
        let mut app = App::new(palettes::load("commands").unwrap(), None);

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
