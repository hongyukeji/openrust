use crate::console::ConsoleError;

struct Function<'a> {
    signature: &'a syn::Signature,
    body: &'a syn::Block,
    attributes: &'a [syn::Attribute],
}
fn function<'a>(
    file: &'a syn::File,
    name: &str,
    trait_name: Option<&str>,
) -> Result<Function<'a>, ConsoleError> {
    let mut found = Vec::new();
    for item in &file.items {
        match (item, trait_name) {
            (syn::Item::Fn(value), None) if value.sig.ident == name => found.push(Function {
                signature: &value.sig,
                body: &value.block,
                attributes: &value.attrs,
            }),
            (syn::Item::Impl(value), Some(trait_name))
                if value
                    .trait_
                    .as_ref()
                    .and_then(|(_, path, _)| path.segments.last())
                    .is_some_and(|segment| segment.ident == trait_name) =>
            {
                for item in &value.items {
                    if let syn::ImplItem::Fn(value) = item
                        && value.sig.ident == name
                    {
                        found.push(Function {
                            signature: &value.sig,
                            body: &value.block,
                            attributes: &value.attrs,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    if found.len() != 1 {
        return Err(ConsoleError::new(format!("生成函数 {name} 不存在或不唯一")));
    }
    Ok(found.remove(0))
}
/// 仅替换仍等价于内置模板的生成函数体，保留文件中的其他业务路由、注释与声明。
pub fn replace_generated_function(
    source: &str,
    expected: &str,
    replacement: &str,
    name: &str,
) -> Result<String, ConsoleError> {
    replace_generated(source, expected, replacement, name, None)
}

/// 安全升级已发布的标准 Action；自定义 trait 方法不会被模板覆盖。
pub fn replace_generated_method(
    source: &str,
    expected: &str,
    replacement: &str,
    trait_name: &str,
    name: &str,
) -> Result<String, ConsoleError> {
    replace_generated(source, expected, replacement, name, Some(trait_name))
}
fn replace_generated(
    source: &str,
    expected: &str,
    replacement: &str,
    name: &str,
    trait_name: Option<&str>,
) -> Result<String, ConsoleError> {
    let parse = |source: &str| {
        syn::parse_file(source).map_err(|_| ConsoleError::new("无法解析应用生成源码"))
    };
    let file = parse(source)?;
    let original = parse(expected)?;
    let template = parse(replacement)?;
    let actual = function(&file, name, trait_name)?;
    let stock = function(&original, name, trait_name)?;
    let update = function(&template, name, trait_name)?;
    let body = |function: &Function<'_>| {
        function.body.brace_token.span.open().byte_range().start
            ..function.body.brace_token.span.close().byte_range().end
    };
    if attributes(&actual) != attributes(&stock)
        || actual.signature != stock.signature
        || actual.body != stock.body
    {
        return Err(ConsoleError::new(format!(
            "生成函数 {name} 已自定义，请手动接线；不会覆盖业务代码"
        )));
    }
    if attributes(&update) != attributes(&stock) || update.signature != stock.signature {
        return Err(ConsoleError::new("替换模板的函数签名与原始模板不一致"));
    }
    if actual.body == update.body {
        return Ok(source.to_owned());
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let replacement = replacement[body(&update)]
        .replace("\r\n", "\n")
        .replace('\n', newline);
    let mut updated = source.to_owned();
    updated.replace_range(body(&actual), &replacement);
    Ok(updated)
}

pub(super) fn signature_start(signature: &syn::Signature) -> usize {
    [
        Some(signature.fn_token.span),
        signature.constness.as_ref().map(|token| token.span),
        signature.asyncness.as_ref().map(|token| token.span),
        signature.unsafety.as_ref().map(|token| token.span),
        signature.abi.as_ref().map(|abi| abi.extern_token.span),
    ]
    .into_iter()
    .flatten()
    .map(|span| span.byte_range().start)
    .min()
    .expect("函数必须包含fn标记")
}

fn attributes<'a>(function: &Function<'a>) -> Vec<&'a syn::Attribute> {
    function
        .attributes
        .iter()
        .filter(|attribute| !attribute.path().is_ident("doc"))
        .collect()
}
