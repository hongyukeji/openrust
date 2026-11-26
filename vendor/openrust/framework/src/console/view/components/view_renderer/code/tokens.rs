// 组件许可见 ../../LICENSE.renderer；词法依赖许可见 ../../LICENSE.lexer。
use crate::console::ConsoleError;
use mago_syntax::{lexer::Lexer, settings::LexerSettings, token::TokenKind};
use mago_syntax_core::input::Input;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Category {
    Default,
    Comment,
    String,
    Html,
    Keyword,
}
impl Category {
    pub(super) fn class(self) -> &'static str {
        match self {
            Self::Default => "strong",
            Self::Comment => "text-gray italic",
            Self::String => "text-gray",
            Self::Html => "text-blue strong",
            Self::Keyword => "text-magenta strong",
        }
    }
}
fn category(kind: TokenKind, previous: Category) -> Category {
    use TokenKind::*;
    match kind {
        Whitespace => previous,
        OpenTag | EchoTag | ShortOpenTag | CloseTag | Identifier | Variable | DirConstant
        | FileConstant | MethodConstant | LiteralFloat | LiteralInteger | NamespaceConstant
        | LineConstant | ClassConstant | FunctionConstant | TraitConstant | Self_ | Parent
        | True | False | Null | From => Category::Default,
        SingleLineComment | HashComment | MultiLineComment | DocBlockComment => Category::Comment,
        LiteralString | PartialLiteralString | StringPart | DoubleQuote => Category::String,
        InlineText | InlineShebang => Category::Html,
        _ => Category::Keyword,
    }
}
fn label(value: &str) -> bool {
    let b = value.as_bytes();
    !b.is_empty()
        && (b[0].is_ascii_alphabetic() || b[0] == b'_' || b[0] >= 128)
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c >= 128)
}
/// 查看源规则允许的空白/注释后第一个令牌，复用成熟词法器而不自行扫描注释。
fn following_word(source: &str) -> Option<(usize, &str)> {
    let mut lexer = Lexer::scripting(
        Input::new(mago_database::file::FileId::zero(), source.as_bytes()),
        LexerSettings {
            enable_short_tags: true,
        },
    );
    let mut separator = false;
    while let Some(Ok(token)) = lexer.advance() {
        if matches!(
            token.kind,
            TokenKind::Whitespace
                | TokenKind::SingleLineComment
                | TokenKind::HashComment
                | TokenKind::MultiLineComment
                | TokenKind::DocBlockComment
        ) {
            separator = true;
            continue;
        }
        return separator.then_some((token.start.offset as usize, token.value));
    }
    None
}
fn yield_from_length(source: &str) -> Option<usize> {
    let (start, word) = following_word(source)?;
    if word
        .get(..4)
        .is_some_and(|p| p.eq_ignore_ascii_case("from"))
        && word
            .as_bytes()
            .get(4)
            .is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_' && *c < 128)
    {
        Some(start + 4)
    } else {
        None
    }
}
fn enum_keyword(source: &str) -> bool {
    following_word(source).is_some_and(|(_, word)| {
        word.as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_' || *c >= 128)
            && !word
                .get(..7)
                .is_some_and(|p| p.eq_ignore_ascii_case("extends"))
            && !word
                .get(..10)
                .is_some_and(|p| p.eq_ignore_ascii_case("implements"))
    })
}
/// 只替换特殊关键字的词法占位；保持字节长度和精确大小写的对应关系。
fn lexical_source(source: &str) -> std::borrow::Cow<'_, str> {
    static HALT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new("(?i-u:__halt_compiler)").expect("合法代码关键字表达式")
    });
    if !HALT.is_match(source) {
        return std::borrow::Cow::Borrowed(source);
    }
    static MARKER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"__code_([0-9a-f]{8})").expect("合法代码占位表达式")
    });
    let mut occupied: std::collections::HashSet<u32> = MARKER
        .captures_iter(source)
        .filter_map(|c| u32::from_str_radix(&c[1], 16).ok())
        .collect();
    let mut aliases = std::collections::HashMap::<String, String>::new();
    let mut next = 0_u32;
    HALT.replace_all(source, |capture: &regex::Captures<'_>| {
        let key = &capture[0];
        if let Some(value) = aliases.get(key) {
            return value.clone();
        }
        while occupied.contains(&next) {
            next = next.wrapping_add(1);
        }
        let marker = format!("__code_{next:08x}");
        occupied.insert(next);
        next = next.wrapping_add(1);
        debug_assert_eq!(marker.len(), key.len());
        aliases.insert(key.to_owned(), marker.clone());
        marker
    })
}
pub(super) fn groups(source: &str) -> Result<Vec<(Category, String)>, ConsoleError> {
    let settings = LexerSettings {
        enable_short_tags: true,
    };
    if source.len() > u32::MAX as usize {
        return Err(ConsoleError::new("代码片段超过词法位置范围"));
    }
    let annotated = lexical_source(source);
    let mut lexer = Lexer::new(
        Input::new(mago_database::file::FileId::zero(), annotated.as_bytes()),
        settings,
    );
    let mut result: Vec<(Category, String)> = vec![];
    let mut previous = Category::Keyword;
    let mut last_token = None;
    let mut property = false;
    let mut halt = None;
    let mut pending = std::collections::VecDeque::new();
    let mut strings: Vec<(TokenKind, usize)> = vec![];
    loop {
        let start = lexer.current_position().offset as usize;
        let Some(token) = pending.pop_front().map(Ok).or_else(|| lexer.advance()) else {
            break;
        };
        let (mut kind, value, raw, end) = match token {
            Ok(token) => {
                let mut end = token.start.offset as usize + token.value.len();
                if token.kind == TokenKind::Yield
                    && !property
                    && strings.last().is_none_or(|(_, depth)| *depth > 0)
                    && let Some(length) = yield_from_length(&source[end..])
                {
                    let composite_end = end + length;
                    while end < composite_end {
                        let Some(Ok(next)) = lexer.advance() else {
                            return Err(ConsoleError::new("代码复合令牌无法读取"));
                        };
                        end = next.start.offset as usize + next.value.len();
                        if end > composite_end {
                            let skip = composite_end - next.start.offset as usize;
                            pending.push_front(mago_syntax::token::Token {
                                kind: TokenKind::FullyQualifiedIdentifier,
                                start: next.start.forward(skip as u32),
                                value: &next.value[skip..],
                            });
                        }
                    }
                    end = composite_end;
                }
                let value = source
                    .get(token.start.offset as usize..end)
                    .ok_or_else(|| ConsoleError::new("代码令牌字节位置无效"))?;
                let mut kind = category(token.kind, previous);
                if token.kind == TokenKind::Enum && !enum_keyword(&source[end..]) {
                    kind = Category::Default;
                }
                if property && label(value) {
                    kind = Category::Default;
                }
                if last_token == Some(TokenKind::DollarLeftBrace)
                    && label(value)
                    && source
                        .as_bytes()
                        .get(end)
                        .is_some_and(|c| matches!(c, b'[' | b'}'))
                {
                    kind = Category::Keyword;
                }
                if token.kind == TokenKind::LiteralInteger
                    && strings.last().is_some_and(|(_, depth)| *depth == 0)
                {
                    kind = Category::Keyword;
                }
                (kind, value, Some(token.kind), end)
            }
            Err(error) => {
                let end = lexer.current_position().offset as usize;
                let text = source
                    .get(start..end)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ConsoleError::new(format!("代码词法处理失败：{error:?}")))?;
                let kind = if text.starts_with("/*") {
                    Category::Comment
                } else if text.as_bytes()[0].is_ascii_digit() {
                    Category::Default
                } else if matches!(
                    error,
                    mago_syntax::error::SyntaxError::UnrecognizedToken(..)
                ) {
                    Category::Keyword
                } else {
                    return Err(ConsoleError::new(format!("代码词法处理失败：{error:?}")));
                };
                (kind, text, None, end)
            }
        };
        let var_name = last_token == Some(TokenKind::DollarLeftBrace)
            && label(value)
            && source
                .as_bytes()
                .get(end)
                .is_some_and(|c| matches!(c, b'[' | b'}'));
        let script_word =
            !property && !var_name && strings.last().is_none_or(|(_, depth)| *depth > 0);
        let source_halt = raw == Some(TokenKind::Identifier)
            && value.eq_ignore_ascii_case("__halt_compiler")
            && script_word;
        if source_halt {
            kind = Category::Keyword;
        }
        if let Some((last, text)) = result.last_mut()
            && *last == kind
        {
            text.push_str(value);
        } else {
            result.push((kind, value.to_owned()));
        }
        previous = kind;
        if let Some(raw) = raw {
            let dropped = matches!(
                raw,
                TokenKind::Whitespace
                    | TokenKind::OpenTag
                    | TokenKind::ShortOpenTag
                    | TokenKind::SingleLineComment
                    | TokenKind::HashComment
                    | TokenKind::MultiLineComment
                    | TokenKind::DocBlockComment
            );
            if let Some(remaining) = &mut halt {
                if !dropped {
                    *remaining -= 1;
                }
                if *remaining == 0 {
                    if end < source.len() {
                        result.push((Category::Html, source[end..].to_owned()));
                    }
                    break;
                }
            } else if source_halt {
                halt = Some(3);
            }
            if !matches!(
                raw,
                TokenKind::Whitespace
                    | TokenKind::SingleLineComment
                    | TokenKind::HashComment
                    | TokenKind::MultiLineComment
                    | TokenKind::DocBlockComment
            ) {
                property = matches!(
                    raw,
                    TokenKind::MinusGreaterThan | TokenKind::QuestionMinusGreaterThan
                );
            }
            match raw {
                TokenKind::DoubleQuote | TokenKind::Backtick => {
                    if strings
                        .last()
                        .is_some_and(|(delimiter, depth)| *delimiter == raw && *depth == 0)
                    {
                        strings.pop();
                    } else {
                        strings.push((raw, 0));
                    }
                }
                TokenKind::DocumentStart(_) => strings.push((raw, 0)),
                TokenKind::DocumentEnd => {
                    strings.pop();
                }
                TokenKind::DollarLeftBrace | TokenKind::LeftBrace => {
                    if let Some((_, depth)) = strings.last_mut() {
                        *depth += 1;
                    }
                }
                TokenKind::RightBrace => {
                    if let Some((_, depth)) = strings.last_mut() {
                        *depth = depth.saturating_sub(1);
                    }
                }
                _ => {}
            }
            last_token = Some(raw);
        } else {
            last_token = None;
            property = false;
        }
    }
    Ok(result)
}
