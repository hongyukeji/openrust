// 第三方许可见 LICENSE 与 LICENSE.renderer。
use super::{
    line::normalize_view_text,
    mutators::{
        EnsureDynamicContentIsHighlighted, EnsureNoPunctuation, EnsurePunctuation,
        EnsureRelativePaths, Mutator,
    },
};
use crate::{
    console::{AsOutput, ConsoleError, OutputOptions, OutputReference, OutputVerbosity, Terminal},
    support::Str,
};
use regex::Regex;
use std::sync::LazyLock;
macro_rules! component_output {
    ($name:ident) => {
        pub struct $name<'a> {
            output: OutputReference<'a>,
        }
        impl<'a> $name<'a> {
            pub fn new(output: &'a mut (impl AsOutput + ?Sized)) -> Self {
                Self {
                    output: output.as_output(),
                }
            }
        }
    };
}
component_output!(Alert);
component_output!(BulletList);
component_output!(TwoColumnDetail);

fn mutate(text: &str, mutators: &[&dyn Mutator]) -> Result<String, ConsoleError> {
    mutators
        .iter()
        .try_fold(text.to_owned(), |value, mutator| mutator.apply(&value))
}
fn styling() -> &'static Regex {
    static STYLING: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<[A-Za-z0-9_=#/;,:.&,%?\-]+>|\x1b\[[0-9]+m").expect("合法的组件样式表达式")
    });
    &STYLING
}
pub(super) fn visible_width(text: &str) -> usize {
    Str::width(&styling().replace_all(text, ""))
}
// 截断后仍在原字节位置插回样式标记，保留模板渲染的边界行为。
pub(super) fn trim_text(text: &str, width: i64) -> Result<String, ConsoleError> {
    let plain = styling().replace_all(text, "");
    let width = if width < 0 {
        i128::from(width) + Str::width(&plain) as i128
    } else {
        i128::from(width)
    };
    let width = usize::try_from(width).map_err(|_| ConsoleError::new("组件文本宽度超出范围"))?;
    let mut bytes = Str::limit(&plain, width, "", false).into_bytes();
    for matched in styling().find_iter(text) {
        let offset = matched.start().min(bytes.len());
        bytes.splice(offset..offset, matched.as_str().bytes());
    }
    String::from_utf8(bytes).map_err(ConsoleError::from_source)
}
impl Alert<'_> {
    pub fn render(&mut self, text: &str) -> Result<(), ConsoleError> {
        self.render_with(text, OutputVerbosity::Normal)
    }
    pub fn render_with(
        &mut self,
        text: &str,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        let text = mutate(
            text,
            &[
                &EnsureDynamicContentIsHighlighted,
                &EnsurePunctuation,
                &EnsureRelativePaths,
            ],
        )?;
        let text = Str::upper(&normalize_view_text(&text));
        let width = Terminal::new()
            .get_width()
            .checked_sub(4)
            .ok_or_else(|| ConsoleError::new("组件终端宽度超出范围"))?;
        let length = visible_width(&text);
        let text = if length as i128 <= i128::from(width) {
            let remaining = usize::try_from(i128::from(width) - length as i128)
                .map_err(|_| ConsoleError::new("组件终端宽度超出范围"))?;
            format!(
                "{}{text}{}",
                " ".repeat(remaining / 2),
                " ".repeat(remaining - remaining / 2)
            )
        } else {
            trim_text(&text, width)?
        };
        let empty = " ".repeat(visible_width(&text));
        let message = format!(
            "\n  <bg=yellow;fg=black>{empty}</>  \n  <bg=yellow;fg=black>{text}</>  \n  <bg=yellow;fg=black>{empty}</>  "
        );
        self.output.writeln(message.as_str(), options)
    }
}
impl BulletList<'_> {
    pub fn render(
        &mut self,
        elements: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<(), ConsoleError> {
        self.render_with(elements, OutputVerbosity::Normal)
    }
    pub fn render_with(
        &mut self,
        elements: impl IntoIterator<Item = impl AsRef<str>>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        let mut elements: Vec<String> = elements
            .into_iter()
            .map(|value| value.as_ref().to_owned())
            .collect();
        for mutator in [
            &EnsureDynamicContentIsHighlighted as &dyn Mutator,
            &EnsureNoPunctuation,
            &EnsureRelativePaths,
        ] {
            for element in &mut elements {
                *element = mutator.apply(element)?;
            }
        }
        let rows: Vec<_> = elements
            .into_iter()
            .map(|value| {
                format!(
                    "  <fg=gray>{}</>  ",
                    normalize_view_text(&format!("⇂ {value}"))
                )
            })
            .collect();
        self.output.writeln(rows.join("\n").as_str(), options)
    }
}
impl TwoColumnDetail<'_> {
    pub fn render(&mut self, first: &str, second: Option<&str>) -> Result<(), ConsoleError> {
        self.render_with(first, second, OutputVerbosity::Normal)
    }
    pub fn render_with(
        &mut self,
        first: &str,
        second: Option<&str>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        let first = mutate(
            first,
            &[
                &EnsureDynamicContentIsHighlighted,
                &EnsureNoPunctuation,
                &EnsureRelativePaths,
            ],
        )?;
        let second = mutate(
            second.unwrap_or_default(),
            &[&EnsureDynamicContentIsHighlighted, &EnsureRelativePaths],
        )?;
        let has_second = !second.is_empty();
        let first = normalize_view_text(&first);
        let second = normalize_view_text(&second);
        let width = Terminal::new()
            .get_width()
            .min(150)
            .checked_sub(4)
            .ok_or_else(|| ConsoleError::new("组件终端宽度超出范围"))?;
        let dots = (i128::from(width)
            - visible_width(&first) as i128
            - 1
            - if has_second {
                visible_width(&second) as i128 + 1
            } else {
                0
            })
        .max(0) as usize;
        let second = if has_second {
            format!(" {second}")
        } else {
            String::new()
        };
        let message = format!("  {first} <fg=gray>{}</>{second}  ", ".".repeat(dots));
        self.output.writeln(message.as_str(), options)
    }
}
