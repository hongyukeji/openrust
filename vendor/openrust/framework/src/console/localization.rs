//! Localized Artisan presentation text.

use crate::support::facades::Lang;

pub(super) fn text(key: &str, english: &str, simplified: &str, traditional: &str) -> String {
    let key = format!("console.{key}");
    if let Ok(translated) = Lang::get(&key, &Default::default(), None)
        && translated != key
    {
        return translated;
    }
    let locale = Lang::getLocale().unwrap_or_default();
    let normalized = locale.replace('_', "-").to_ascii_lowercase();
    match normalized.as_str() {
        "zh" | "zh-cn" | "zh-sg" | "zh-hans" => simplified.into(),
        "zh-tw" | "zh-hk" | "zh-mo" | "zh-hant" => traditional.into(),
        _ => english.into(),
    }
}

pub(super) fn description(command: &clap::Command) -> String {
    let fallback = command
        .get_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    let (simplified, traditional) = match command.get_name() {
        "list" => ("列出可用命令", "列出可用命令"),
        "help" => ("显示命令帮助", "顯示命令說明"),
        "inspire" => ("显示一句励志名言", "顯示一句勵志名言"),
        _ => (fallback.as_str(), fallback.as_str()),
    };
    text(
        &format!("commands.{}.description", command.get_name()),
        &fallback,
        simplified,
        traditional,
    )
}

pub(super) fn headings(rendered: &str) -> String {
    rendered
        .split_inclusive('\n')
        .map(|line| {
            for (key, english, simplified, traditional) in [
                ("usage", "Usage:", "用法：", "用法："),
                ("options", "Options:", "选项：", "選項："),
                ("arguments", "Arguments:", "参数：", "引數："),
                ("commands", "Commands:", "命令：", "命令："),
            ] {
                if let Some(tail) = line.strip_prefix(english) {
                    return format!("{}{tail}", text(key, english, simplified, traditional));
                }
            }
            line.into()
        })
        .collect()
}

pub(super) fn argument_error(error: &clap::Error) -> String {
    use clap::error::{ContextKind, ErrorKind};

    let context = |kind| error.get(kind).map(ToString::to_string).unwrap_or_default();
    let (key, english, simplified, traditional, details) = match error.kind() {
        ErrorKind::UnknownArgument => (
            "unknown_argument",
            "Unexpected argument",
            "无法识别的参数",
            "無法辨識的引數",
            context(ContextKind::InvalidArg),
        ),
        ErrorKind::InvalidSubcommand => (
            "unknown_command",
            "Command is not defined",
            "命令不存在",
            "命令不存在",
            context(ContextKind::InvalidSubcommand),
        ),
        ErrorKind::MissingRequiredArgument => (
            "missing_argument",
            "Required arguments are missing",
            "缺少必填参数",
            "缺少必填引數",
            context(ContextKind::InvalidArg),
        ),
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => (
            "invalid_value",
            "Invalid value",
            "参数值无效",
            "引數值無效",
            format!(
                "{} {}",
                context(ContextKind::InvalidArg),
                context(ContextKind::InvalidValue)
            ),
        ),
        ErrorKind::ArgumentConflict => (
            "argument_conflict",
            "Arguments conflict",
            "参数冲突",
            "引數衝突",
            format!(
                "{} {}",
                context(ContextKind::InvalidArg),
                context(ContextKind::PriorArg)
            ),
        ),
        _ => (
            "invalid_arguments",
            "Invalid arguments",
            "参数无效",
            "引數無效",
            context(ContextKind::InvalidArg),
        ),
    };
    let label = text(key, english, simplified, traditional);
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
