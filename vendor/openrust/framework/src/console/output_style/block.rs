use super::{ConsoleError, OutputMessages, OutputOptions, OutputStyle};

/// 文本块的可选标签、样式与排版参数。
#[derive(Clone, Copy, Debug)]
pub struct BlockOptions<'a> {
    pub kind: Option<&'a str>,
    pub style: Option<&'a str>,
    pub prefix: &'a str,
    pub padding: bool,
    pub escape: bool,
}
impl Default for BlockOptions<'_> {
    fn default() -> Self {
        Self {
            kind: None,
            style: None,
            prefix: " ",
            padding: false,
            escape: true,
        }
    }
}
impl OutputStyle<'_> {
    /// 按所选格式器与构造时的终端宽度排版文本块。
    pub fn block<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: BlockOptions<'_>,
    ) -> Result<(), ConsoleError> {
        let messages = match messages.into() {
            OutputMessages::Text(message) => vec![message],
            OutputMessages::Lines(messages) => messages.to_vec(),
        };
        self.prepare_block()?;
        let prefix =
            super::super::formatting::unformatted_text(self.output.formatter(), options.prefix)
                .map_err(ConsoleError::from_source)?;
        let prefix_width = super::super::formatting::display_width(&prefix);
        let label = options.kind.map(|kind| format!("[{kind}] "));
        let indent = label
            .as_deref()
            .map_or(0, super::super::formatting::display_width);
        let mut lines = Vec::new();
        for (index, message) in messages.iter().enumerate() {
            let message = if options.escape {
                super::super::OutputFormatter::escape(message)
            } else {
                (*message).to_owned()
            };
            lines.extend(super::wrapping::wrap(
                &message,
                i128::from(self.line_length) - prefix_width as i128 - indent as i128,
            )?);
            if index + 1 < messages.len() {
                lines.push(String::new());
            }
        }
        let first = usize::from(options.padding && self.is_decorated());
        if first == 1 {
            lines.insert(0, String::new());
            lines.push(String::new());
        }
        for (index, line) in lines.iter_mut().enumerate() {
            if let Some(label) = &label {
                *line = format!(
                    "{}{line}",
                    if index == first {
                        label.to_owned()
                    } else {
                        " ".repeat(indent)
                    }
                );
            }
            *line = format!("{}{line}", options.prefix);
            let plain = super::super::formatting::unformatted_text(self.output.formatter(), line)
                .map_err(ConsoleError::from_source)?;
            let padding = (i128::from(self.line_length)
                - super::super::formatting::display_width(&plain) as i128)
                .max(0);
            line.push_str(
                &" ".repeat(usize::try_from(padding).map_err(ConsoleError::from_source)?),
            );
            if let Some(style) = options
                .style
                .filter(|style| !style.is_empty() && *style != "0")
            {
                *line = format!("<{style}>{line}</>");
            }
        }
        self.writeln_iter(lines, OutputOptions::default())?;
        self.new_line(1)
    }
    pub fn comment<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.block(
            messages,
            BlockOptions {
                prefix: "<fg=default;bg=default> // </>",
                escape: false,
                ..BlockOptions::default()
            },
        )
    }
    pub fn success<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.block(
            messages,
            BlockOptions {
                kind: Some("OK"),
                style: Some("fg=black;bg=green"),
                padding: true,
                ..BlockOptions::default()
            },
        )
    }
    pub fn error<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.block(
            messages,
            BlockOptions {
                kind: Some("ERROR"),
                style: Some("fg=white;bg=red"),
                padding: true,
                ..BlockOptions::default()
            },
        )
    }
    pub fn warning<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.block(
            messages,
            BlockOptions {
                kind: Some("WARNING"),
                style: Some("fg=black;bg=yellow"),
                padding: true,
                ..BlockOptions::default()
            },
        )
    }
    pub fn note<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.block(
            messages,
            BlockOptions {
                kind: Some("NOTE"),
                style: Some("fg=yellow"),
                prefix: " ! ",
                ..BlockOptions::default()
            },
        )
    }
    pub fn info<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.block(
            messages,
            BlockOptions {
                kind: Some("INFO"),
                style: Some("fg=green"),
                padding: true,
                ..BlockOptions::default()
            },
        )
    }
    pub fn caution<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.block(
            messages,
            BlockOptions {
                kind: Some("CAUTION"),
                style: Some("fg=white;bg=red"),
                prefix: " ! ",
                padding: true,
                ..BlockOptions::default()
            },
        )
    }
}
