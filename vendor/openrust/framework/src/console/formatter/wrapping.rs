use super::{ConsoleFormatError, OutputFormatter};
impl OutputFormatter {
    pub(super) fn apply_current(
        &self,
        text: &str,
        current: &str,
        width: i64,
        line_length: &mut usize,
    ) -> Result<String, ConsoleFormatError> {
        if text.is_empty() {
            return Ok(String::new());
        }
        if width == 0 {
            let mut output = String::new();
            if self.is_decorated() {
                self.stack.get_current().append_to(text, &mut output)?;
            } else {
                output.push_str(text);
            }
            return Ok(output);
        }
        let mut text = text.to_owned();
        if *line_length == 0 && !current.is_empty() {
            text = text
                .trim_start_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
                .to_owned();
        }
        let mut prefix = String::new();
        if *line_length != 0 {
            let mut lines = text.splitn(2, '\n');
            let first = lines.next().unwrap_or_default();
            let rest = lines.next().map(str::to_owned);
            let available = width.saturating_sub(i64::try_from(*line_length).unwrap_or(i64::MAX));
            let slices = super::super::formatting::unicode::Slices::new(first)?;
            let count = super::super::formatting::unicode::length(first)?;
            let end = if available >= 0 {
                usize::try_from(available).unwrap_or(usize::MAX).min(count)
            } else {
                count.saturating_sub(available.unsigned_abs() as usize)
            };
            prefix = slices.slice(0, Some(end)).to_owned();
            prefix.push('\n');
            text = slices.slice(end, None).to_owned();
            if let Some(rest) = rest {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&rest);
            }
        }
        let ending = text.ends_with('\n');
        let wrapped = word_wrap(&text, width)?;
        let mut rendered = prefix + &wrapped;
        rendered = rendered.trim_end_matches('\n').to_owned();
        if ending {
            rendered.push('\n');
        }
        if *line_length == 0 && !current.is_empty() && !current.ends_with('\n') {
            rendered.insert(0, '\n');
        }
        let lines: Vec<&str> = rendered.split('\n').collect();
        let mut output = String::new();
        for (index, line) in lines.iter().enumerate() {
            let length = super::super::formatting::unicode::length(line)?;
            *line_length = if index == 0 {
                line_length.saturating_add(length)
            } else {
                length
            };
            if width <= i64::try_from(*line_length).unwrap_or(i64::MAX) {
                *line_length = 0;
            }
            if index > 0 {
                output.push('\n');
            }
            if self.is_decorated() {
                self.stack.get_current().append_to(line, &mut output)?;
            } else {
                output.push_str(line);
            }
        }
        Ok(output)
    }
}
fn word_wrap(text: &str, width: i64) -> Result<String, ConsoleFormatError> {
    if text.is_empty() {
        return Ok(String::new());
    }
    let mut chars = Vec::<String>::new();
    let mut mask = String::new();
    let slices = super::super::formatting::unicode::Slices::new(text)?;
    for index in 0..super::super::formatting::unicode::length(text)? {
        let character = slices.slice(index, Some(index + 1));
        chars.push(character.to_owned());
        mask.push(match character {
            " " => ' ',
            "\n" => '#',
            _ => '?',
        });
    }
    let wrapped = if width < 0 {
        let mut output = String::new();
        for ch in mask.chars() {
            output.push('#');
            if ch != ' ' && ch != '#' {
                output.push(ch);
            }
        }
        output
    } else {
        crate::support::Str::word_wrap(
            &mask,
            usize::try_from(width).map_err(ConsoleFormatError::from_source)?,
            "#",
            true,
        )
        .map_err(ConsoleFormatError::from_source)?
    };
    let mut output = String::new();
    let mut cursor = 0;
    for ch in wrapped.chars() {
        if ch == '#' {
            if chars
                .get(cursor)
                .is_some_and(|text| text == "\n" || text == " ")
            {
                cursor += 1;
            }
            output.push('\n');
        } else if let Some(text) = chars.get(cursor) {
            output.push_str(text);
            cursor += 1;
        }
    }
    for text in &chars[cursor..] {
        output.push_str(text);
    }
    Ok(output)
}
