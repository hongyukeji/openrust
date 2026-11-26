// 许可见 LICENSE.symfony。
use super::{Helper, HelperInterface, HelperSet, OutputFormatter, OutputMessages};

/// 生成可由输出格式器处理的标题、文本块和截断文本。
#[derive(Debug, Default)]
pub struct FormatterHelper {
    helper: Helper,
}

impl FormatterHelper {
    pub const fn new() -> Self {
        Self {
            helper: Helper::new(),
        }
    }

    pub const fn get_name(&self) -> &'static str {
        "formatter"
    }

    pub fn set_helper_set(&self, set: Option<&HelperSet>) {
        self.helper.set_helper_set(set);
    }

    pub fn get_helper_set(&self) -> Option<HelperSet> {
        self.helper.get_helper_set()
    }

    pub fn format_section(&self, section: &str, message: &str) -> String {
        self.format_section_with(section, message, "info")
    }

    pub fn format_section_with(&self, section: &str, message: &str, style: &str) -> String {
        format!("<{style}>[{section}]</{style}> {message}")
    }

    pub fn format_block<'a>(&self, messages: impl Into<OutputMessages<'a>>, style: &str) -> String {
        self.format_block_with(messages, style, false)
    }

    pub fn format_block_with<'a>(
        &self,
        messages: impl Into<OutputMessages<'a>>,
        style: &str,
        large: bool,
    ) -> String {
        let messages = messages.into();
        let lines = match &messages {
            OutputMessages::Text(message) => std::slice::from_ref(message),
            OutputMessages::Lines(messages) => messages,
        };
        let padding = if large { "  " } else { " " };
        let mut width = 0;
        let lines: Vec<_> = lines
            .iter()
            .map(|message| {
                let escaped = OutputFormatter::escape(message);
                let row_width = Helper::width(Some(&escaped)) + 2 * padding.len();
                width = width.max(row_width);
                (escaped, row_width)
            })
            .collect();

        let mut block = Vec::with_capacity(lines.len() + usize::from(large) * 2);
        if large {
            block.push(format!("<{style}>{}</{style}>", " ".repeat(width)));
        }
        for (line, row_width) in lines {
            let trailing = " ".repeat(width - row_width);
            block.push(format!(
                "<{style}>{padding}{line}{padding}{trailing}</{style}>"
            ));
        }
        if large {
            block.push(format!("<{style}>{}</{style}>", " ".repeat(width)));
        }
        block.join("\n")
    }

    pub fn truncate(&self, message: &str, length: i64) -> String {
        self.truncate_with(message, length, "...")
    }

    pub fn truncate_with(&self, message: &str, length: i64, suffix: &str) -> String {
        let remaining = i128::from(length) - Helper::width(Some(suffix)) as i128;
        if remaining > Helper::width(Some(message)) as i128 {
            return message.to_owned();
        }
        let text = Helper::substr(Some(message), 0, Some(length));
        format!("{text}{suffix}")
    }
}

impl HelperInterface for FormatterHelper {
    fn get_name(&self) -> &str {
        self.get_name()
    }

    fn set_helper_set(&self, set: Option<&HelperSet>) {
        self.set_helper_set(set);
    }

    fn get_helper_set(&self) -> Option<HelperSet> {
        self.get_helper_set()
    }
}

crate::console::helper::impl_helper_tools!(FormatterHelper);
