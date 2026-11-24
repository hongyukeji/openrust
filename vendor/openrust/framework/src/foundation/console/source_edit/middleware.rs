use crate::console::ConsoleError;

fn methods<'a>(expression: &'a syn::Expr, name: &str, found: &mut Vec<&'a syn::ExprMethodCall>) {
    match expression {
        syn::Expr::MethodCall(call) => {
            if call.method == name {
                found.push(call);
            }
            methods(&call.receiver, name, found);
            for argument in &call.args {
                methods(argument, name, found);
            }
        }
        syn::Expr::Block(value) => statements(&value.block, name, found),
        syn::Expr::Try(value) => methods(&value.expr, name, found),
        syn::Expr::Return(value) => {
            if let Some(value) = &value.expr {
                methods(value, name, found);
            }
        }
        syn::Expr::Closure(value) => methods(&value.body, name, found),
        syn::Expr::If(value) => {
            methods(&value.cond, name, found);
            statements(&value.then_branch, name, found);
            if let Some((_, otherwise)) = &value.else_branch {
                methods(otherwise, name, found);
            }
        }
        _ => {}
    }
}
fn statements<'a>(block: &'a syn::Block, name: &str, found: &mut Vec<&'a syn::ExprMethodCall>) {
    for statement in &block.stmts {
        match statement {
            syn::Stmt::Expr(expression, _) => methods(expression, name, found),
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    methods(&init.expr, name, found);
                }
            }
            _ => {}
        }
    }
}

/// 在已有中间件闭包中显式启用第一方SPA，保留其他配置和关闭Sanctum时的可编译性。
/// 不识别的手动装配返回错误；调用者可在写入其他安装文件前预检。
pub fn enable_stateful_api(source: &str) -> Result<String, ConsoleError> {
    let invalid = || {
        ConsoleError::new(
            "无法安全接入第一方 SPA：请在 bootstrap/app.rs 的 with_middleware 闭包中显式调用 middleware.stateful_api()，再重试安装",
        )
    };
    let file = syn::parse_file(source).map_err(|_| invalid())?;
    let mut found = Vec::new();
    for item in &file.items {
        if let syn::Item::Fn(function) = item {
            statements(&function.block, "with_middleware", &mut found);
        }
    }
    if found.len() != 1 {
        return Err(invalid());
    }
    let Some(syn::Expr::Closure(closure)) = found[0].args.first() else {
        return Err(invalid());
    };
    if closure.inputs.len() != 1 {
        return Err(invalid());
    }
    let parameter = match closure.inputs.first() {
        Some(syn::Pat::Ident(parameter)) => parameter,
        Some(syn::Pat::Type(typed)) => match typed.pat.as_ref() {
            syn::Pat::Ident(parameter) => parameter,
            _ => return Err(invalid()),
        },
        _ => return Err(invalid()),
    };
    let syn::Expr::Block(body) = closure.body.as_ref() else {
        return Err(invalid());
    };
    let mut existing = Vec::new();
    statements(&body.block, "stateful_api", &mut existing);
    if existing.iter().any(|call|matches!(call.receiver.as_ref(),syn::Expr::Path(path) if path.path.is_ident(&parameter.ident))) {
        return Ok(source.to_owned());
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let addition = format!(
        "{newline}            #[cfg(feature = \"sanctum\")]{newline}            {}.stateful_api();{newline}",
        parameter.ident
    );
    let mut updated = source.to_owned();
    updated.insert_str(
        body.block.brace_token.span.open().byte_range().end,
        &addition,
    );
    Ok(updated)
}
