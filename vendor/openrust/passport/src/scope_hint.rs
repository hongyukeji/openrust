//! League scope 提示使用 HTML 4.01 实体规则。输入来自当前 Rust UTF-8 scope 载体。

const HTML401_NAMES: &[&str] = &include!("scope_html401_entities.rs");

pub(super) fn escape(scope: &str) -> String {
    let mut escaped = String::with_capacity(scope.len());
    for (index, character) in scope.char_indices() {
        match character {
            '&' if !has_entity(&scope[index + 1..]) => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#039;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn has_entity(text: &str) -> bool {
    let numeric = text.strip_prefix('#');
    let body = numeric.unwrap_or(text);
    let Some(end) = body.bytes().position(|byte| !byte.is_ascii_alphanumeric()) else {
        return false;
    };
    if body.as_bytes()[end] != b';' {
        return false;
    }
    let name = &body[..end];
    if numeric.is_none() {
        return HTML401_NAMES.binary_search(&name).is_ok();
    }
    let Some(hex) = name.strip_prefix(['x', 'X']) else {
        return name.parse::<u32>().is_ok_and(|value| value <= 0x10_FFFF);
    };
    // PHP 的十六进制 strtol 也接受数字部分中的 0x 前缀。
    let digits = hex
        .strip_prefix("0x")
        .or_else(|| hex.strip_prefix("0X"))
        .unwrap_or(hex);
    u32::from_str_radix(digits, 16).is_ok_and(|value| value <= 0x10_FFFF)
}

#[cfg(test)]
mod tests {
    use super::super::OAuthError;

    #[test]
    fn scope_hint_keeps_only_valid_html401_entities_and_stays_out_of_diagnostics()
    -> openrust::Result<()> {
        let empty =
            serde_json::to_value(OAuthError::invalid_scope()).map_err(openrust::Error::internal)?;
        assert_eq!(
            empty["hint"],
            "Specify a scope in the request or set a default scope"
        );
        let scope = "示例<>\"'& &copy; &Alpha; &AMP; &apos; &copy &#0; &#xD800; &#x0x41; &#1114111; &#1114112; &#;";
        let error = OAuthError::invalid_scope_for(scope);
        let payload = serde_json::to_value(&error).map_err(openrust::Error::internal)?;
        assert_eq!(
            payload["hint"],
            "Check the `示例&lt;&gt;&quot;&#039;&amp; &copy; &Alpha; &amp;AMP; &amp;apos; &amp;copy &#0; &#xD800; &#x0x41; &#1114111; &amp;#1114112; &amp;#;` scope"
        );
        assert!(!format!("{error:?} {error}").contains("示例"));
        Ok(())
    }
}
