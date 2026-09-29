//! Modeless editing with cached, stateful syntax highlighting.
use super::{ACCENT, BG, INK, MUTED, PANEL};
use crate::model::Language;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use edtui::{
    EditorEventHandler, EditorMode, EditorState, EditorTheme, EditorView, Highlight, Index2,
    LineNumbers, Lines, RowIndex, actions::SwitchMode,
};
use ratatui::{prelude::*, widgets::Block};
use std::sync::LazyLock;
use syntect::{
    easy::HighlightLines,
    highlighting::{
        Color as SyntaxColor, FontStyle, StyleModifier, Theme, ThemeItem, ThemeSettings,
    },
    parsing::SyntaxSet,
};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let gray = |value| SyntaxColor {
        r: value,
        g: value,
        b: value,
        a: 255,
    };
    // Deliberate scope styles retain syntax distinctions without relying on hue.
    let rules = [
        ("punctuation", 176, FontStyle::empty()),
        ("string", 192, FontStyle::empty()),
        ("constant, support.constant", 240, FontStyle::BOLD),
        (
            "entity.name.function, entity.name.type, support.function, support.type",
            240,
            FontStyle::BOLD,
        ),
        ("variable.parameter", 224, FontStyle::ITALIC),
        ("keyword, storage", 255, FontStyle::BOLD),
        ("keyword.operator", 208, FontStyle::empty()),
        (
            "comment, punctuation.definition.comment",
            152,
            FontStyle::ITALIC,
        ),
    ];
    Theme {
        name: Some("Spar grayscale".into()),
        settings: ThemeSettings {
            foreground: Some(gray(224)),
            background: Some(gray(0)),
            ..ThemeSettings::default()
        },
        scopes: rules
            .into_iter()
            .map(|(scope, brightness, font_style)| ThemeItem {
                scope: scope.parse().expect("valid bundled syntax scope"),
                style: StyleModifier {
                    foreground: Some(gray(brightness)),
                    font_style: Some(font_style),
                    ..StyleModifier::default()
                },
            })
            .collect(),
        ..Theme::default()
    }
});
const MAX_SOURCE: usize = 32_000;

#[derive(Clone)]
struct Snapshot {
    source: String,
    cursor: Index2,
}

pub(super) struct CodeEditor {
    state: EditorState,
    handler: EditorEventHandler,
    source: String,
    saved: String,
    language: Language,
    syntax: Vec<Highlight>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    anchor: Option<Index2>,
    pub limit_reached: bool,
    placeholder: Option<&'static str>,
}

impl CodeEditor {
    pub fn new(source: &str, language: Language) -> Self {
        let source = source.strip_suffix('\n').unwrap_or(source).to_string();
        let mut state = EditorState::new(Lines::from(source.as_str()));
        state.mode = EditorMode::Insert;
        let mut editor = Self {
            state,
            handler: EditorEventHandler::emacs_mode(),
            saved: source.clone(),
            source,
            language,
            syntax: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            anchor: None,
            limit_reached: false,
            placeholder: None,
        };
        editor.highlight();
        editor
    }

    pub fn lines(&self) -> Vec<String> {
        self.source.split('\n').map(str::to_owned).collect()
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.state.cursor.row, self.state.cursor.col)
    }

    pub fn modified(&self) -> bool {
        self.source != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.saved.clone_from(&self.source);
    }
    pub fn set_placeholder(&mut self, text: &'static str) {
        self.placeholder = Some(text);
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            source: self.source.clone(),
            cursor: self.state.cursor,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.source = snapshot.source;
        self.state.lines = Lines::from(self.source.as_str());
        self.state.cursor = snapshot.cursor;
        self.clear_selection();
        self.highlight();
    }

    pub fn clear_selection(&mut self) {
        self.state.selection = None;
        self.state.mode = EditorMode::Insert;
        self.anchor = None;
    }

    pub fn selected_text(&self) -> Option<String> {
        let selection = self.state.selection.as_ref()?;
        let start = self.byte_at(selection.start());
        let end = self.byte_at(selection.end());
        let end = end + self.source[end..].chars().next().map_or(0, char::len_utf8);
        Some(self.source[start..end].to_string())
    }

    fn commit(&mut self, before: Snapshot) -> bool {
        let source = self
            .state
            .lines
            .iter_row()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        self.limit_reached = source.len() > MAX_SOURCE;
        if self.limit_reached {
            self.restore(before);
            return false;
        }
        if source == before.source {
            return false;
        }
        self.undo.push(before);
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.source = source;
        self.highlight();
        true
    }

    fn select(&mut self, start: Index2, end: Index2) {
        self.state.execute(SwitchMode(EditorMode::Visual));
        if let Some(selection) = &mut self.state.selection {
            selection.start = start;
            selection.end = end;
        }
        self.state.mode = EditorMode::Insert;
    }

    fn byte_at(&self, index: Index2) -> usize {
        let mut offset = 0;
        for (row, line) in self.source.split('\n').enumerate() {
            if row == index.row {
                return offset
                    + line
                        .char_indices()
                        .nth(index.col)
                        .map_or(line.len(), |(i, _)| i);
            }
            offset += line.len() + 1;
        }
        self.source.len()
    }

    fn index_at(&self, byte: usize) -> Index2 {
        let prefix = &self.source[..byte];
        Index2::new(
            prefix.matches('\n').count(),
            prefix.rsplit('\n').next().unwrap_or("").chars().count(),
        )
    }

    /// Insert plain text at the insertion point; selection ranges in EdTUI are inclusive.
    fn replace_selection(&mut self, text: &str) {
        let (start, end) = if let Some(selection) = &self.state.selection {
            let start = self.byte_at(selection.start());
            let end = self.byte_at(selection.end());
            let end = end + self.source[end..].chars().next().map_or(0, char::len_utf8);
            (start, end)
        } else {
            let pos = self.byte_at(self.state.cursor);
            (pos, pos)
        };
        let mut updated = self.source.clone();
        updated.replace_range(start..end, text);
        let prefix = &updated[..start + text.len()];
        let cursor = Index2::new(
            prefix.matches('\n').count(),
            prefix.rsplit('\n').next().unwrap_or("").chars().count(),
        );
        self.state.lines = Lines::from(updated.as_str());
        self.state.cursor = cursor;
        self.clear_selection();
    }

    pub fn insert_str(&mut self, text: impl AsRef<str>) -> bool {
        let before = self.snapshot();
        let text = text.as_ref().replace("\r\n", "\n").replace('\r', "\n");
        self.replace_selection(&text);
        self.commit(before)
    }

    pub fn input(&mut self, key: KeyEvent) -> bool {
        self.limit_reached = false;
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && matches!(key.code, KeyCode::Char('z' | 'y')) {
            let redo =
                key.code == KeyCode::Char('y') || key.modifiers.contains(KeyModifiers::SHIFT);
            let snapshot = if redo {
                self.redo.pop()
            } else {
                self.undo.pop()
            };
            if let Some(snapshot) = snapshot {
                let current = self.snapshot();
                if redo {
                    self.undo.push(current);
                } else {
                    self.redo.push(current);
                }
                self.restore(snapshot);
                return true;
            }
            return false;
        }
        if control && key.code == KeyCode::Char('a') {
            if !self.source.is_empty() {
                let end = self.source.char_indices().last().map_or(0, |(i, _)| i);
                self.select(Index2::new(0, 0), self.index_at(end));
            }
            return false;
        }
        let navigation = matches!(
            key.code,
            KeyCode::Left
                | KeyCode::Right
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
        );
        if navigation {
            let selecting = key.modifiers.contains(KeyModifiers::SHIFT);
            let anchor = *self.anchor.get_or_insert(self.state.cursor);
            self.state.mode = EditorMode::Insert;
            if control && key.code == KeyCode::Home {
                self.state.cursor = Index2::new(0, 0);
            } else if control && key.code == KeyCode::End {
                self.state.cursor = self.index_at(self.source.len());
            } else {
                self.handler.on_key_event(
                    KeyEvent::new(key.code, key.modifiers - KeyModifiers::SHIFT),
                    &mut self.state,
                );
            }
            if selecting {
                let (a, b) = if anchor <= self.state.cursor {
                    (anchor, self.state.cursor)
                } else {
                    (self.state.cursor, anchor)
                };
                let end_byte = self.byte_at(b);
                if a == b {
                    self.state.selection = None;
                } else {
                    let prev = self.source[..end_byte]
                        .char_indices()
                        .last()
                        .map_or(0, |(i, _)| i);
                    self.select(a, self.index_at(prev));
                }
            } else {
                self.clear_selection();
            }
            return false;
        }
        if key.code == KeyCode::Tab {
            let width = self.indent_width();
            return self.insert_str(" ".repeat(width - self.state.cursor.col % width));
        }
        if key.code == KeyCode::Enter {
            let line = self
                .source
                .split('\n')
                .nth(self.state.cursor.row)
                .unwrap_or("");
            let prefix: String = line.chars().take(self.state.cursor.col).collect();
            let indent: String = prefix
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let extra = if prefix.trim_end().ends_with([':', '{', '[', '(']) {
                " ".repeat(self.indent_width())
            } else {
                String::new()
            };
            return self.insert_str(format!("\n{indent}{extra}"));
        }
        if !control
            && !key.modifiers.contains(KeyModifiers::ALT)
            && let KeyCode::Char(c) = key.code
        {
            return self.insert_str(c.to_string());
        }
        if matches!(key.code, KeyCode::Backspace | KeyCode::Delete)
            && self.state.selection.is_some()
        {
            return self.insert_str("");
        }
        // The app owns save, find, quit and undo. Ignore other control chords rather than
        // exposing modal or Emacs-only mutations with surprising meanings.
        if !matches!(key.code, KeyCode::Backspace | KeyCode::Delete) {
            return false;
        }
        let before = self.snapshot();
        self.clear_selection();
        self.handler.on_key_event(key, &mut self.state);
        self.commit(before)
    }

    pub fn indent_width(&self) -> usize {
        if self.language == Language::Python {
            4
        } else {
            2
        }
    }

    pub fn scroll(&mut self, amount: (i16, i16)) {
        let (x, y) = self.state.viewport_offset();
        let row = y
            .saturating_add_signed(amount.0 as isize)
            .min(self.state.lines.len().saturating_sub(1));
        self.state.set_viewport_offset(x, row);
        self.state.cursor.row = row;
        self.state.cursor.col = self
            .state
            .cursor
            .col
            .min(self.state.lines.get(RowIndex::new(row)).map_or(0, Vec::len));
        self.clear_selection();
    }

    pub fn mouse(&mut self, event: MouseEvent) {
        self.handler.on_mouse_event(event, &mut self.state);
        self.state.mode = EditorMode::Insert;
        self.anchor = None;
    }

    pub fn find(&mut self, query: &str) -> bool {
        if query.is_empty() {
            return false;
        }
        let cursor = self.byte_at(self.state.cursor);
        let after = cursor
            + self.source[cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
        let found = self.source[after..]
            .find(query)
            .map(|i| i + after)
            .or_else(|| self.source[..after].find(query));
        if let Some(start) = found {
            self.clear_selection();
            let end = start + query.char_indices().last().map_or(0, |(i, _)| i);
            self.state.cursor = self.index_at(start);
            self.select(self.state.cursor, self.index_at(end));
            return true;
        }
        false
    }

    fn highlight(&mut self) {
        let extension = if self.language == Language::Python {
            "py"
        } else {
            "ts"
        };
        let syntax = SYNTAXES
            .find_syntax_by_extension(extension)
            .unwrap_or_else(|| SYNTAXES.find_syntax_plain_text());
        let mut parser = HighlightLines::new(syntax, &THEME);
        self.syntax.clear();
        // Keep parser state across lines: triple-quoted strings, block comments and
        // template literals must retain their scopes. Only rebuild after source changes.
        for (row, line) in self.source.split('\n').enumerate() {
            if let Ok(tokens) = parser.highlight_line(&format!("{line}\n"), &SYNTAXES) {
                let mut col = 0;
                for (style, token) in tokens {
                    let len = token.trim_end_matches('\n').chars().count();
                    if len > 0 {
                        let mut modifiers = Modifier::empty();
                        for (syntax, terminal) in [
                            (FontStyle::BOLD, Modifier::BOLD),
                            (FontStyle::ITALIC, Modifier::ITALIC),
                            (FontStyle::UNDERLINE, Modifier::UNDERLINED),
                        ] {
                            if style.font_style.contains(syntax) {
                                modifiers.insert(terminal);
                            }
                        }
                        self.syntax.push(Highlight::new(
                            Index2::new(row, col),
                            Index2::new(row, col + len - 1),
                            Style::default()
                                .fg(Color::Rgb(
                                    style.foreground.r,
                                    style.foreground.g,
                                    style.foreground.b,
                                ))
                                .bg(BG)
                                .add_modifier(modifiers),
                        ));
                        col += len;
                    }
                }
            }
        }
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        block: Block<'static>,
        active: bool,
        readonly: bool,
    ) {
        self.state.highlights.clone_from(&self.syntax);
        let (row, col) = self.cursor();
        let label = if readonly {
            "read-only"
        } else if self.modified() {
            "modified"
        } else {
            "saved"
        };
        let block = block.title_bottom(
            Line::styled(
                format!(" {label} · {}:{} ", row + 1, col + 1),
                Style::default().fg(MUTED),
            )
            .right_aligned(),
        );
        let theme = EditorTheme::default()
            .base(Style::default().fg(INK).bg(PANEL))
            .block(block)
            .hide_status_line()
            .line_numbers_style(Style::default().fg(MUTED).bg(PANEL))
            .selection_style(Style::default().fg(BG).bg(Color::Rgb(208, 208, 208)))
            .cursor_style(if active && !readonly {
                Style::default().fg(BG).bg(ACCENT)
            } else {
                Style::default()
            });
        let width = self.indent_width();
        frame.render_widget(
            EditorView::new(&mut self.state)
                .theme(theme)
                .line_numbers(LineNumbers::Absolute)
                .wrap(true)
                .tab_width(width),
            area,
        );
        if self.source.is_empty()
            && let Some(placeholder) = self.placeholder
        {
            let hint = Rect::new(
                area.x + 3,
                area.y + 1,
                area.width.saturating_sub(4),
                area.height.saturating_sub(2),
            );
            frame.render_widget(
                ratatui::widgets::Paragraph::new(placeholder)
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .style(Style::default().fg(MUTED).italic()),
                hint,
            );
        }
    }
}
