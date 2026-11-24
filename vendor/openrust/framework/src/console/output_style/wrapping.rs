use super::super::ConsoleError;

pub(super) fn wrap(text: &str, width: usize) -> Result<Vec<String>, ConsoleError> {
    if width == 0 {
        return Ok(text.split('\n').map(str::to_owned).collect());
    }
    let tag = r"<(?:(?:[a-z](?:[^\\<>]*+ | \\.)*)|/(?:[a-z][^<>]*+)?)>";
    let row = format!(r"(?:{tag}|https?://\S+|.){{1,{width}}}");
    let pattern = format!(
        r"(?:((?>({row})((?<=[^\S\r\n])[^\S\r\n]?|(?=\r?\n)|$|[^\S\r\n]))|({row}))(?:\r?\n)?|(?:\r?\n|$))"
    );
    let regex = pcre2::bytes::RegexBuilder::new()
        .caseless(true)
        .multi_line(true)
        .extended(true)
        .utf(true)
        .ucp(true)
        .build(&pattern)
        .map_err(ConsoleError::from_source)?;
    let mut output = Vec::new();
    let mut cursor = 0;
    for capture in regex.captures_iter(text.as_bytes()) {
        let capture = capture.map_err(ConsoleError::from_source)?;
        let matched = capture.get(0).expect("匹配包含完整范围");
        output.extend_from_slice(&text.as_bytes()[cursor..matched.start()]);
        if let Some(first) = capture.get(1) {
            output.extend_from_slice(first.as_bytes());
        }
        output.push(b'\n');
        cursor = matched.end();
    }
    output.extend_from_slice(&text.as_bytes()[cursor..]);
    let output = String::from_utf8(output).map_err(ConsoleError::from_source)?;
    Ok(output
        .trim_end_matches('\n')
        .replace(" \n", "\n")
        .split('\n')
        .map(str::to_owned)
        .collect())
}
