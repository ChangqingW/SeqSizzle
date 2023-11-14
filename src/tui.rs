use std::{io, panic};
use std::panic::panic_any;
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
pub struct Tui {
  terminal: CrosstermTerminal,
  pub events: EventHandler,
  pub scroll_idx: u16
}

impl Tui {
  /// Constructs a new instance of [`Tui`].
  pub fn new(terminal: CrosstermTerminal, events: EventHandler) -> Self {
    Self { terminal, events, scroll_idx: 0}
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

  /// [`Draw`] the terminal interface by [`rendering`] the widgets.
  ///
  /// [`Draw`]: tui::Terminal::draw
  /// [`rendering`]: crate::ui:render
  pub fn draw(&mut self, app: &mut App) -> Result<()> {
    if app.line_buf.is_empty() {
      panic!("No lines in app.line_buf!\n{:?}", app)
    }
    self.terminal.draw(|frame| render(app.line_buf.clone(), app, frame, self.scroll_idx))?;
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