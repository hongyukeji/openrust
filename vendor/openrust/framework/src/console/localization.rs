//! Localized Artisan presentation text.

use crate::support::facades::Lang;

// 语言选择、资源加载与回退交给统一译器；启动前尚无译器时保留默认文案。
pub(crate) fn text(key: &str, fallback: &str) -> String {
    text_for_locale(key, fallback, None)
}

pub(crate) fn text_for_locale(key: &str, fallback: &str, locale: Option<&str>) -> String {
    let key = format!("console.{key}");
    match Lang::get(&key, &Default::default(), locale) {
        Ok(translated) if translated != key => translated,
        _ => fallback.to_owned(),
    }
}

pub(super) fn description(command: &clap::Command) -> String {
    let fallback = command
        .get_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    text(
        &format!("commands.{}.description", command.get_name()),
        &fallback,
    )
}

pub(super) fn headings(rendered: &str) -> String {
    styled_headings(rendered, "", "")
}

pub(super) fn styled_headings(rendered: &str, usage_style: &str, header_style: &str) -> String {
    rendered
        .split_inclusive('\n')
        .map(|line| {
            for (key, english) in [
                ("usage", "Usage:"),
                ("options", "Options:"),
                ("arguments", "Arguments:"),
                ("commands.heading", "Commands:"),
            ] {
                let style = if key == "usage" {
                    usage_style
                } else {
                    header_style
                };
                if let Some(tail) = line
                    .strip_prefix(style)
                    .and_then(|line| line.strip_prefix(english))
                {
                    return format!("{style}{}{tail}", text(key, english));
                }
            }
            line.into()
        })
        .collect()
}

pub(super) fn argument_error(error: &clap::Error) -> String {
    use clap::error::{ContextKind, ErrorKind};

    let context = |kind| error.get(kind).map(ToString::to_string).unwrap_or_default();
    let (key, english, details) = match error.kind() {
        ErrorKind::UnknownArgument => (
            "unknown_argument",
            "Unexpected argument",
            context(ContextKind::InvalidArg),
        ),
        ErrorKind::InvalidSubcommand => (
            "unknown_command",
            "Command is not defined",
            context(ContextKind::InvalidSubcommand),
        ),
        ErrorKind::MissingRequiredArgument => (
            "missing_argument",
            "Required arguments are missing",
            context(ContextKind::InvalidArg),
        ),
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => (
            "invalid_value",
            "Invalid value",
            format!(
                "{} {}",
                context(ContextKind::InvalidArg),
                context(ContextKind::InvalidValue)
            ),
        ),
        ErrorKind::ArgumentConflict => (
            "argument_conflict",
            "Arguments conflict",
            format!(
                "{} {}",
                context(ContextKind::InvalidArg),
                context(ContextKind::PriorArg)
            ),
        ),
        _ => (
            "invalid_arguments",
            "Invalid arguments",
            context(ContextKind::InvalidArg),
        ),
    };
    let label = text(key, english);
    let rendered = error.to_string();
    if label == english {
        return rendered;
    }
    let usage = context(ContextKind::Usage);
    let details = details.trim();
    if usage.is_empty() {
        format!("{label}: {details}\n")
    } else {
        format!("{label}: {details}\n\n{}\n", headings(&usage))
    }
}
