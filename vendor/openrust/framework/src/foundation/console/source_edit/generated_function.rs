use crate::console::ConsoleError;

fn function<'a>(file: &'a syn::File, name: &str) -> Result<&'a syn::ItemFn, ConsoleError> {
    let mut found = file.items.iter().filter_map(|item| match item {
        syn::Item::Fn(value) if value.sig.ident == name => Some(value),
        _ => None,
    });
    let result = found
        .next()
        .ok_or_else(|| ConsoleError::new(format!("未找到生成函数 {name}")))?;
    if found.next().is_some() {
        return Err(ConsoleError::new(format!("生成函数 {name} 不唯一")));
    }
    Ok(result)
}
fn tokens(source: &str) -> Result<String, ConsoleError> {
    source
        .parse::<proc_macro2::TokenStream>()
        .map(|value| value.to_string())
        .map_err(|_| ConsoleError::new("无法解析生成函数的Rust标记"))
}

/// 仅替换仍等价于内置模板的生成函数体，保留文件中的其他业务路由、注释与声明。
/// 自定义签名或函数体一律拒绝，由应用明确完成手动接线。
pub fn replace_generated_function(
    source: &str,
    expected: &str,
    replacement: &str,
    name: &str,
) -> Result<String, ConsoleError> {
    let parse = |source: &str| {
        syn::parse_file(source).map_err(|_| ConsoleError::new("无法解析应用生成源码"))
    };
    let file = parse(source)?;
    let original = parse(expected)?;
    let template = parse(replacement)?;
    let actual = function(&file, name)?;
    let stock = function(&original, name)?;
    let update = function(&template, name)?;
    let signature = |source: &str, function: &syn::ItemFn| {
        tokens(
            &source[function.sig.fn_token.span.byte_range().start
                ..function.block.brace_token.span.open().byte_range().start],
        )
    };
    let body = |function: &syn::ItemFn| {
        function.block.brace_token.span.open().byte_range().start
            ..function.block.brace_token.span.close().byte_range().end
    };
    if signature(source, actual)? != signature(expected, stock)?
        || tokens(&source[body(actual)])? != tokens(&expected[body(stock)])?
    {
        return Err(ConsoleError::new(format!(
            "生成函数 {name} 已自定义，请手动接入用户资源；不会覆盖业务代码"
        )));
    }
    if signature(replacement, update)? != signature(expected, stock)? {
        return Err(ConsoleError::new("替换模板的函数签名与原始模板不一致"));
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let replacement = replacement[body(update)]
        .replace("\r\n", "\n")
        .replace('\n', newline);
    let mut updated = source.to_owned();
    updated.replace_range(body(actual), &replacement);
    Ok(updated)
}
