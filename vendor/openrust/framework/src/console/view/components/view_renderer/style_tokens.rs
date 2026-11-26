// 第三方许可见 ../LICENSE.renderer。
/// 保留原类名，避免响应式前缀改变依赖原始标记的布局判定。
pub(super) struct Token<'a> {
    pub original: &'a str,
    pub resolved: Option<&'a str>,
}
pub(super) fn tokens(input: &str, width: i64, include_inactive: bool) -> Vec<Token<'_>> {
    // 对应“空白后，在下一个左方括号前没有右方括号”；反向扫描避免重复搜索。
    let mut closing = None;
    let mut opening = None;
    let mut boundaries = vec![];
    for (index, byte) in input.bytes().enumerate().rev() {
        match byte {
            b'[' => opening = Some(index),
            b']' => closing = Some(index),
            _ => {}
        }
        if byte.is_ascii_whitespace()
            && closing.is_none_or(|end| opening.is_some_and(|start| end >= start))
        {
            boundaries.push(index);
        }
    }
    let mut start = 0;
    let mut values = vec![];
    for end in boundaries
        .into_iter()
        .rev()
        .chain(std::iter::once(input.len()))
    {
        let original = &input[start..end];
        start = end + 1;
        if original.is_empty() || original == "0" {
            continue;
        }
        let (rank, resolved, active) = match original.split_once(':') {
            Some(("sm", value)) => (1, value, width >= 64),
            Some(("md", value)) => (2, value, width >= 76),
            Some(("lg", value)) => (3, value, width >= 102),
            Some(("xl", value)) => (4, value, width >= 128),
            Some(("2xl", value)) => (5, value, width >= 153),
            _ => (0, original, true),
        };
        let resolved = (active && !resolved.is_empty()).then_some(resolved);
        if resolved.is_some() || include_inactive {
            values.push((rank, Token { original, resolved }));
        }
    }
    values.sort_by_key(|(rank, _)| *rank);
    values.into_iter().map(|(_, token)| token).collect()
}
