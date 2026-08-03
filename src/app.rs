use std::num::NonZeroU16;
use std::path::Path;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::Result;
use crate::cli::{Invocation, Mode};
use crate::config::resolve_config_dir;
use crate::terminal::TerminalSession;

const DEFAULT_WIDTH: u16 = 90;
const DEFAULT_HEIGHT: u16 = 7;
const DEFAULT_PAD_X: u16 = 3;
const DEFAULT_MOBILE_WIDTH: u16 = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    pub rows: u16,
    pub width: u16,
    pub pad_x: u16,
    pub border: &'static str,
    pub body_style: &'static str,
    pub border_style: &'static str,
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

pub fn run(invocation: Invocation) -> Result<()> {
    let config_dir = resolve_config_dir(invocation.config_dir.as_deref())?;

    match invocation.mode {
        Mode::Measure => {
            println!(
                "{}",
                measure(invocation.client_width, invocation.client_height)
            );
            Ok(())
        }
        Mode::Interactive => run_interactive(&invocation, &config_dir),
    }
}

pub fn measure(client_width: Option<NonZeroU16>, client_height: Option<NonZeroU16>) -> Measurement {
    let mut measurement = Measurement {
        rows: DEFAULT_HEIGHT,
        width: DEFAULT_WIDTH,
        pad_x: DEFAULT_PAD_X,
        border: "none",
        body_style: "default",
        border_style: "default",
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

fn run_interactive(invocation: &Invocation, config_dir: &Path) -> Result<()> {
    let mut terminal = TerminalSession::enter(!invocation.no_mouse)?;

    loop {
        terminal.draw(|frame| render(frame, invocation, config_dir))?;

        match event::read().map_err(crate::Error::Terminal)? {
            Event::Key(key) if is_exit_key(key) => return Ok(()),
            Event::Key(_) | Event::Mouse(_) | Event::Resize(_, _) => {}
            Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
        }
    }
}

fn is_exit_key(key: KeyEvent) -> bool {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return false;
    }

    matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn render(frame: &mut Frame<'_>, invocation: &Invocation, config_dir: &Path) {
    let area = frame.area();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    let title = format!(" {} ", display_palette_name(&invocation.palette));
    frame.render_widget(
        Line::from(title)
            .style(Style::default().add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center),
        sections[0],
    );

    let body = Paragraph::new(vec![
        Line::from("Rust port bootstrap"),
        Line::from(""),
        Line::from(format!("Config: {}", config_dir.display())),
        Line::from("Palette behavior has not been ported yet."),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::NONE))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, sections[1]);

    frame.render_widget(
        Line::from("Esc / q: close").alignment(Alignment::Center),
        sections[2],
    );
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
    use super::*;

    #[test]
    fn default_measurement_is_shell_compatible() {
        let result = measure(None, None);

        assert_eq!(result.to_string(), "7\t90\t3\tnone\tdefault\tdefault");
    }

    #[test]
    fn narrow_clients_use_full_width_and_height() {
        let result = measure(NonZeroU16::new(60), NonZeroU16::new(30));

        assert_eq!(result.width, 60);
        assert_eq!(result.rows, 30);
        assert_eq!(result.pad_x, 1);
    }

    #[test]
    fn short_mobile_clients_still_fit_the_bootstrap_chrome() {
        let result = measure(NonZeroU16::new(60), NonZeroU16::new(3));

        assert_eq!(result.rows, DEFAULT_HEIGHT);
    }

    #[test]
    fn formats_palette_names_for_the_header() {
        assert_eq!(display_palette_name("find-pane"), "Find Pane");
        assert_eq!(display_palette_name("commands"), "Commands");
    }
}
