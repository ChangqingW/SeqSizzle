use std::{io, panic};
use anyhow::Result;
use crossterm::{
  event::{DisableMouseCapture, EnableMouseCapture},
  terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
pub type CrosstermTerminal = ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stderr>>;
use ratatui::{
  prelude::{Constraint, Direction, Frame, Layout, Line},
  widgets::{Block, Borders, Paragraph, Wrap},
};
use ratatui::layout::Rect;
use tui_textarea::TextArea;
use crate::{app::App, event::EventHandler};

/// Representation of a terminal user interface.
///
/// It is responsible for setting up the terminal,
/// initializing the interface and handling the draw events.
pub struct Tui<'a> {
  terminal: CrosstermTerminal,
  pub events: EventHandler,
  view_buffer: Vec<Line<'a>>,
  pub scroll_idx: u16
}

impl Tui<'_> {
  /// Constructs a new instance of [`Tui`].
  pub fn new(terminal: CrosstermTerminal, events: EventHandler) -> Self {
    Self { terminal, events, view_buffer: Vec::new(), scroll_idx: 0}
  }

  pub fn enter(&mut self) -> Result<()> {
    terminal::enable_raw_mode()?;
    crossterm::execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture)?;

    // Define a custom panic hook to reset the terminal properties.
    // This way, you won't have your terminal messed up if an unexpected error happens.
    let panic_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic| {
      Self::reset().expect("failed to reset the terminal");
      panic_hook(panic);
    }));

    self.terminal.hide_cursor()?;
    self.terminal.clear()?;
    Ok(())
  }

  fn update_view_buffer(&mut self, app: &App) {
    self.view_buffer = app.update();
  }

  /// [`Draw`] the terminal interface by [`rendering`] the widgets.
  ///
  /// [`Draw`]: tui::Terminal::draw
  /// [`rendering`]: crate::ui:render
  pub fn draw(&mut self, app: &mut App) -> Result<()> {
    if self.view_buffer.is_empty() {
      self.update_view_buffer(app);
    }
    self.terminal.draw(|frame| render(self.view_buffer.clone(), app, frame, self.scroll_idx))?;
    Ok(())
  }

  fn reset() -> Result<()> {
    terminal::disable_raw_mode()?;
    crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
  }
  pub fn exit(&mut self) -> Result<()> {
    Self::reset()?;
    self.terminal.show_cursor()?;
    Ok(())
  }
  pub fn viewer_size(&self) -> Rect {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(self.terminal.size().unwrap())[0]
  }
}

fn render(view_buffer: Vec<Line>, app: &mut App, frame: &mut Frame, scroll: u16) {
  let layout = Layout::default()
      .direction(Direction::Vertical)
      .constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
      .split(frame.size());

  frame.render_widget(
    Paragraph::new(view_buffer)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0)),
    layout[0],
  );
  let mut textarea = TextArea::default();
  frame.render_widget(textarea.widget(), layout[1]);
}