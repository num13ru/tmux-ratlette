use std::collections::BTreeSet;
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
const DEFAULT_EMPTY_HEIGHT: u16 = 3;
const DEFAULT_PAD_X: u16 = 3;
const DEFAULT_MOBILE_WIDTH: u16 = 80;
const CHROME_ROWS: u16 = 2;
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

#[derive(Debug)]
struct App {
    palette: Palette,
    selected: Option<usize>,
    scroll: usize,
    status: Option<String>,
    dispatch_path: Option<PathBuf>,
    should_quit: bool,
}

impl App {
    fn new(palette: Palette, dispatch_path: Option<PathBuf>) -> Self {
        let selected = first_selectable(&palette.items);
        Self {
            palette,
            selected,
            scroll: 0,
            status: None,
            dispatch_path,
            should_quit: false,
        }
    }

    fn unsupported(name: &str, dispatch_path: Option<PathBuf>) -> Self {
        let title = display_palette_name(name);
        let mut palette = Palette::new(name, &title, Vec::new());
        palette.grouped = false;
        palette.empty_text = format!("The {title} palette has not been ported to Rust yet");
        let mut app = Self::new(palette, dispatch_path);
        app.status = Some("Esc closes this placeholder".to_owned());
        app
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut last_category: Option<&str> = None;

        for (index, item) in self.palette.items.iter().enumerate() {
            if self.palette.grouped
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
            .palette
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.selectable.then_some(index))
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

    fn select_edge(&mut self, last: bool) {
        self.selected = if last {
            self.palette
                .items
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, item)| item.selectable.then_some(index))
        } else {
            first_selectable(&self.palette.items)
        };
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

    fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Esc | KeyCode::Char('q'), _) => self.should_quit = true,
            (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.move_selection(-1);
            }
            (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.move_selection(1);
            }
            (KeyCode::PageUp, _) => self.move_selection(-PAGE_SIZE),
            (KeyCode::PageDown, _) => self.move_selection(PAGE_SIZE),
            (KeyCode::Home, _) => self.select_edge(false),
            (KeyCode::End, _) => self.select_edge(true),
            (KeyCode::Enter, _) => self.activate_selected(),
            _ => {}
        }
    }
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
        .saturating_add(CHROME_ROWS)
        .clamp(DEFAULT_EMPTY_HEIGHT, DEFAULT_MAX_HEIGHT)
}

fn run_interactive(invocation: &Invocation) -> Result<()> {
    let dispatch_path = std::env::var_os("TMUX_PALETTE_CMD").map(PathBuf::from);
    let mut app = match palettes::load(&invocation.palette) {
        Some(mut palette) => {
            if let Some(category) = invocation.category.as_deref() {
                palette.filter_category(category);
            }
            App::new(palette, dispatch_path)
        }
        None => App::unsupported(&invocation.palette, dispatch_path),
    };
    let mut terminal = TerminalSession::enter(!invocation.no_mouse)?;

    while !app.should_quit {
        terminal.draw(|frame| render(frame, &mut app))?;

        match event::read().map_err(crate::Error::Terminal)? {
            Event::Key(key) => app.handle_key(key),
            Event::Mouse(_) | Event::Resize(_, _) => {}
            Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
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

    let header = Rect::new(area.x, area.y, area.width, 1);
    render_header(frame, header, &app.palette.title, &styles);

    let footer_height = u16::from(area.height >= 2);
    let list_y = area.y.saturating_add(1);
    let list_height = area.height.saturating_sub(1 + footer_height);
    let list = Rect::new(area.x, list_y, area.width, list_height);
    render_list(frame, list, app, &styles);

    if footer_height == 1 {
        let footer = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
        render_footer(frame, footer, app, &styles);
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect, title: &str, styles: &ThemeStyles) {
    let escape = "esc";
    let available = area.width.saturating_sub(escape.len() as u16) as usize;
    let title = truncate_chars(title, available);
    let gap = area
        .width
        .saturating_sub(title.chars().count() as u16)
        .saturating_sub(escape.len() as u16) as usize;
    let line = Line::from(vec![
        Span::styled(title, styles.header),
        Span::styled(" ".repeat(gap), styles.panel),
        Span::styled(escape, styles.muted),
    ]);
    frame.render_widget(Paragraph::new(line).style(styles.panel), area);
}

fn render_list(frame: &mut Frame<'_>, area: Rect, app: &mut App, styles: &ThemeStyles) {
    if area.is_empty() {
        app.scroll = 0;
        return;
    }

    app.ensure_selection_visible(area.height as usize);
    let rows = app.rows();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(truncate_chars(&app.palette.empty_text, area.width as usize))
                .style(styles.muted),
            area,
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
            Row::Category(category) => frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("  ", styles.panel),
                    Span::styled(category.as_str(), styles.category),
                ]))
                .style(styles.panel),
                row_area,
            ),
            Row::Item(index) => render_item(
                frame,
                row_area,
                &app.palette.items[*index],
                app.selected == Some(*index),
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
    let marker = if selected { "▌" } else { " " };
    let icon = item.icon.as_deref().unwrap_or(" ");
    let mut spans = vec![
        Span::styled(marker, accent),
        Span::styled(" ", style),
        Span::styled(icon, accent),
        Span::styled("  ", style),
        Span::styled(item.title.as_str(), style),
    ];
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
    frame.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let (text, style) = match app.status.as_deref() {
        Some(status) => (status.to_owned(), styles.status),
        None => {
            let count = app
                .palette
                .items
                .iter()
                .filter(|item| item.selectable)
                .count();
            let noun = if count == 1 { "command" } else { "commands" };
            (
                format!("enter select   up/down move   {count} {noun}"),
                styles.muted,
            )
        }
    };
    frame.render_widget(
        Paragraph::new(truncate_chars(&text, area.width as usize))
            .style(style)
            .alignment(Alignment::Left),
        area,
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

        assert_eq!(result.rows, 3);
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
        assert_eq!(app.status.as_deref(), Some("No commands"));
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
    fn rendering_shows_commands_and_selection() {
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
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
    fn rendering_applies_the_default_bundled_theme() {
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        let mut app = App::new(
            test_palette(vec![test_item("First").category("Group")]),
            None,
        );

        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Rgb(45, 43, 85));
        assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Rgb(255, 255, 255));
        assert_eq!(buffer[(2, 1)].fg, ratatui::style::Color::Rgb(250, 208, 0));
        assert_eq!(buffer[(0, 2)].bg, ratatui::style::Color::Rgb(80, 77, 122));
        assert_eq!(buffer[(0, 2)].fg, ratatui::style::Color::Rgb(250, 208, 0));
        assert_eq!(buffer[(5, 2)].fg, ratatui::style::Color::Rgb(255, 255, 255));
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
