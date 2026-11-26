//! 分层回归专用的 Rust 语法桥接；不暴露框架运行期接口。
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use serde_json::{Value, json};
use std::ops::Range;
use syn::{Expr, Item, Stmt, Type};

const SENSITIVE: [&str; 2] = ["two_factor_secret", "two_factor_recovery_codes"];

fn path_name(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.segments.last().is_some_and(|s| s.ident == name))
}
fn returned_static(expr: &Expr) -> Option<&syn::Ident> {
    match expr {
        Expr::Reference(reference) => match reference.expr.as_ref() {
            Expr::Path(path) => path.path.get_ident(),
            _ => None,
        },
        Expr::Paren(paren) => returned_static(&paren.expr),
        Expr::Return(value) => value.expr.as_deref().and_then(returned_static),
        _ => None,
    }
}
fn metadata_chain(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall(call) => metadata_chain(&call.receiver),
        Expr::Call(call) => matches!(call.func.as_ref(), Expr::Path(path)
            if path.path.segments.len() >= 2
            && path.path.segments.last().is_some_and(|s| s.ident == "new")
            && path.path.segments.iter().rev().nth(1).is_some_and(|s| s.ident == "ModelMetadata")),
        _ => false,
    }
}
fn effective_hidden(expr: &Expr) -> Option<&syn::ExprArray> {
    let Expr::MethodCall(call) = expr else {
        return None;
    };
    if call.method == "hidden" {
        if call.args.len() != 1 {
            return None;
        }
        let Expr::Reference(reference) = call.args.first()? else {
            return None;
        };
        let Expr::Array(array) = reference.expr.as_ref() else {
            return None;
        };
        return Some(array);
    }
    effective_hidden(&call.receiver)
}
fn user_hidden(file: &syn::File) -> Option<&syn::ExprArray> {
    for item in &file.items {
        let Item::Impl(model) = item else { continue };
        if !path_name(&model.self_ty, "User")
            || !model.trait_.as_ref().is_some_and(|(_, path, _)| {
                path.segments.last().is_some_and(|s| s.ident == "Model")
            })
        {
            continue;
        }
        for item in &model.items {
            let syn::ImplItem::Fn(method) = item else {
                continue;
            };
            if method.sig.ident != "metadata" {
                continue;
            }
            let Some(Stmt::Expr(result, _)) = method.block.stmts.last() else {
                continue;
            };
            let Some(name) = returned_static(result) else {
                continue;
            };
            for stmt in &method.block.stmts {
                let Stmt::Item(Item::Static(value)) = stmt else {
                    continue;
                };
                if value.ident == *name
                    && path_name(&value.ty, "ModelMetadata")
                    && metadata_chain(&value.expr)
                {
                    return effective_hidden(&value.expr);
                }
            }
        }
    }
    None
}
fn secret(segment: &[TokenTree]) -> bool {
    let [TokenTree::Literal(literal)] = segment else {
        return false;
    };
    syn::parse_str::<syn::LitStr>(&literal.to_string())
        .is_ok_and(|value| SENSITIVE.contains(&value.value().as_str()))
}
fn flatten(stream: TokenStream, literals: bool, hidden: Option<&Range<usize>>) -> Vec<String> {
    let mut result = Vec::new();
    for token in stream {
        match token {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                if !open.is_empty() {
                    result.push(open.into());
                }
                if hidden.is_some_and(|range| *range == group.span().byte_range()) {
                    // 仅规范化实际返回的 User 元数据中有效 hidden 数组，其他代码逐 token 保留。
                    let mut segments = vec![Vec::new()];
                    for child in group.stream() {
                        if matches!(&child, TokenTree::Punct(p) if p.as_char() == ',') {
                            segments.push(Vec::new());
                        } else {
                            segments.last_mut().unwrap().push(child);
                        }
                    }
                    let mut first = true;
                    for segment in segments
                        .into_iter()
                        .filter(|part| !part.is_empty() && !secret(part))
                    {
                        if !first {
                            result.push(",".into());
                        }
                        first = false;
                        result.extend(flatten(segment.into_iter().collect(), literals, None));
                    }
                } else {
                    result.extend(flatten(group.stream(), literals, hidden));
                }
                if !close.is_empty() {
                    result.push(close.into());
                }
            }
            TokenTree::Literal(literal) if literals => result.push(literal.to_string()),
            TokenTree::Literal(_) => {}
            other => result.push(other.to_string()),
        }
    }
    result
}
fn analyze(source: &str, parse_file: bool) -> Result<Value, String> {
    let stream: TokenStream = source
        .parse()
        .map_err(|error: proc_macro2::LexError| error.to_string())?;
    let file = if parse_file {
        Some(syn::parse2::<syn::File>(stream.clone()).map_err(|error| error.to_string())?)
    } else {
        None
    };
    let hidden = file.as_ref().and_then(user_hidden);
    let range = hidden.map(|array| {
        array.bracket_token.span.open().byte_range().start
            ..array.bracket_token.span.close().byte_range().end
    });
    let names: Vec<_> = hidden
        .into_iter()
        .flat_map(|array| array.elems.iter())
        .filter_map(|value| match value {
            Expr::Lit(value) => match &value.lit {
                syn::Lit::Str(value) => Some(value.value()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    Ok(
        json!({"tokens":flatten(stream.clone(), true, None), "code_tokens":flatten(stream.clone(), false, None),
        "normalized":flatten(stream, true, range.as_ref()), "hidden":names, "hidden_found":range.is_some()}),
    )
}

#[test]
fn parse_request() {
    if std::env::var("OPENRUST_MODEL_LAYERING_PARSE").as_deref() != Ok("1") {
        assert_eq!(
            analyze(
                "let quote = '\"'; /* 外层 /* 内层 */ 注释 */ Db::connection(None);",
                false
            )
            .unwrap()["code_tokens"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|v| **v == "Db")
                .count(),
            1
        );
        return;
    }
    use std::io::Read as _;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let response = serde_json::from_str::<Value>(&input)
        .map_err(|error| error.to_string())
        .and_then(|request| {
            analyze(
                request["source"].as_str().ok_or("缺少源码")?,
                request["parse_file"].as_bool().unwrap_or(false),
            )
        });
    let value = response.unwrap_or_else(|error| json!({"error":error}));
    println!("MODEL_LAYERING_JSON {value}");
}

#[test]
fn syntax_and_hidden_scope_use_rust_ast() {
    let source = r#"struct User; impl Model for User { fn metadata() -> &'static ModelMetadata {
        static OTHER: ModelMetadata = ModelMetadata::new("other").hidden(&["unused"]);
        static METADATA: ModelMetadata = ModelMetadata::new("users").hidden(&["password", "two_factor_secret", "two_factor_recovery_codes"]);
        &METADATA
    }}"#;
    let value = analyze(source, true).unwrap();
    assert_eq!(
        value["hidden"],
        json!(["password", "two_factor_secret", "two_factor_recovery_codes"])
    );
    let normalized = value["normalized"].as_array().unwrap();
    assert!(!normalized.contains(&json!("\"two_factor_secret\"")));
    assert!(normalized.contains(&json!("\"unused\"")));
    let wrong = source.replace(".hidden(&[\"password\",", ".fillable(&[\"password\",");
    assert_eq!(analyze(&wrong, true).unwrap()["hidden_found"], false);
    let overridden = source.replace("&METADATA", "&OTHER");
    assert_eq!(
        analyze(&overridden, true).unwrap()["hidden"],
        json!(["unused"])
    );
}
