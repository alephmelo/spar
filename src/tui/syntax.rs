//! Shared grayscale syntax styles for editors and read-only documents.
use super::{BG, INK};
use crate::model::Language;
use ratatui::prelude::*;
use std::sync::LazyLock;
use syntect::{
    easy::HighlightLines,
    highlighting::{
        Color as SyntaxColor, FontStyle, StyleModifier, Theme, ThemeItem, ThemeSettings,
    },
    parsing::SyntaxSet,
};

pub(super) static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
pub(super) static THEME: LazyLock<Theme> = LazyLock::new(|| {
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

pub(super) fn extension(language: Language) -> &'static str {
    match language {
        Language::Python => "py",
        Language::Typescript => "ts",
    }
}

pub(super) fn terminal_style(style: syntect::highlighting::Style) -> Style {
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
    Style::default()
        .fg(Color::Rgb(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        ))
        .bg(BG)
        .add_modifier(modifiers)
}

pub(super) fn highlight(source: &str, extension: &str) -> Vec<Line<'static>> {
    let syntax = SYNTAXES
        .find_syntax_by_extension(extension)
        .unwrap_or_else(|| SYNTAXES.find_syntax_plain_text());
    let mut parser = HighlightLines::new(syntax, &THEME);
    source
        .split('\n')
        .map(|line| {
            // Preserve parser state for multiline strings, comments and templates.
            let terminated = format!("{line}\n");
            match parser.highlight_line(&terminated, &SYNTAXES) {
                Ok(tokens) => Line::from(
                    tokens
                        .into_iter()
                        .filter_map(|(style, token)| {
                            let token = token.trim_end_matches('\n');
                            (!token.is_empty())
                                .then(|| Span::styled(token.to_owned(), terminal_style(style)))
                        })
                        .collect::<Vec<_>>(),
                ),
                Err(_) => Line::styled(line.to_owned(), Style::default().fg(INK).bg(BG)),
            }
        })
        .collect()
}
