//! 安装器只修改明确识别的 User 接入，保留其他源码及用户定制。
use crate::console::ConsoleError;
use syn::{Expr, Item, Stmt, Type};

pub struct UserModelSource<'a> {
    source: &'a str,
    file: syn::File,
}
fn is_user(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none() && p.path.is_ident("User"))
}
fn returned_static(expr: &Expr) -> Option<&syn::Ident> {
    match expr {
        Expr::Reference(r) => match r.expr.as_ref() {
            Expr::Path(p) => p.path.get_ident(),
            _ => None,
        },
        Expr::Paren(p) => returned_static(&p.expr),
        Expr::Return(r) => r.expr.as_deref().and_then(returned_static),
        _ => None,
    }
}
fn metadata_chain(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall(c) => metadata_chain(&c.receiver),
        Expr::Call(c) => {
            matches!(c.func.as_ref(), Expr::Path(p) if p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().ends_with(&["ModelMetadata".into(), "new".into()]))
        }
        _ => false,
    }
}
fn hidden(expr: &Expr) -> Option<&syn::ExprArray> {
    let Expr::MethodCall(c) = expr else {
        return None;
    };
    if c.method != "hidden" {
        return hidden(&c.receiver);
    }
    if c.args.len() != 1 {
        return None;
    }
    let Expr::Reference(r) = c.args.first()? else {
        return None;
    };
    let Expr::Array(a) = r.expr.as_ref() else {
        return None;
    };
    Some(a)
}
fn unsupported() -> ConsoleError {
    ConsoleError::new("无法安全识别 User 的声明或隐藏字段，请手动完成模型接入。")
}
impl<'a> UserModelSource<'a> {
    pub fn parse(source: &'a str) -> Result<Self, ConsoleError> {
        let tokens: proc_macro2::TokenStream = source
            .parse()
            .map_err(|e| ConsoleError::new(format!("User 源码词法解析失败：{e}")))?;
        let file = syn::parse2(tokens)
            .map_err(|e| ConsoleError::new(format!("User 源码语法解析失败：{e}")))?;
        Ok(Self { source, file })
    }
    pub fn implements(&self, name: &str) -> bool {
        self.file.items.iter().any(|item| matches!(item, Item::Impl(i) if is_user(&i.self_ty)
            && i.trait_.as_ref().is_some_and(|(negative,p,_)| negative.is_none() && p.segments.iter().map(|s|s.ident.to_string()).eq(name.split("::").map(str::to_owned)))))
    }
    pub fn with_clone(&self) -> Result<String, ConsoleError> {
        let mut structs = self.file.items.iter().filter_map(|item| match item {
            Item::Struct(s) if s.ident == "User" => Some(s),
            _ => None,
        });
        let user = structs.next().ok_or_else(unsupported)?;
        if structs.next().is_some() {
            return Err(unsupported());
        }
        let derives_clone = user
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("derive"))
            .any(|a| {
                a.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                )
                .is_ok_and(|paths| {
                    paths.iter().any(|p| {
                        p.is_ident("Clone")
                            || p.segments
                                .iter()
                                .map(|s| s.ident.to_string())
                                .eq(["std", "clone", "Clone"].map(str::to_owned))
                            || p.segments
                                .iter()
                                .map(|s| s.ident.to_string())
                                .eq(["core", "clone", "Clone"].map(str::to_owned))
                    })
                })
            });
        if derives_clone
            || ["Clone", "std::clone::Clone", "core::clone::Clone"]
                .iter()
                .any(|p| self.implements(p))
        {
            return Ok(self.source.into());
        }
        // 不为任意自定义字段推断 Clone，只自动接入已知的 ModelState 包装。
        if !matches!(&user.fields, syn::Fields::Unnamed(f) if f.unnamed.len()==1 && matches!(&f.unnamed[0].ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident=="ModelState")))
        {
            return Err(unsupported());
        }
        let span = match &user.vis {
            syn::Visibility::Public(v) => v.span,
            syn::Visibility::Restricted(v) => v.pub_token.span,
            _ => user.struct_token.span,
        };
        let mut result = self.source.to_owned();
        result.insert_str(span.byte_range().start, "#[derive(Clone)]\n");
        Ok(result)
    }
    pub fn with_hidden(&self, required: &[&str]) -> Result<String, ConsoleError> {
        let mut arrays = Vec::new();
        for item in &self.file.items {
            let Item::Impl(i) = item else { continue };
            if !is_user(&i.self_ty)
                || !i.trait_.as_ref().is_some_and(|(neg, p, _)| {
                    neg.is_none() && p.segments.last().is_some_and(|s| s.ident == "Model")
                })
            {
                continue;
            }
            for item in &i.items {
                let syn::ImplItem::Fn(f) = item else { continue };
                if f.sig.ident != "metadata" {
                    continue;
                }
                let Some(Stmt::Expr(expr, _)) = f.block.stmts.last() else {
                    continue;
                };
                let Some(name) = returned_static(expr) else {
                    continue;
                };
                for stmt in &f.block.stmts {
                    let Stmt::Item(Item::Static(s)) = stmt else {
                        continue;
                    };
                    if s.ident == *name
                        && matches!(&*s.ty, Type::Path(p) if p.path.segments.last().is_some_and(|s|s.ident=="ModelMetadata"))
                        && metadata_chain(&s.expr)
                        && let Some(array) = hidden(&s.expr)
                    {
                        arrays.push(array);
                    }
                }
            }
        }
        if arrays.len() != 1 {
            return Err(unsupported());
        }
        let array = arrays[0];
        let missing:Vec<_>=required.iter().filter(|name| !array.elems.iter().any(|e| matches!(e,Expr::Lit(l) if matches!(&l.lit,syn::Lit::Str(s) if s.value()==**name)))).collect();
        if missing.is_empty() {
            return Ok(self.source.into());
        }
        let mut addition = String::new();
        if !array.elems.is_empty() && !array.elems.trailing_punct() {
            addition.push(',');
        }
        for name in missing {
            addition.push_str(&format!(" {name:?},"));
        }
        let mut result = self.source.to_owned();
        result.insert_str(
            array.bracket_token.span.close().byte_range().start,
            &addition,
        );
        Ok(result)
    }
}
