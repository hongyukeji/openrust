// Port of Egulias EmailValidator 4.0.4's lexer and RFC parser (MIT).
// 普通模式接受警告；严格模式复用同一解析过程并拒绝警告。
use std::sync::LazyLock;

use icu_normalizer::uts46::Uts46MapperBorrowed;
use regex::Regex;

const EMPTY: i32 = -1;
const GENERIC: i32 = 300;
const IPV6: i32 = 301;
const CRLF: i32 = 1310;
const DOUBLE_COLON: i32 = 5858;

static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i:[a-zA-Z]+[46]?|[^\x00-\x7F]|[0-9]+|\r\n|::|\s+?|.)")
        .expect("fixed email lexer expression")
});
static INVALID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[\p{S}\p{C}]+\z").expect("fixed email token categories"));

#[derive(Clone, Copy)]
struct Token<'a> {
    kind: i32,
    value: &'a str,
}

const NULL: Token<'static> = Token {
    kind: EMPTY,
    value: "",
};

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    index: usize,
    warnings: bool,
}

pub(super) fn passes(value: &str) -> bool {
    validate(value, false)
}
pub(super) fn strict(value: &str) -> bool {
    validate(value, true)
}
fn validate(value: &str, strict: bool) -> bool {
    let mut tokens = Vec::new();
    for found in TOKENS.find_iter(value) {
        let value = found.as_str();
        let kind = match value {
            "IPv6" => IPV6,
            "::" => DOUBLE_COLON,
            "\r\n" => CRLF,
            "¿" => 168,
            "¡" => 173,
            _ if value.len() == 1
                && (value.as_bytes()[0] == 0
                    || b"{}()<>[]:;@\\/,.'`\"- \t\r\n*!&^$%~|_=+?#"
                        .contains(&value.as_bytes()[0])) =>
            {
                i32::from(value.as_bytes()[0])
            }
            _ if INVALID.is_match(value) => return false,
            _ => GENERIC,
        };
        tokens.push(Token { kind, value });
    }
    let mut parser = Parser {
        tokens,
        index: 0,
        warnings: value.len() > 254,
    };
    parser.current().kind != 64
        && parser.local()
        && parser.domain()
        && (!strict || !parser.warnings)
}

impl Parser<'_> {
    fn current(&self) -> Token<'_> {
        self.tokens.get(self.index).copied().unwrap_or(NULL)
    }

    fn next(&self) -> i32 {
        self.tokens.get(self.index + 1).map_or(EMPTY, |t| t.kind)
    }

    fn previous(&self) -> i32 {
        self.index
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map_or(EMPTY, |t| t.kind)
    }

    fn advance(&mut self) {
        self.index += 1;
    }

    fn find(&self, kind: i32) -> bool {
        self.tokens
            .iter()
            .skip(self.index + 1)
            .any(|t| t.kind == kind)
    }

    fn escaped(&self) -> bool {
        self.previous() == 92 && self.current().kind != GENERIC
    }

    fn folding(&mut self) -> bool {
        let kind = self.current().kind;
        if self.escaped() || !matches!(kind, 32 | 9 | 13 | 10 | CRLF) {
            return true;
        }
        self.warnings = true;
        !(kind == CRLF && !matches!(self.next(), 32 | 9)
            || matches!(kind, 13 | 10 | 0)
            || self.next() == GENERIC && self.previous() != 64)
    }

    fn quoted(&mut self) -> bool {
        if !self.check_quote() {
            return false;
        }
        self.advance();
        while !matches!(self.current().kind, 34 | EMPTY) {
            if self.current().kind == 92 && self.next() == 34 {
                self.advance();
            }
            self.advance();
            if !self.escaped() && matches!(self.current().kind, 0 | 9 | 13 | 10) {
                return false;
            }
        }
        if self.previous() == 92 && !self.check_quote() {
            return false;
        }
        self.next() == 64 || self.previous() == 92
    }

    fn check_quote(&mut self) -> bool {
        self.warnings = true;
        !(self.next() == GENERIC && self.previous() == GENERIC) && self.find(34)
    }

    fn comment(&mut self, domain: bool) -> bool {
        self.warnings = true;
        if self.current().kind != 40 || !self.find(41) {
            return false;
        }
        let mut depth = 1;
        loop {
            let end = if domain {
                depth == 0 && self.next() == 46
            } else {
                self.next() == 64
            };
            if end || self.current().kind == EMPTY {
                break;
            }
            match self.next() {
                40 => depth += 1,
                41 => depth -= 1,
                _ => {}
            }
            self.advance();
        }
        depth == 0 && self.next() == if domain { 46 } else { 64 }
    }

    fn local(&mut self) -> bool {
        while !matches!(self.current().kind, 64 | EMPTY) {
            if self.current().kind == 46 && self.previous() == EMPTY {
                return false;
            }
            if self.current().kind == 34 && !self.quoted() {
                return false;
            }
            if matches!(self.current().kind, 40 | 41) && !self.comment(false) {
                return false;
            }
            if self.current().kind == 46 && matches!(self.next(), 46 | 64)
                || self.current().kind == 92 && self.next() == GENERIC
                || matches!(self.current().kind, 44 | 93 | 91 | 62 | 60 | 58 | 59)
                || !self.folding()
            {
                return false;
            }
            self.advance();
        }
        self.warnings |= self.tokens[..self.index]
            .iter()
            .map(|token| token.value.len())
            .sum::<usize>()
            > 64;
        true
    }

    fn domain(&mut self) -> bool {
        self.advance();
        let start = self.index;
        if matches!(self.current().kind, 46 | 45 | EMPTY | 64)
            || self.current().kind == 32 && self.next() != GENERIC
        {
            return false;
        }
        let mut label = String::new();
        let mut comments = false;
        let mut tld_missing = true;
        loop {
            if matches!(self.current().kind, 92 | 47) {
                return false;
            }
            if matches!(self.current().kind, 40 | 41) {
                comments = true;
                if !self.comment(true) {
                    return false;
                }
            }
            if self.current().kind == 46 && self.next() == 46 {
                return false;
            }
            if self.current().kind == 91 {
                if !self.literal() {
                    return false;
                }
                break;
            }
            if self.current().kind == 46 {
                tld_missing &= self.next() != GENERIC;
                if label_too_long(&label) {
                    return false;
                }
                label.clear();
            }
            label.push_str(self.current().value);
            if !self.folding()
                || self.current().kind == 45 && self.next() == 46
                || !(matches!(self.current().kind, GENERIC | 45 | 46)
                    || comments && matches!(self.current().kind, 40 | 41))
            {
                return false;
            }
            self.advance();
            if self.current().kind == EMPTY {
                if label_too_long(&label) {
                    return false;
                }
                break;
            }
        }
        self.warnings |= tld_missing;
        // Recording includes the closing literal bracket and stops there, as upstream.
        !matches!(self.previous(), 46 | 45)
            && self.current().kind != 32
            && self.tokens[start..self.tokens.len().min(self.index + 1)]
                .iter()
                .map(|t| t.value.len())
                .sum::<usize>()
                <= 253
    }

    fn literal(&mut self) -> bool {
        self.warnings = true;
        if !self.find(93) {
            return false;
        }
        loop {
            if self.current().kind == 0
                || matches!(self.next(), 91 | 13)
                || self.current().kind == 92
            {
                return false;
            }
            if self.current().kind == 93 {
                return true;
            }
            self.advance();
            if self.current().kind == EMPTY {
                return true;
            }
        }
    }
}

fn label_too_long(label: &str) -> bool {
    if label.is_ascii() {
        return label.len() > 63;
    }
    // 固定原生Intl 78.1的默认IDNA选项为非过渡处理。
    // 只提升LABEL_TOO_LONG；前导组合标记的错误不应伪装为长度错误。
    let mapped: String = Uts46MapperBorrowed::new()
        .map_normalize(label.chars())
        .collect();
    mapped.split('.').any(|part| {
        if part.is_ascii() {
            part.len() > 63
        } else if part.chars().next().is_some_and(intl::unicode::is_mark) {
            false
        } else {
            // Compute length even when UTS 46 would report another diagnostic.
            idna::punycode::encode_str(part).is_some_and(|encoded| encoded.len() + 4 > 63)
        }
    })
}
