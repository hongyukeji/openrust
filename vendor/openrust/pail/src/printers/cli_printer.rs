use std::path::PathBuf;

use openrust::console::{ConsoleError, Output};
use serde_json::Value;

use crate::{
    contracts::Printer,
    value_objects::{MessageLogged, Origin},
};

/// Pail's CLI presentation, rendered directly without a PHP HTML/DOM runtime.
pub struct CliPrinter<'a> {
    output: &'a mut Output,
    base_path: PathBuf,
}

impl<'a> CliPrinter<'a> {
    pub fn new(output: &'a mut Output, base_path: impl Into<PathBuf>) -> Self {
        Self {
            output,
            base_path: base_path.into(),
        }
    }

    fn width(&self) -> usize {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| usize::from(console::Term::stdout().size().1))
    }

    fn relative(&self, file: &str) -> String {
        let base = self.base_path.to_string_lossy();
        #[cfg(windows)]
        let (base, file) = (base.replace('\\', "/"), file.replace('\\', "/"));
        file.replace(&format!("{base}/"), "")
    }

    fn truncate_class_or_type(&self, class_or_type: &str) -> String {
        if self.output.is_verbose() {
            return class_or_type.into();
        }
        abbreviate(
            class_or_type,
            if class_or_type.contains('\\') {
                "\\"
            } else {
                "::"
            },
        )
    }

    fn truncate_message(&self, message: &str) -> String {
        if self.output.is_verbose() {
            return message.into();
        }
        substring(message, self.width().saturating_sub(5).min(145))
    }

    fn file(&self, message: &MessageLogged, class_or_type: &str) -> Option<String> {
        let mut file = self.relative(message.file()?);
        if !self.output.is_verbose() {
            file = abbreviate(&file, "/");
            let size = self
                .width()
                .saturating_sub(class_or_type.len() + 16)
                .min(145);
            file = substring(&file, size);
        }
        (file != "…").then(|| file.replace("……", "…"))
    }

    fn options(
        &self,
        message: &MessageLogged,
    ) -> Result<Vec<(Option<String>, String)>, ConsoleError> {
        let mut options: Vec<(Option<String>, Value)> = match message.origin().map_err(error)? {
            Origin::Http(origin) => {
                let path = if origin.path.starts_with('/') {
                    origin.path
                } else {
                    format!("/{}", origin.path)
                };
                let auth = origin
                    .auth_id
                    .as_deref()
                    .filter(|id| truthy(id))
                    .map_or_else(
                        || "guest".into(),
                        |id| match origin.auth_email.as_deref().filter(|mail| truthy(mail)) {
                            Some(email) => format!("{id} ({email})"),
                            None => id.to_owned(),
                        },
                    );
                vec![
                    (Some(origin.method.to_ascii_uppercase()), path.into()),
                    (Some("Auth ID".into()), auth.into()),
                ]
            }
            Origin::Queue(origin) => vec![
                (
                    None,
                    origin
                        .command
                        .filter(|value| truthy(value))
                        .map(|command| format!("artisan {command}"))
                        .into(),
                ),
                (None, origin.queue.into()),
                (None, origin.job.into()),
            ],
            Origin::Console(origin) => vec![(
                None,
                origin
                    .command
                    .filter(|value| truthy(value))
                    .map_or_else(|| "artisan".into(), |command| format!("artisan {command}"))
                    .into(),
            )],
        };
        for (key, value) in message.context() {
            if numeric_key(&key) {
                options.push((None, value));
            } else if let Some((_, original)) = options
                .iter_mut()
                .find(|(name, _)| name.as_deref() == Some(&key))
            {
                *original = value;
            } else {
                options.push((Some(key), value));
            }
        }
        Ok(options
            .into_iter()
            .filter(|(key, value)| key.is_some() || !value.is_null())
            .map(|(key, value)| {
                let value = value
                    .as_str()
                    .map_or_else(|| export(&value, 0), str::to_owned);
                (key, value)
            })
            .collect())
    }

    pub fn options_html(&self, message: &MessageLogged) -> Result<String, ConsoleError> {
        Ok(self
            .options(message)?
            .into_iter()
            .map(|(key, value)| {
                let value = escape(&value);
                let value = key.map_or_else(|| value.clone(), |key| format!("{key}: {value}"));
                format!("<span class=\"font-bold\">{value}</span>")
            })
            .collect::<Vec<_>>()
            .join(" • "))
    }

    fn trace(&self, message: &MessageLogged) -> Vec<String> {
        if !self.output.is_very_verbose() {
            return Vec::new();
        }
        message
            .trace()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let file = frame.get("file").and_then(Value::as_str).unwrap_or("");
                let line = frame.get("line").map_or_else(String::new, scalar);
                format!("{}. {}:{line}", index + 1, self.relative(file))
            })
            .collect()
    }

    pub fn trace_html(&self, message: &MessageLogged) -> String {
        self.trace(message).into_iter().map(|frame| format!(
            "    <div class=\"flex text-gray\">\n        <span>\n            <span class=\"mr-1 text-gray\">│</span>\n            <span>{frame} </span>\n        </span>\n    </div>"
        )).collect()
    }

    fn style(&self, text: &str, color: &str, bold: bool) -> String {
        if !self.output.is_decorated() || text.is_empty() {
            return text.into();
        }
        let code = match color {
            "blue" => 34,
            "yellow" => 33,
            "red" => 31,
            _ => 90,
        };
        if bold {
            format!("\x1b[{code};1m{text}\x1b[39;22m")
        } else {
            format!("\x1b[{code}m{text}\x1b[39m")
        }
    }
}

impl Printer for CliPrinter<'_> {
    fn print(&mut self, message: &MessageLogged) -> Result<(), ConsoleError> {
        let verbose = self.output.is_verbose();
        let width = self.width().min(150);
        let class = self.truncate_class_or_type(&message.class_or_type());
        let date = if verbose {
            message.date()
        } else {
            message.time()
        }
        .map_err(error)?;
        let file = self
            .file(message, &class)
            .map(|file| format!(" {file} "))
            .unwrap_or_default();
        let ending = if verbose { "" } else { "┐" };
        let top = format!("┌ {date} {class} ");
        let bars =
            "─".repeat(width.saturating_sub(measure(&top) + measure(&file) + measure(ending)));
        let mut frame = self.style(&format!("┌ {date} "), "gray", false);
        frame.push_str(&self.style(&class, message.color(), true));
        frame.push_str(&self.style(&format!(" {bars}{file}{ending}"), "gray", false));
        frame.push('\n');
        let text = self.truncate_message(message.message());
        let empty = !truthy(&text);
        let text = if empty {
            "No message.".into()
        } else if text.contains('\n') {
            preformatted(&text)
        } else {
            collapse(&text)
        };
        frame.push_str(&self.style("│ ", "gray", false));
        frame.push_str(&if empty {
            self.style(&text, "gray", false)
        } else {
            text.clone()
        });
        if !verbose {
            frame.push_str(&" ".repeat(width.saturating_sub(3 + measure(&text))));
            frame.push_str(&self.style("│", "gray", false));
        }
        frame.push('\n');
        for trace in self.trace(message) {
            frame.push_str(&self.style(&format!("│ {trace}"), "gray", false));
            frame.push('\n');
        }
        let options = self
            .options(message)?
            .into_iter()
            .map(|(key, value)| {
                collapse(&key.map_or_else(|| value.clone(), |key| format!("{key}: {value}")))
            })
            .collect::<Vec<_>>();
        let options_text = options.join(" • ");
        let ending = if verbose { "" } else { "┘" };
        let bars = "─".repeat(width.saturating_sub(3 + measure(ending) + measure(&options_text)));
        frame.push_str(&self.style(&format!("└{bars} "), "gray", false));
        for (index, option) in options.iter().enumerate() {
            if index != 0 {
                frame.push_str(&self.style(" • ", "gray", false));
            }
            frame.push_str(&self.style(option, "gray", true));
        }
        if !verbose {
            frame.push_str(&self.style(&format!(" {ending}"), "gray", false));
        }
        frame.push('\n');
        self.output.write_raw(&frame).map_err(error)
    }
}

fn truthy(value: &str) -> bool {
    !value.is_empty() && value != "0"
}
fn measure(value: &str) -> usize {
    static STYLING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"<[A-Za-z0-9_=#/;,:.&,%?\-]+>|\x1b\[[0-9]+m")
            .expect("valid Termwind styling pattern")
    });
    openrust::support::Str::width(&STYLING.replace_all(value, ""))
}
fn numeric_key(value: &str) -> bool {
    value
        .parse::<i64>()
        .is_ok_and(|number| number.to_string() == value)
}
fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
fn scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => if *value { "1" } else { "" }.into(),
        _ => value.to_string(),
    }
}
fn collapse(value: &str) -> String {
    value
        .replace(['\0', '\x0b'], " ")
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn abbreviate(value: &str, separator: &str) -> String {
    let parts: Vec<_> = value.split(separator).collect();
    if parts.len() > 4 {
        [parts[0], parts[1], "…", parts[parts.len() - 1]].join(separator)
    } else {
        value.into()
    }
}
// Pail compares byte length before taking a Unicode substring.
fn substring(value: &str, size: usize) -> String {
    if value.len() > size {
        format!("{}…", value.chars().take(size).collect::<String>())
    } else {
        value.into()
    }
}
fn preformatted(value: &str) -> String {
    let mut lines: Vec<_> = value.split('\n').collect();
    if lines.first() == Some(&"") {
        lines.remove(0);
    }
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let width = lines.iter().map(|line| line.len()).max().unwrap_or(0) + 3;
    lines
        .into_iter()
        .map(|line| format!("\n{line}{}", " ".repeat(width - line.len())))
        .collect()
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#039;")
}
fn export(value: &Value, indent: usize) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::Bool(value) => value.to_string(),
        Value::String(value) => value
            .split('\0')
            .map(|part| format!("'{}'", part.replace('\\', "\\\\").replace('\'', "\\'")))
            .collect::<Vec<_>>()
            .join(" . \"\\0\" . "),
        Value::Number(value) if value.is_f64() => {
            let number = value.as_f64().expect("JSON floating point number");
            let absolute = number.abs();
            if absolute != 0.0 && !(0.0001..1e17).contains(&absolute) {
                let exponential = format!("{number:e}");
                let (mantissa, exponent) = exponential.split_once('e').expect("exponent");
                format!(
                    "{mantissa}{}E{}{exponent}",
                    if mantissa.contains('.') { "" } else { ".0" },
                    if exponent.starts_with('-') { "" } else { "+" }
                )
            } else {
                let mut text = number.to_string();
                if !text.contains('.') {
                    text.push_str(".0");
                }
                text
            }
        }
        Value::Number(value) => value.to_string(),
        Value::Array(values) => export_array(
            values
                .iter()
                .enumerate()
                .map(|(key, value)| (key.to_string(), value)),
            indent,
        ),
        Value::Object(values) => export_array(
            values.iter().map(|(key, value)| {
                (
                    if numeric_key(key) {
                        key.clone()
                    } else {
                        export(&Value::from(key.as_str()), 0)
                    },
                    value,
                )
            }),
            indent,
        ),
    }
}
fn export_array<'a>(values: impl Iterator<Item = (String, &'a Value)>, indent: usize) -> String {
    let mut output = "array (\n".to_owned();
    for (key, value) in values {
        let prefix = if value.is_array() || value.is_object() {
            format!("\n{}", " ".repeat(indent + 2))
        } else {
            String::new()
        };
        output.push_str(&format!(
            "{}{key} => {prefix}{},\n",
            " ".repeat(indent + 2),
            export(value, indent + 2)
        ));
    }
    output.push_str(&format!("{})", " ".repeat(indent)));
    output
}
