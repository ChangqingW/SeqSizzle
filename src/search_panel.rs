use crate::app::SearchPattern;
use crossterm::event::KeyEvent;
use ratatui::layout::Alignment;
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Line, Modifier, Rect, Span, Style};
use ratatui::widgets::block::title::{Position, Title};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget};
use std::collections::BTreeMap;
use std::rc::Rc;
use tui_textarea::{CursorMove, TextArea};

const ACTIVE_BOARDER_STYLE: Style = Style {
    fg: Some(ratatui::prelude::Color::Red),
    bg: None,
    underline_color: None,
    add_modifier: ratatui::style::Modifier::BOLD,
    sub_modifier: ratatui::style::Modifier::BOLD,
};

fn search_patterns_to_list<'a>(search_patterns: &[SearchPattern]) -> List<'a> {
    List::new(
        search_patterns
            .iter()
            .map(|x| {
                ListItem::new(Line::from(vec![
                    Span::styled(x.search_string.clone(), Style::new().fg(x.color)),
                    Span::from(if !x.comment.is_empty() {
                        format!(" ({}), ", x.comment)
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
pub struct StatefulList<'a> {
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
        if !search_patterns.is_empty()
            && self.state.selected().is_some()
            && self.state.selected().unwrap() >= search_patterns.len()
        {
            self.state.select(Some(search_patterns.len() - 1));
        } else if search_patterns.is_empty() {
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

    /// return the selected index
    pub fn selected(&self) -> Option<usize> {
        self.state.selected()
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
pub struct FocusableElement<'a, T> {
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

    /// Re-exported lines() method
    pub fn lines(&self) -> &[String] {
        self.element.lines()
    }
}

#[derive(Debug, Clone)]
pub enum PanelElement<'a> {
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PanelElementName {
    PatternsList,
    InputPattern,
    InputColor,
    InputDistance,
    InputComment,
}
impl PanelElementName {
    fn next(&self, reverse: bool) -> Self {
        if reverse {
            match self {
                PanelElementName::PatternsList => PanelElementName::InputComment,
                PanelElementName::InputPattern => PanelElementName::PatternsList,
                PanelElementName::InputColor => PanelElementName::InputPattern,
                PanelElementName::InputDistance => PanelElementName::InputColor,
                PanelElementName::InputComment => PanelElementName::InputDistance,
            }
        } else {
            match self {
                PanelElementName::PatternsList => PanelElementName::InputPattern,
                PanelElementName::InputPattern => PanelElementName::InputColor,
                PanelElementName::InputColor => PanelElementName::InputDistance,
                PanelElementName::InputDistance => PanelElementName::InputComment,
                PanelElementName::InputComment => PanelElementName::PatternsList,
            }
        }
    }

    /// Title to display
    fn title(&self) -> &'static str {
        match self {
            PanelElementName::PatternsList => {
                "Search patterns (up / down to select patterns, enter / delete to edit or delete)"
            } // Not in use currently
            PanelElementName::InputPattern => "Search String",
            PanelElementName::InputColor => "Color",
            PanelElementName::InputDistance => "Edit distance",
            PanelElementName::InputComment => "Comment (optional)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchPanel<'a> {
    elements: BTreeMap<PanelElementName, PanelElement<'a>>,
    focused_element: PanelElementName, // must have a focused element
    layout: fn(Rect) -> Rc<[Rect]>,
    file_save_popup: TextArea<'a>,
}

impl<'a> SearchPanel<'a> {
    /// focus the next element in the given direction (true for forward, false for backward)
    pub fn focus_next(&mut self, reverse: bool) {
        self.focused_element = self.focused_element.next(reverse);
    }

    /// cycle through the list of patterns selection
    pub fn cycle_patterns_list(&mut self, reverse: bool) {
        match self
            .elements
            .get_mut(&PanelElementName::PatternsList)
            .unwrap()
        {
            PanelElement::ListElement(list) => list.next(reverse),
            _ => panic!("Wrong type of element"),
        }
    }

    /// return the name of the focused element
    pub fn focused_element(&self) -> PanelElementName {
        self.focused_element.clone()
    }

    /// return the selected index of the patterns list
    pub fn selected_pattern(&self) -> Option<usize> {
        match &self.elements[&PanelElementName::PatternsList] {
            PanelElement::ListElement(list) => list.element.selected(),
            _ => panic!("Wrong type of element"),
        }
    }

    /// return a reference to the elements map
    pub fn elements(&self) -> &BTreeMap<PanelElementName, PanelElement<'a>> {
        &self.elements
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
                    .split(vert_chunk[1])
                    .iter(),
            );
            Rc::from(chunks)
        }

        let patterns_list = PanelElement::ListElement(FocusableElement::new_list_elment(
            StatefulList::from_search_patterns(search_patterns),
            Block::default()
                .borders(Borders::ALL)
                .border_style(ACTIVE_BOARDER_STYLE)
                .title("Search patterns"),
        ));

        let mut elements: BTreeMap<PanelElementName, PanelElement> = BTreeMap::new();
        elements.insert(PanelElementName::PatternsList, patterns_list);
        for element in [
            PanelElementName::InputPattern,
            PanelElementName::InputColor,
            PanelElementName::InputDistance,
            PanelElementName::InputComment,
        ]
        .into_iter()
        {
            elements.insert(
                element.clone(),
                PanelElement::TextAreaElement(FocusableElement::new_textarea_element(
                    {
                        let mut input = TextArea::default();
                        input.set_block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(element.title()),
                        );
                        input
                    },
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(ACTIVE_BOARDER_STYLE)
                        .title(element.title()),
                )),
            );
        }

        let mut file_save_popup = TextArea::default();
        file_save_popup.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Save patterns as CSV to ...")
                .title(
                    Title::from("Esc to cancel; Enter to save")
                        .position(Position::Bottom)
                        .alignment(Alignment::Right),
                ),
        );

        Self {
            elements,
            focused_element: PanelElementName::PatternsList,
            layout,
            file_save_popup,
        }
    }

    /// update the list of search patterns
    pub fn update(&mut self, search_patterns: &[SearchPattern]) {
        match self
            .elements
            .get_mut(&PanelElementName::PatternsList)
            .unwrap()
        {
            PanelElement::ListElement(list) => list.update(search_patterns),
            _ => panic!("Wrong type of element"),
        }
    }

    /// Clear all TextArea elements' inptus
    pub fn clear_inputs(&mut self) {
        self.elements.values_mut().for_each(|element| {
            if let PanelElement::TextAreaElement(textarea) = element {
                textarea.clear()
            }
        });
    }

    /// Instert the given pattern to the input fields
    pub fn edit_pattern(&mut self, pattern: SearchPattern) {
        // clear all inputs
        self.clear_inputs();

        // insert values
        for (element_name, element) in self.elements.iter_mut() {
            if let PanelElement::TextAreaElement(textarea) = element {
                match element_name {
                    PanelElementName::InputPattern => {
                        textarea.element.insert_str(pattern.search_string.clone());
                    }
                    PanelElementName::InputColor => {
                        textarea.element.insert_str(pattern.color.to_string());
                    }
                    PanelElementName::InputDistance => {
                        textarea
                            .element
                            .insert_str(pattern.edit_distance.to_string());
                    }
                    PanelElementName::InputComment => {
                        textarea.element.insert_str(pattern.comment.clone());
                    }
                    _ => (),
                }
            }
        }

        // move cursor to end
        for element in self.elements.values_mut() {
            if let PanelElement::TextAreaElement(textarea) = element {
                textarea.element.move_cursor(CursorMove::Bottom);
                textarea.element.move_cursor(CursorMove::End);
            }
        }
    }

    /// pass input to focused element
    pub fn handle_input(&mut self, keyevent: KeyEvent) {
        match self.elements.get_mut(&self.focused_element).unwrap() {
            PanelElement::TextAreaElement(textarea) => {
                textarea.element.input(keyevent);
            }
            // slightly inconsistent, but
            // keep list operations in main.rs
            _ => panic!("Wrong type of element"),
        }
    }

    pub fn file_popup_input(&mut self, keyevent: KeyEvent) {
        self.file_save_popup.input(keyevent);
    }

    /// return the lines from the file save popup
    pub fn file_save_popup_lines(&self) -> &[String] {
        self.file_save_popup.lines()
    }

    /// Re-export widget method for rendering the file save popup
    pub fn file_save_popup_widget(&self) -> impl Widget + '_ {
        self.file_save_popup.widget()
    }
}

impl Widget for &SearchPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let chunks = (self.layout)(area);
        assert_eq!(chunks.len(), self.elements.len());
        for (i, (element_name, element)) in self.elements.iter().enumerate() {
            element.render(chunks[i], buf, *element_name == self.focused_element);
        }
    }
}
