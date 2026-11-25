use super::{ConsoleQuestionError, Question};
use crate::console::{ConsoleError, OutputFormatterStyle, OutputOptions, OutputStyle};
use crossterm::event::{KeyCode, KeyModifiers};
use std::sync::Arc;

fn recent(value: &str) -> &str {
    if let Some((_, last)) = value.rsplit_once(',') {
        let last = last.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']);
        if !last.is_empty() {
            return last;
        }
    }
    value
}
fn trim(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
}
fn suffix(value: &str, byte: usize) -> Result<&str, ConsoleError> {
    if byte >= value.len() {
        return Ok("");
    }
    value
        .get(byte..)
        .ok_or_else(|| ConsoleError::new("自动补全候选的截取位置不是有效UTF-8边界"))
}
fn prefix(value: &str, count: usize) -> String {
    value.chars().take(count).collect()
}
fn write(style: &mut OutputStyle<'_>, value: &str) -> Result<(), ConsoleError> {
    style.write(value, false, OutputOptions::default())
}
/// 输入、候选与已接受文本分别保存；退格先移除候选，再移除已输入字符。
pub(super) async fn read(
    style: &mut OutputStyle<'_>,
    question: &Question,
) -> Result<Option<String>, ConsoleError> {
    let callback = question.completion.as_ref().expect("调用前已核对补全回调");
    let mut full = String::new();
    let mut entered = String::new();
    let mut count = 0_usize;
    let mut selected: Option<usize> = None;
    let mut matches = callback.call(entered.clone()).await?;
    let mut available = matches.len();
    style.get_formatter().set_style(
        "hl",
        Arc::new(OutputFormatterStyle::new(
            Some("black"),
            Some("white"),
            &[],
        )?),
    );
    loop {
        let key = super::reader::key_event().await?;
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if key.code == KeyCode::Char('c') {
                return Err(ConsoleError::from_source(ConsoleQuestionError::Interrupted));
            }
            // 将原生终端的EOF键交给同一问答耗尽／默认值机制。
            if matches!(key.code, KeyCode::Char('d') | KeyCode::Char('z')) {
                return Ok((!full.is_empty()).then_some(full));
            }
        }
        match key.code {
            KeyCode::Backspace => {
                if available == 0 && count != 0 {
                    count -= 1;
                    let length = crate::console::formatting::grapheme_length(&full)
                        .map_err(ConsoleError::from_source)?;
                    let slices = crate::console::formatting::unicode::Slices::new(&full)
                        .map_err(ConsoleError::from_source)?;
                    let width = crate::console::formatting::display_width(
                        slices.slice(length.saturating_sub(1), None),
                    );
                    write(style, &format!("\x1b[{width}D"))?;
                    full = prefix(&full, count);
                }
                if count == 0 {
                    selected = None;
                    matches = callback.call(entered.clone()).await?;
                    available = matches.len();
                } else {
                    available = 0;
                }
                entered = prefix(&entered, count);
            }
            KeyCode::Up | KeyCode::Down => {
                if available == 0 {
                    continue;
                }
                selected = Some(match selected {
                    None if key.code == KeyCode::Down => 0,
                    None => available - 1,
                    Some(index) if key.code == KeyCode::Down => (index + 1) % available,
                    Some(index) => (index + available - 1) % available,
                });
            }
            KeyCode::Tab | KeyCode::Enter => {
                if available > 0
                    && let Some(index) = selected
                {
                    entered = matches[index].clone();
                    let remaining = suffix(&entered, trim(recent(&full)).len())?;
                    write(style, remaining)?;
                    full.push_str(remaining);
                    count = full.chars().count();
                    matches = callback
                        .call(entered.clone())
                        .await?
                        .into_iter()
                        .filter(|value| entered.is_empty() || value.starts_with(&entered))
                        .collect();
                    selected = None;
                }
                if key.code == KeyCode::Enter {
                    write(style, "\n")?;
                    return Ok(Some(full));
                }
                available = 0;
                continue;
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    || key
                        .modifiers
                        .contains(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                write(style, &character.to_string())?;
                entered.push(character);
                full.push(character);
                count += 1;
                let candidate_prefix = if question
                    .choice
                    .as_ref()
                    .is_some_and(|choice| choice.multiple)
                {
                    recent(&full)
                } else {
                    &entered
                };
                matches = callback
                    .call(entered.clone())
                    .await?
                    .into_iter()
                    .filter(|value| value.starts_with(candidate_prefix))
                    .collect();
                available = matches.len();
                selected = Some(0);
            }
            _ => continue,
        }
        write(style, "\x1b[K")?;
        if available > 0
            && let Some(index) = selected
        {
            write(style, "\x1b7")?;
            let remaining = suffix(&matches[index], trim(recent(&full)).len())?;
            let escaped =
                crate::console::formatting::Formatter::escape_trailing_backslash(remaining);
            write(style, &format!("<hl>{escaped}</hl>"))?;
            write(style, "\x1b8")?;
        }
    }
}
