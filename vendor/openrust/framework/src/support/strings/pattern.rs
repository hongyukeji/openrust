use regex::{Captures, Error, Regex, Replacer};

pub(super) fn first<'a>(pattern: &str, subject: &'a str) -> Result<&'a str, Error> {
    let expression = Regex::new(pattern)?;
    Ok(expression
        .captures(subject)
        .and_then(|captures| captures.get(1).or_else(|| captures.get(0)))
        .map_or("", |found| found.as_str()))
}

pub(super) fn all<'a>(pattern: &str, subject: &'a str) -> Result<Vec<&'a str>, Error> {
    let expression = Regex::new(pattern)?;
    let group = usize::from(expression.captures_len() > 1);
    Ok(expression
        .captures_iter(subject)
        .map(|captures| captures.get(group).map_or("", |found| found.as_str()))
        .collect())
}

pub(super) fn any(patterns: &[&str], subject: &str) -> Result<bool, Error> {
    for pattern in patterns {
        if Regex::new(pattern)?.is_match(subject) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn replace(
    pattern: &str,
    replacement: impl Replacer,
    subject: &str,
    count: Option<usize>,
) -> Result<String, Error> {
    let expression = Regex::new(pattern)?;
    if count == Some(0) {
        return Ok(subject.to_owned());
    }
    Ok(expression
        .replacen(subject, count.unwrap_or(0), replacement)
        .into_owned())
}

pub(super) fn replace_with(
    pattern: &str,
    mut replacement: impl FnMut(&Captures<'_>) -> String,
    subject: &str,
    count: Option<usize>,
) -> Result<String, Error> {
    replace(
        pattern,
        |captures: &Captures<'_>| replacement(captures),
        subject,
        count,
    )
}
