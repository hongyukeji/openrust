use std::fmt;

use serde::Serialize;

use crate::contracts::support::Htmlable;

/// An owned HTML string. Constructing it preserves the supplied markup.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HtmlString(String);

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl HtmlString {
    pub fn new(html: impl Into<String>) -> Self {
        Self(html.into())
    }

    pub fn toHtml(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_not_empty(&self) -> bool {
        !self.is_empty()
    }
}

impl Htmlable for HtmlString {
    fn toHtml(&self) -> &str {
        self.toHtml()
    }
}

impl fmt::Display for HtmlString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.toHtml())
    }
}

pub(crate) fn escape_html(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#039;"),
            other => output.push(other),
        }
    }
}

/// 固定 PHP ENT_QUOTES / HTML401 解码，保留未知名称和非法控制码点。
pub(crate) fn decode_html_entities(value: &str) -> std::borrow::Cow<'_, str> {
    use std::sync::LazyLock;
    static ENTITIES: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"&(?:#[xX][0-9A-Fa-f]+|#[0-9]+|[A-Za-z][A-Za-z0-9]+);")
            .expect("实体匹配表达式")
    });
    ENTITIES.replace_all(value, |capture: &regex::Captures<'_>| {
        let entity = &capture[0];
        if let Some(number) = entity.strip_prefix("&#") {
            let number = &number[..number.len() - 1];
            let code = if let Some(hex) = number.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                number.parse().ok()
            };
            return code
                .filter(|code| {
                    matches!(code, 9 | 10 | 13) || (*code >= 32 && !(127..=159).contains(code))
                })
                .and_then(char::from_u32)
                .map_or_else(|| entity.to_owned(), |code| code.to_string());
        }
        if HTML401_ENTITIES
            .binary_search(&&entity[1..entity.len() - 1])
            .is_err()
        {
            return entity.to_owned();
        }
        // HTML401 的尖括号码点与 HTML5 命名实体表不同。
        match entity {
            "&lang;" => return "\u{2329}".to_owned(),
            "&rang;" => return "\u{232a}".to_owned(),
            _ => {}
        }
        web_atoms::NAMED_ENTITIES.get(&entity[1..]).map_or_else(
            || entity.to_owned(),
            |&(first, second)| {
                [first, second]
                    .into_iter()
                    .filter(|&code| code != 0)
                    .filter_map(char::from_u32)
                    .collect()
            },
        )
    })
}

// 名称由 tests/reference/test-response.php 的 PHP HTML401 翻译表生成。
const HTML401_ENTITIES: &[&str] = &[
    "AElig", "Aacute", "Acirc", "Agrave", "Alpha", "Aring", "Atilde", "Auml", "Beta", "Ccedil",
    "Chi", "Dagger", "Delta", "ETH", "Eacute", "Ecirc", "Egrave", "Epsilon", "Eta", "Euml",
    "Gamma", "Iacute", "Icirc", "Igrave", "Iota", "Iuml", "Kappa", "Lambda", "Mu", "Ntilde", "Nu",
    "OElig", "Oacute", "Ocirc", "Ograve", "Omega", "Omicron", "Oslash", "Otilde", "Ouml", "Phi",
    "Pi", "Prime", "Psi", "Rho", "Scaron", "Sigma", "THORN", "Tau", "Theta", "Uacute", "Ucirc",
    "Ugrave", "Upsilon", "Uuml", "Xi", "Yacute", "Yuml", "Zeta", "aacute", "acirc", "acute",
    "aelig", "agrave", "alefsym", "alpha", "amp", "and", "ang", "aring", "asymp", "atilde", "auml",
    "bdquo", "beta", "brvbar", "bull", "cap", "ccedil", "cedil", "cent", "chi", "circ", "clubs",
    "cong", "copy", "crarr", "cup", "curren", "dArr", "dagger", "darr", "deg", "delta", "diams",
    "divide", "eacute", "ecirc", "egrave", "empty", "emsp", "ensp", "epsilon", "equiv", "eta",
    "eth", "euml", "euro", "exist", "fnof", "forall", "frac12", "frac14", "frac34", "frasl",
    "gamma", "ge", "gt", "hArr", "harr", "hearts", "hellip", "iacute", "icirc", "iexcl", "igrave",
    "image", "infin", "int", "iota", "iquest", "isin", "iuml", "kappa", "lArr", "lambda", "lang",
    "laquo", "larr", "lceil", "ldquo", "le", "lfloor", "lowast", "loz", "lrm", "lsaquo", "lsquo",
    "lt", "macr", "mdash", "micro", "middot", "minus", "mu", "nabla", "nbsp", "ndash", "ne", "ni",
    "not", "notin", "nsub", "ntilde", "nu", "oacute", "ocirc", "oelig", "ograve", "oline", "omega",
    "omicron", "oplus", "or", "ordf", "ordm", "oslash", "otilde", "otimes", "ouml", "para", "part",
    "permil", "perp", "phi", "pi", "piv", "plusmn", "pound", "prime", "prod", "prop", "psi",
    "quot", "rArr", "radic", "rang", "raquo", "rarr", "rceil", "rdquo", "real", "reg", "rfloor",
    "rho", "rlm", "rsaquo", "rsquo", "sbquo", "scaron", "sdot", "sect", "shy", "sigma", "sigmaf",
    "sim", "spades", "sub", "sube", "sum", "sup", "sup1", "sup2", "sup3", "supe", "szlig", "tau",
    "there4", "theta", "thetasym", "thinsp", "thorn", "tilde", "times", "trade", "uArr", "uacute",
    "uarr", "ucirc", "ugrave", "uml", "upsih", "upsilon", "uuml", "weierp", "xi", "yacute", "yen",
    "yuml", "zeta", "zwj", "zwnj",
];
