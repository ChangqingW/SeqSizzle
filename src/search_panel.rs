use crate::app::SearchPattern;
use ratatui::prelude::{
    Alignment, Buffer, Color, Constraint, Direction, Layout, Line, Modifier, Rect, Span, Style,
    Stylize,
};
use ratatui::widgets::{
    block::title::{Position, Title},
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap,
};
use std::rc::Rc;
use std::collections::HashMap;
use tui_textarea::{CursorMove, TextArea};

const ACTIVE_BOARDER_STYLE: Style = Style::new().red().bold();

fn search_patterns_to_list<'a>(search_patterns: &[SearchPattern]) -> List<'a> {
    List::new(
        search_patterns
            .iter()
            .map(|x| {
                ListItem::new(Line::from(vec![
                    Span::styled(x.search_string.clone(), Style::new().fg(x.color)),
                    Span::from(if x.comment.is_some() {
                        format!(" ({}), ", x.comment.as_ref().unwrap())
                    } else {
                        String::from(", ")
                    }),
                    Span::styled(x.color.to_string(), Style::new().fg(x.color)),
                    Span::from(format!(", edit-distance: {}", x.edit_distance)),
                ]))
            })
            .collect::<Vec<ListItem>>(),
    )
    .block(
        Block::default()
            .title("Search patterns")
            .borders(Borders::ALL),
    )
    .highlight_style(Style::default().add_modifier(Modifier::BOLD))
    .highlight_symbol("> ")
}

/// List plus a state field
#[derive(Debug, Clone)]
struct StatefulList<'a> {
    state: ListState,
    list: List<'a>,
}
impl<'a> StatefulList<'a> {
    fn new(list: List<'a>) -> Self {
        Self {
            state: ListState::default(),
            list,
        }
    }
    /// Create a new StatefulList from a vector of SearchPattern
    fn from_search_patterns(search_patterns: &[SearchPattern]) -> Self {
        Self::new(search_patterns_to_list(search_patterns))
    }
    /// Update with a vector of SearchPattern, keeping the selected element if possible
    fn update(&mut self, search_patterns: &[SearchPattern]) {
        self.list = search_patterns_to_list(search_patterns);
        if search_patterns.len() > 0
            && self.state.selected().is_some()
            && self.state.selected().unwrap() >= search_patterns.len()
        {
            self.state.select(Some(search_patterns.len() - 1));
        } else if search_patterns.len() == 0 {
            self.state.select(None);
        }
    }

    /// toggle through the list of items
    fn next(&mut self, reverse: bool) {
        let len = self.list.len();
        let i = match self.state.selected() {
            Some(i) => {
                if reverse {
                    (i + len - 1) % len
                } else {
                    (i + 1) % len
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// change to given block and return self
    fn block(mut self, block: Block<'a>) -> Self {
        self.list = self.list.block(block);
        self
    }
}

/// Re-exported List method
impl<'a> Widget for StatefulList<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        StatefulWidget::render(self.list, area, buf, &mut self.state.clone());
    }
}

/// Element that change block appearance when focused
#[derive(Debug, Clone)]
struct FocusableElement<'a, T> {
    element: T,
    focused_block: Block<'a>, // block to render when focused
}

impl<'a> FocusableElement<'a, StatefulList<'a>> {
    fn new_list_elment(list: StatefulList<'a>, focused_block: Block<'a>) -> Self {
        Self {
            element: list,
            focused_block,
        }
    }

    fn update(&mut self, search_patterns: &[SearchPattern]) {
        self.element.update(search_patterns);
    }

    fn next(&mut self, forward: bool) {
        self.element.next(forward);
    }

    /// render the element with the focused block if focused is true
    fn render(&self, area: Rect, buf: &mut Buffer, focused: bool) {
        Widget::render(
            if focused {
                self.element.clone().block(self.focused_block.clone())
            } else {
                self.element.clone()
            },
            area,
            buf,
        );
    }
}

impl<'a> FocusableElement<'a, TextArea<'a>> {
    fn new_textarea_element(textarea: TextArea<'a>, focused_block: Block<'a>) -> Self {
        Self {
            element: textarea,
            focused_block,
        }
    }

    /// render the element with the focused block if focused is true
    fn render(&self, area: Rect, buf: &mut Buffer, focused: bool) {
        if !focused {
            self.element.widget().render(area, buf);
        } else {
            let mut cloned = self.element.clone();
            cloned.set_block(self.focused_block.clone());
            cloned.widget().render(area, buf);
        }
    }

    /// Clears the text area
    fn clear(&mut self) {
        // ffs why is this not a method of TextArea
        self.element.delete_line_by_end();
        self.element.delete_line_by_head();
    }
}

#[derive(Debug, Clone)]
enum PanelElement<'a> {
    ListElement(FocusableElement<'a, StatefulList<'a>>),
    TextAreaElement(FocusableElement<'a, TextArea<'a>>),
}
impl PanelElement<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer, focused: bool) {
        match self {
            PanelElement::ListElement(list) => list.render(area, buf, focused),
            PanelElement::TextAreaElement(textarea) => textarea.render(area, buf, focused),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SearchPanelElement {
    PatternsList,
    InputPattern,
    InputColor,
    InputDistance,
    InputComment,
}

#[derive(Debug, Clone)]
pub struct SearchPanel<'a> {
    // Maybe use a HashMap instead?
    // elements: HashMap<SearchPanelElement, PanelElement<'a>>,
    patterns_list: PanelElement<'a>,  // 0
    input_pattern: PanelElement<'a>,  // 1
    input_color: PanelElement<'a>,    // 2
    input_distance: PanelElement<'a>, // 3
    input_comment: PanelElement<'a>,  // 4
    focused_element: Option<usize>,
    layout: fn(Rect) -> Rc<[Rect]>,
}
const SEARCH_PANEL_LEN: usize = 5;

impl<'a> SearchPanel<'a> {
    /// focus the next element in the given direction (true for forward, false for backward)
    fn focus_next(&mut self, direction: bool) {
        if self.focused_element.is_none() {
            self.focused_element = Some(0);
        } else {
            let mut new_focus = self.focused_element.unwrap();
            if direction {
                new_focus += 1;
            } else {
                new_focus -= 1;
            }
            if new_focus >= SEARCH_PANEL_LEN {
                new_focus = 0;
            } else if new_focus < 0 {
                new_focus = SEARCH_PANEL_LEN - 1;
            }
            self.focused_element = Some(new_focus);
        }
    }

    pub fn cycle_patterns_list(&mut self, reverse: bool) {
        match &mut self.patterns_list {
            PanelElement::ListElement(list) => list.next(reverse),
            _ => panic!("Wrong type of element"),
        }
    }

    pub fn new(search_patterns: &[SearchPattern]) -> Self {
        fn layout(area: Rect) -> Rc<[Rect]> {
            let vert_chunk = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Percentage(80), Constraint::Percentage(20)])
                .split(area);
            let mut chunks: Vec<Rect> = vec![vert_chunk[0]];
            chunks.extend(
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(vec![
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                    ])
                    .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(vert_chunk[0])
                    .iter(),
            );
            Rc::from(chunks)
        }

        let input_pattern = PanelElement::TextAreaElement(FocusableElement::new_textarea_element(
            {
                let mut input = TextArea::default();
                input.set_block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Search string"),
                );
                input
            },
            Block::default()
                .borders(Borders::ALL)
                .border_style(ACTIVE_BOARDER_STYLE)
                .title("Search string"),
        ));
        let input_color = PanelElement::TextAreaElement(FocusableElement::new_textarea_element(
            {
                let mut input = TextArea::default();
                input.set_block(Block::default().borders(Borders::ALL).title("Color"));
                input
            },
            Block::default()
                .borders(Borders::ALL)
                .border_style(ACTIVE_BOARDER_STYLE)
                .title("Color"),
        ));
        let input_distance = PanelElement::TextAreaElement(FocusableElement::new_textarea_element(
            {
                let mut input = TextArea::default();
                input.set_block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Edit distance"),
                );
                input
            },
            Block::default()
                .borders(Borders::ALL)
                .border_style(ACTIVE_BOARDER_STYLE)
                .title("Edit distance"),
        ));
        let input_comment = PanelElement::TextAreaElement(FocusableElement::new_textarea_element(
            {
                let mut input = TextArea::default();
                input.set_block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Comment (optional)"),
                );
                input
            },
            Block::default()
                .borders(Borders::ALL)
                .border_style(ACTIVE_BOARDER_STYLE)
                .title("Comment (optional)"),
        ));

        let patterns_list = PanelElement::ListElement(FocusableElement::new_list_elment(
            StatefulList::from_search_patterns(search_patterns),
            Block::default()
                .borders(Borders::ALL)
                .border_style(ACTIVE_BOARDER_STYLE)
                .title("Search patterns"),
        ));

        Self {
            patterns_list,
            input_pattern,
            input_color,
            input_distance,
            input_comment,
            focused_element: None,
            layout,
        }
    }

    /// update the list of search patterns
    pub fn update(&mut self, search_patterns: &[SearchPattern]) {
        match &mut self.patterns_list {
            PanelElement::ListElement(list) => list.update(search_patterns),
            _ => panic!("Wrong type of element"),
        }
    }

    pub fn clear_inputs(&mut self) {
        for element in [
            &mut self.input_pattern,
            &mut self.input_color,
            &mut self.input_distance,
            &mut self.input_comment,
        ].iter() {
            match element {
                PanelElement::TextAreaElement(textarea) => textarea.clear(),
                _ => panic!("Wrong type of element"),
            }
        }
    }

    pub fn edit_pattern(&mut self, pattern: SearchPattern) {
        self.clear_inputs();
        match &mut self.input_pattern {
            PanelElement::TextAreaElement(textarea) => {
                textarea.element.insert_str(pattern.search_string);
                textarea.element.move_cursor(CursorMove::Bottom);
                textarea.element.move_cursor(CursorMove::End);
            }
            _ => panic!("Wrong type of element"),
        }
        match &mut self.input_color {
            PanelElement::TextAreaElement(textarea) => {
                textarea.element.insert_str(pattern.color.to_string());
                textarea.element.move_cursor(CursorMove::Bottom);
                textarea.element.move_cursor(CursorMove::End);
            }
            _ => panic!("Wrong type of element"),
        }
        match &mut self.input_distance {
            PanelElement::TextAreaElement(textarea) => {
                textarea.element.insert_str(pattern.edit_distance.to_string());
                textarea.element.move_cursor(CursorMove::Bottom);
                textarea.element.move_cursor(CursorMove::End);
            }
            _ => panic!("Wrong type of element"),
        }
        match &mut self.input_comment {
            PanelElement::TextAreaElement(textarea) => {
                if let Some(comment) = &pattern.comment {
                    textarea.element.insert_str(comment);
                    textarea.element.move_cursor(CursorMove::Bottom);
                    textarea.element.move_cursor(CursorMove::End);
                } else {
                    textarea.element.insert_str(String::new());
                }
            }
            _ => panic!("Wrong type of element"),
        }
    }

    pub fn focused_on_patterns_list(&self) -> bool {
        self.focused_element == Some(0)
    }
}

impl Widget for SearchPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let chunks = (self.layout)(area);
        assert_eq!(chunks.len(), SEARCH_PANEL_LEN);
        for (i, element) in [
            self.patterns_list,
            self.input_pattern,
            self.input_color,
            self.input_distance,
            self.input_comment,
        ]
        .iter()
        .enumerate()
        {
            element.render(chunks[i], buf, self.focused_element == Some(i));
        }
    }
}
