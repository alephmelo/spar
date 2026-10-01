//! Read-only brief formatting; cached independently of the learner's editors.
use super::{ACCENT, INFO, MUTED, clean_output, syntax};
use crate::model::Language;
use ratatui::prelude::*;
use serde_json::Value;

#[derive(Default)]
pub(super) struct Cache {
    key: Option<(String, Language, u16)>,
    text: Text<'static>,
}

impl Cache {
    pub fn render(&mut self, source: String, language: Language, width: u16) -> Text<'static> {
        let key = (source, language, width);
        if self.key.as_ref() != Some(&key) {
            self.text = render(&clean_output(&key.0), language, width as usize);
            self.key = Some(key);
        }
        self.text.clone()
    }
}

fn heading(line: &str) -> bool {
    matches!(
        line,
        "REQUIREMENTS"
            | "STARTING POINT"
            | "CHECK COVERAGE"
            | "YOUR TASK"
            | "YOUR TESTS"
            | "INTERFACE"
            | "INPUT"
            | "INPUT GUARANTEES"
            | "OUTPUT"
            | "CONSTRAINTS"
    ) || line.starts_with("HINT ")
        || line.starts_with("EXAMPLE")
}

fn code(lines: &mut Vec<Line<'static>>, source: &str, extension: &str) {
    for mut line in syntax::highlight(source, extension) {
        line.spans
            .insert(0, Span::styled("│ ", Style::default().fg(MUTED)));
        lines.push(line);
    }
}

fn render(source: &str, language: Language, width: usize) -> Text<'static> {
    let extension = syntax::extension(language);
    let mut input = source.lines().peekable();
    let mut lines = Vec::new();
    let mut interface = false;
    while let Some(line) = input.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = trimmed.chars().next().unwrap();
            let count = trimmed.chars().take_while(|c| *c == marker).count();
            let tag = trimmed[count..].split_whitespace().next().unwrap_or("");
            let fenced_extension = match tag {
                "" => extension,
                "python" | "py" => "py",
                "typescript" | "ts" => "ts",
                "javascript" | "js" => "js",
                other => other,
            };
            let mut block = Vec::new();
            for next in input.by_ref() {
                let end = next.trim();
                if end.chars().take_while(|c| *c == marker).count() >= count
                    && end.chars().all(|c| c == marker)
                {
                    break;
                }
                block.push(next);
            }
            let block = block.join("\n");
            let block = if fenced_extension == "json" {
                serde_json::from_str::<Value>(&block)
                    .map(|value| format_json(&value, width.saturating_sub(2)))
                    .unwrap_or(block)
            } else {
                block
            };
            code(&mut lines, &block, fenced_extension);
            interface = false;
        } else if heading(line) {
            lines.push(Line::styled(
                line.to_owned(),
                Style::default().fg(ACCENT).bold(),
            ));
            interface = line == "INTERFACE";
            // Format cached v1 example assertions as a single stateful code block.
            if line == "EXAMPLES" {
                let mut block = Vec::new();
                while input.peek().is_some_and(|next| !heading(next)) {
                    block.push(input.next().unwrap());
                }
                code(&mut lines, block.join("\n").trim_end(), extension);
            }
        } else if let Some((label, value)) = line.split_once(": ")
            && matches!(label, "Input" | "Output")
            && let Ok(value) = serde_json::from_str::<Value>(value)
        {
            lines.push(Line::styled(
                format!("{label}:"),
                Style::default().fg(INFO).bold(),
            ));
            code(
                &mut lines,
                &format_json(&value, width.saturating_sub(2)),
                "json",
            );
        } else if interface && !line.is_empty() {
            code(&mut lines, line, extension);
            interface = false;
        } else if trimmed.starts_with("assert ") || trimmed.starts_with("assert.") {
            code(&mut lines, line, extension);
        } else if let Some((id, description)) = line.split_once("  ")
            && id.starts_with('R')
            && id.len() > 1
            && id[1..].chars().all(|c| c.is_ascii_digit())
        {
            let mut spans = vec![Span::styled(
                format!("{id}  "),
                Style::default().fg(INFO).bold(),
            )];
            spans.extend(inline(description, extension));
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(inline(line, extension)));
        }
    }
    Text::from(lines)
}

fn inline(mut source: &str, extension: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    while let Some(start) = source.find('`') {
        let count = source[start..].chars().take_while(|c| *c == '`').count();
        let delimiter = &source[start..start + count];
        let rest = &source[start + count..];
        let Some(end) = rest.find(delimiter) else {
            break;
        };
        spans.push(Span::raw(source[..start].to_owned()));
        // Backticks mark code, including identifiers that have no lexical color.
        for span in syntax::highlight(&rest[..end], extension).remove(0).spans {
            spans.push(span.patch_style(Style::default().bold()));
        }
        source = &rest[end + count..];
    }
    spans.push(Span::raw(source.to_owned()));
    spans
}

/// Keep small values on one line; expand larger containers by nesting level.
/// Arrays of short records get one record per line, rather than a wall of JSON
/// or a separate line for every field. Values and string escapes stay valid JSON.
fn format_json(value: &Value, width: usize) -> String {
    let compact = compact_json(value);
    if Line::raw(&compact).width() <= width {
        return compact;
    }
    match value {
        Value::Array(values) if !values.is_empty() => {
            let items = values
                .iter()
                .map(|value| format_json(value, width.saturating_sub(3)).replace('\n', "\n  "))
                .collect::<Vec<_>>();
            format!("[\n  {}\n]", items.join(",\n  "))
        }
        Value::Object(fields) if !fields.is_empty() => {
            let items = fields
                .iter()
                .map(|(key, value)| {
                    let prefix = format!("{}: ", serde_json::to_string(key).unwrap());
                    let available = width.saturating_sub(Line::raw(&prefix).width() + 3);
                    format!(
                        "{prefix}{}",
                        format_json(value, available).replace('\n', "\n  ")
                    )
                })
                .collect::<Vec<_>>();
            format!("{{\n  {}\n}}", items.join(",\n  "))
        }
        _ => compact,
    }
}

fn compact_json(value: &Value) -> String {
    match value {
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(compact_json)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        serde_json::to_string(key).unwrap(),
                        compact_json(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => value.to_string(),
    }
}
