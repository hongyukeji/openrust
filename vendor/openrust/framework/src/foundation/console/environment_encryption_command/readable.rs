use std::collections::{HashMap, VecDeque};

use zeroize::Zeroizing;

use crate::{console::ConsoleError, encryption::Encrypter};

use super::error;

type Entries<'a> = Vec<(&'a str, &'a str, Zeroizing<Vec<u8>>)>;

pub(super) fn encrypt(
    contents: &str,
    previous: Option<&str>,
    encrypter: &Encrypter,
) -> Result<String, ConsoleError> {
    let (mut existing, mut previous_output) = (HashMap::<&str, VecDeque<_>>::new(), String::new());
    if let Some(previous) = previous {
        if Encrypter::appearsEncrypted(previous) {
            return Err(ConsoleError::new(
                "The existing encrypted environment file is not in readable format. Use --force to overwrite it.",
            ));
        }
        for (name, encrypted, value) in encrypted_entries(previous, encrypter)? {
            existing
                .entry(name)
                .or_default()
                .push_back((encrypted, value));
            append(&mut previous_output, name, encrypted);
        }
    }

    let normalized = Zeroizing::new(contents.replace("\r\n", "\n").replace('\r', "\n"));
    let mut result = String::new();
    for (name, value) in raw_entries(&normalized) {
        let reused = existing.get_mut(name).and_then(|items| {
            let index = items
                .iter()
                .position(|(_, old)| old.as_slice() == value.as_bytes())?;
            items.remove(index).map(|entry| entry.0)
        });
        match reused {
            Some(payload) => append(&mut result, name, payload),
            None => append(
                &mut result,
                name,
                &encrypter.encrypt_string(value.as_bytes()).map_err(error)?,
            ),
        }
    }
    Ok(if previous.is_some() && result == previous_output {
        previous.unwrap_or_default().to_owned()
    } else {
        result
    })
}

pub(super) fn decrypt(
    contents: &str,
    encrypter: &Encrypter,
) -> Result<Zeroizing<Vec<u8>>, ConsoleError> {
    let mut file = Zeroizing::new(Vec::new());
    for (name, _, value) in encrypted_entries(contents, encrypter)? {
        file.extend_from_slice(name.as_bytes());
        file.push(b'=');
        file.extend_from_slice(&value);
        file.push(b'\n');
    }
    Ok(file)
}

fn encrypted_entries<'a>(
    contents: &'a str,
    encrypter: &Encrypter,
) -> Result<Entries<'a>, ConsoleError> {
    let mut entries = Vec::new();
    let lines = contents
        .split('\n')
        .flat_map(|line| line.strip_suffix('\r').unwrap_or(line).split('\r'));
    for (index, line) in lines.enumerate() {
        let number = index + 1;
        if ignored(line) {
            continue;
        }
        let (name, payload) = line
            .split_once('=')
            .filter(|(name, _)| !name.is_empty())
            .ok_or_else(|| {
                ConsoleError::new(format!(
                    "Invalid encrypted environment entry on line {number}."
                ))
            })?;
        let value = encrypter.decrypt_string(payload).map_err(|_| {
            ConsoleError::new(format!(
                "Unable to decrypt the encrypted environment entry on line {number}."
            ))
        })?;
        entries.push((name, payload, Zeroizing::new(value)));
    }
    Ok(entries)
}

fn raw_entries(contents: &str) -> Vec<(&str, &str)> {
    let mut entries = Vec::new();
    let mut pending: Option<(&str, usize)> = None;
    let mut start = 0;
    for line in contents.split_inclusive('\n') {
        let end = start + line.trim_end_matches('\n').len();
        let text = &contents[start..end];
        if let Some((name, value_start)) = pending {
            if text == "\"" || quotes(text) != 0 {
                entries.push((name, &contents[value_start..end]));
                pending = None;
            }
        } else if !ignored(text)
            && let Some((name, value)) = text.split_once('=').filter(|(name, _)| !name.is_empty())
        {
            if value.starts_with('"') && quotes(text) == 1 {
                pending = Some((name, start + name.len() + 1));
            } else {
                entries.push((name, value));
            }
        }
        start += line.len();
    }
    entries
}

fn quotes(text: &str) -> usize {
    let mut escaped = false;
    let mut count = 0;
    for byte in text.bytes() {
        if byte == b'"' && !escaped {
            count += 1;
        }
        escaped = byte == b'\\' && !escaped;
    }
    count
}

fn ignored(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.starts_with('#')
}

fn append(out: &mut String, name: &str, payload: &str) {
    out.push_str(name);
    out.push('=');
    out.push_str(payload);
    out.push('\n');
}
