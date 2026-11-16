use super::escape_html;

pub(super) fn escape_preserving_entities(output: &mut String, mut text: &str) {
    while let Some(index) = text.find('&') {
        escape_html(output, &text[..index]);
        let rest = &text[index + 1..];
        if let Some(length) = entity_length(rest) {
            output.push_str(&text[index..index + 1 + length]);
            text = &rest[length..];
        } else {
            output.push_str("&amp;");
            text = rest;
        }
    }
    escape_html(output, text);
}

fn entity_length(text: &str) -> Option<usize> {
    let length = text
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'#')
        .count();
    if text.as_bytes().get(length) != Some(&b';') {
        return None;
    }
    let name = &text[..length];
    let valid = if let Some(number) = name.strip_prefix('#') {
        let (digits, radix) = match number.strip_prefix(['x', 'X']) {
            Some(digits) => (digits, 16),
            None => (number, 10),
        };
        u32::from_str_radix(digits, radix).is_ok_and(|code| code <= 0x10_FFFF)
    } else {
        HTML4_ENTITIES.binary_search(&name).is_ok()
    };
    valid.then_some(length + 1)
}

// HTML 4.01 named character references accepted by Laravel's default e() helper.
const HTML4_ENTITIES: &[&str] = &[
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
