use std::borrow::Cow;

/// Laravel parses rule arguments as a CSV record with backslash escape enabled.
pub(super) fn csv(mut input: &str) -> Vec<Cow<'_, str>> {
    let mut fields = Vec::new();
    loop {
        let rest = if let Some(quoted) = input
            .trim_start_matches(|character: char| character.is_ascii_whitespace())
            .strip_prefix('"')
        {
            let mut chars = quoted.char_indices().peekable();
            let mut field = String::new();
            let mut tail = "";
            while let Some((index, character)) = chars.next() {
                match character {
                    '\\' => {
                        field.push(character);
                        field.extend(chars.next().map(|(_, character)| character));
                    }
                    '"' if chars.peek().is_some_and(|(_, next)| *next == '"') => {
                        chars.next();
                        field.push('"');
                    }
                    '"' => {
                        tail = &quoted[index + 1..];
                        break;
                    }
                    character => field.push(character),
                }
            }
            let (suffix, rest) = tail
                .split_once(',')
                .map_or((tail, None), |(field, rest)| (field, Some(rest)));
            field.push_str(suffix);
            fields.push(Cow::Owned(field));
            rest
        } else {
            let (field, rest) = input
                .split_once(',')
                .map_or((input, None), |(field, rest)| (field, Some(rest)));
            fields.push(Cow::Borrowed(field));
            rest
        };
        let Some(rest) = rest else {
            return fields;
        };
        input = rest;
    }
}
