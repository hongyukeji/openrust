use std::{collections::BTreeMap, path::PathBuf};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, generator_command::snake_case,
    },
    contracts::{container::Container as _, foundation::Application as _},
    database::{DatabaseManager, eloquent::FieldModel, schema::SchemaFallback},
    foundation::Application,
};

use super::{model_make_command::validated_parts, source_edit};

const BEGIN: &str = "// openrust:model-fields begin";
const END: &str = "// openrust:model-fields end";
fn generated_section(model: &str) -> String {
    format!(
        "    #[generated(command = {:?})]",
        format!("artisan model:fields {model}")
    )
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ModelFieldsCommand(CommandDefinition::new(
        Syntax::new("model:fields")
            .about("从数据库表结构生成或刷新模型字段引用（扩展）")
            .arg(
                Arg::new("name")
                    .num_args(1..)
                    .required_unless_present("all")
                    .conflicts_with("all"),
            )
            .arg(
                Arg::new("all")
                    .long("all")
                    .action(ArgAction::SetTrue)
                    .help("同步所有已编译并启用自动字段同步的应用模型"),
            )
            .arg(
                Arg::new("check")
                    .long("check")
                    .action(ArgAction::SetTrue)
                    .help("只检查生成结果，过期时返回非零退出码，不写入源码"),
            )
            .arg(Arg::new("table").long("table"))
            .arg(Arg::new("database").long("database")),
    ))))
}

struct ModelFieldsCommand(CommandDefinition);

impl Command for ModelFieldsCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let mut names = if input.matches().get_flag("all") {
                compiled_models()?
            } else {
                input
                    .matches()
                    .get_many::<String>("name")
                    .expect("required")
                    .cloned()
                    .collect()
            };
            names.sort();
            names.dedup();
            let table = input.matches().get_one::<String>("table");
            let database = input.matches().get_one::<String>("database");
            if names.len() != 1 && (table.is_some() || database.is_some()) {
                return Err(ConsoleError::new(
                    "批量同步不能共用 --table 或 --database；请使用各模型的配置。",
                ));
            }
            if names.is_empty() {
                return Err(ConsoleError::new(
                    "没有已编译的自动字段模型；请先逐个运行 model:fields 并重新构建。",
                ));
            }
            let check = input.matches().get_flag("check");
            // 只读检查不创建源码锁或临时文件。写入操作复用现有锁，先完成全部生成预检。
            let _lock = if check {
                None
            } else {
                Some(source_edit::lock_source(app.base_path())?)
            };
            let mut updates = Vec::new();
            for name in names {
                let parts = validated_parts(&name)?;
                let class = parts.last().expect("validated");
                let path = model_path(app, &parts);
                let previous = source_edit::read(&path)?;
                let definition = compiled_definition(&parts)?;
                let literals = table_literals(&previous, class)?;
                let declared = literals.first().map(|(_, table)| table.as_str());
                let generated = generated_table(&previous, class)?;
                if table.is_none()
                    && definition
                        .as_ref()
                        .zip(declared)
                        .is_some_and(|((table, _), declared)| table != declared)
                {
                    return Err(ConsoleError::new(
                        "源码表名与已编译模型不一致，请重新构建或显式指定 --table，未修改模型。",
                    ));
                }
                let inferred = pluralizer::pluralize(&snake_case(class), 2, false);
                let selected_table = table
                    .map(String::as_str)
                    .or_else(|| definition.as_ref().map(|(table, _)| table.as_str()))
                    .or(declared)
                    .or(generated.as_deref())
                    .unwrap_or(&inferred);
                let selected_database = database.map(String::as_str).or_else(|| {
                    definition
                        .as_ref()
                        .and_then(|(_, database)| database.as_deref())
                });
                let block =
                    generate_for(app, &parts.join("/"), selected_table, selected_database).await?;
                let updated = merge(&previous, &block, &parts.join("/"))?;
                updates.push((path, previous, updated));
            }
            // 数据库预检期间发生的源码修改不能被覆盖，也不能报告为检查通过。
            for (path, previous, _) in &updates {
                if source_edit::read(path)? != *previous {
                    return Err(ConsoleError::new(format!(
                        "{} 在检查期间已修改，请重试。",
                        path.display()
                    )));
                }
            }
            let mut stale = false;
            for (path, previous, updated) in updates {
                if previous == updated {
                    output.info(&format!("模型字段已同步 [{}]。", path.display()));
                } else if check {
                    stale = true;
                    output.error(&format!(
                        "模型字段已过期 [{}]；请运行 model:fields 刷新并重新构建。",
                        path.display()
                    ));
                } else {
                    source_edit::replace(&path, &previous, &updated)?;
                    output.info(&format!(
                        "已更新模型字段 [{}]，请重新构建。",
                        path.display()
                    ));
                }
            }
            if stale {
                return Ok(1);
            }
            Ok(0)
        })
    }
}

pub(super) fn table_name(input: &Input, class: &str) -> String {
    input
        .matches()
        .get_one::<String>("table")
        .cloned()
        .unwrap_or_else(|| pluralizer::pluralize(&snake_case(class), 2, false))
}

pub(super) async fn generate(app: &Application, input: &Input) -> Result<String, ConsoleError> {
    let name = input.matches().get_one::<String>("name").expect("required");
    let parts = validated_parts(name)?;
    let class = parts.last().expect("validated");
    let table = table_name(input, class);
    let database = input
        .matches()
        .get_one::<String>("database")
        .map(String::as_str);
    generate_for(app, &parts.join("/"), &table, database).await
}

async fn generate_for(
    app: &Application,
    name: &str,
    table: &str,
    database: Option<&str>,
) -> Result<String, ConsoleError> {
    let parts = validated_parts(name)?;
    let class = parts.last().expect("validated");
    let manager = app
        .container()
        .get::<DatabaseManager>("db")
        .map_err(error)?;
    let connection = manager.connection(database).map_err(error)?;
    let mut schema = connection.getSchemaBuilder();
    let columns = schema.getColumns(table).await.map_err(error)?;
    let (namespace, logical_name) = schema
        .parseSchemaAndTable(table, SchemaFallback::None)
        .map_err(error)?;
    let physical_name = format!("{}{logical_name}", connection.getTablePrefix());
    let namespaces = namespace.map_or_else(
        || schema.getCurrentSchemaListing(),
        |name| vec![name.to_owned()],
    );
    let mut table_comment = None;
    // 按连接的 schema 查找顺序读取同一张表，避免拿到其他 schema 的同名表说明。
    for namespace in namespaces {
        if let Some(info) = schema
            .getTables(Some(&namespace))
            .await
            .map_err(error)?
            .into_iter()
            .find(|info| info.name == physical_name)
        {
            table_comment = info.comment;
            break;
        }
    }
    if columns.is_empty() {
        return Err(ConsoleError::new(format!(
            "表 [{table}] 不存在或没有可生成的列。"
        )));
    }
    let mut names = BTreeMap::new();
    let mut fields = Vec::with_capacity(columns.len());
    for column in columns {
        let field = constant_name(&column.name)?;
        let name = column.name.clone();
        if let Some(previous) = names.insert(field.clone(), name.clone()) {
            return Err(ConsoleError::new(format!(
                "列 [{}] 与 [{name}] 都映射为 [{field}]，未修改模型。",
                previous
            )));
        }
        fields.push((field, column));
    }
    let locale = app
        .container()
        .get::<crate::translation::Translator>("translator")
        .map_err(error)?
        .getLocale();
    let section = generated_section(name);
    let mut block = format!("openrust::model_fields! {{\n{section}\n    {class} {{\n");
    let table_documentation = table_comment
        .as_deref()
        .filter(|comment| !comment.trim().is_empty())
        .map_or_else(
            || {
                crate::console::localization::text_for_locale(
                    "model_fields.table",
                    "Table",
                    Some(&locale),
                )
            },
            str::to_owned,
        );
    append_documentation(&mut block, &table_documentation);
    block.push_str(&format!("        TABLE = {table:?};\n"));
    for (field, column) in fields {
        if let Some(comment) = column
            .comment
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            append_documentation(&mut block, comment);
        }
        block.push_str(&format!("        {field} = {:?};\n", column.name));
    }
    block.push_str("    }\n}\n");
    Ok(block)
}

fn append_documentation(block: &mut String, comment: &str) {
    // 控制字符使用转义属性，其余说明采用易读的文档注释。
    if comment
        .chars()
        .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    {
        block.push_str(&format!("        #[doc = {comment:?}]\n"));
    } else {
        for line in comment.split('\n') {
            block.push_str(&format!("        /// {line}\n"));
        }
    }
}

fn constant_name(column: &str) -> Result<String, ConsoleError> {
    if !column.is_ascii() {
        return Err(ConsoleError::new(format!(
            "列 [{column}] 包含非 ASCII 字符，请使用显式 model_fields! 映射。"
        )));
    }
    let normalized: String = column
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let mut field = snake_case(&normalized).to_ascii_uppercase();
    if !field.bytes().any(|byte| byte.is_ascii_alphanumeric()) {
        return Err(ConsoleError::new(format!(
            "列 [{column}] 无法生成字段标识。"
        )));
    }
    if field.as_bytes()[0].is_ascii_digit() {
        field.insert_str(0, "COLUMN_");
    }
    // TABLE 留给模型表名，实际同名列使用带前缀的标识，仍参与冲突预检。
    if field == "TABLE" {
        field = "COLUMN_TABLE".into();
    }
    Ok(field)
}

// 通过宏前紧邻的配置式块注释定位说明，不依赖语言、标题或翻译历史。
fn configuration_section_start(prefix: &str) -> Option<usize> {
    let prefix = prefix.trim_end();
    if !prefix.ends_with("*/") {
        return None;
    }
    let start = prefix
        .rfind("\n/*")
        .map(|offset| offset + 1)
        .or_else(|| prefix.starts_with("/*").then_some(0))?;
    let mut lines = prefix[start..].lines();
    if lines.next()? != "/*" {
        return None;
    }
    let lines = lines.collect::<Vec<_>>();
    if lines.last().copied()? != "*/"
        || !lines[..lines.len() - 1]
            .iter()
            .all(|line| line.starts_with('|'))
    {
        return None;
    }
    let separators = lines
        .iter()
        .filter(|line| {
            line.strip_prefix('|')
                .is_some_and(|text| text.len() >= 8 && text.bytes().all(|byte| byte == b'-'))
        })
        .count();
    (separators >= 2).then_some(start)
}

pub(super) fn merge(source: &str, block: &str, name: &str) -> Result<String, ConsoleError> {
    let parts = validated_parts(name)?;
    let class = *parts.last().expect("validated");
    let (original, original_base) = parse_source(source)?;
    let legacy_macros = original
        .items
        .iter()
        .filter_map(|item| {
            let syn::Item::Macro(item) = item else {
                return None;
            };
            (item.mac.path.segments.last()?.ident == "model_fields").then(|| {
                original_base
                    + item
                        .mac
                        .path
                        .segments
                        .first()
                        .expect("macro path")
                        .ident
                        .span()
                        .byte_range()
                        .start
            })
        })
        .collect::<Vec<_>>();
    let mut source = source.to_owned();
    // 旧版标记仅用于迁移；新格式直接使用完整 Rust 宏的语法边界。
    let mut begin = None;
    let mut end = None;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        match line.trim_end_matches(['\r', '\n']) {
            BEGIN if begin.is_none() => begin = Some(offset),
            END if end.is_none() => end = Some(offset + line.len()),
            BEGIN | END => return Err(ConsoleError::new("模型字段生成区域的标记重复。")),
            _ => {}
        }
        offset += line.len();
    }
    if let (Some(start), Some(finish)) = (begin, end)
        && start < finish
        && !legacy_macros
            .iter()
            .any(|offset| (start..finish).contains(offset))
    {
        // 原始字符串内的同名文本不是生成区域。
        begin = None;
        end = None;
    }
    match (begin, end) {
        (None, None) => {}
        (Some(begin), Some(end)) if begin < end => {
            // 标记只界定候选区域；混入业务项或其他模型时不能整段删除。
            let legacy = syn::parse_file(&source[begin..end]).map_err(error)?;
            let [syn::Item::Macro(item)] = legacy.items.as_slice() else {
                return Err(ConsoleError::new(
                    "旧模型字段区域混有其他源码，未修改文件。",
                ));
            };
            let owns_model = legacy_owns_model(item.mac.tokens.clone(), class);
            if !item.attrs.is_empty()
                || item
                    .mac
                    .path
                    .segments
                    .last()
                    .is_none_or(|segment| segment.ident != "model_fields")
                || !owns_model
            {
                return Err(ConsoleError::new(
                    "旧模型字段区域不属于当前模型或含有附加属性，未修改文件。",
                ));
            }
            // 移走文件末尾的旧分区时，同时移除它与业务代码之间的一行空白。
            let begin = if end == source.len() && source[..begin].ends_with("\r\n\r\n") {
                begin - 2
            } else if end == source.len() && source[..begin].ends_with("\n\n") {
                begin - 1
            } else {
                begin
            };
            source.replace_range(begin..end, "");
        }
        _ => {
            return Err(ConsoleError::new(
                "模型字段生成区域的标记不完整或顺序错误。",
            ));
        }
    }
    let (file, base) = parse_source(&source)?;
    let declarations = file
        .items
        .iter()
        .filter_map(|item| {
            let syn::Item::Macro(item) = item else {
                return None;
            };
            let name = item.mac.path.segments.last()?;
            if name.ident != "model_fields" {
                return None;
            }
            let first = item.mac.tokens.clone().into_iter().next()?;
            if !matches!(first, proc_macro2::TokenTree::Ident(ident) if ident == class) {
                return None;
            }
            let start = base
                + item
                    .mac
                    .path
                    .segments
                    .first()?
                    .ident
                    .span()
                    .byte_range()
                    .start;
            let end = base
                + item.semi_token.as_ref().map_or_else(
                    || item.mac.delimiter.span().close().byte_range().end,
                    |token| token.span.byte_range().end,
                );
            Some((start, end, item.attrs.is_empty()))
        })
        .collect::<Vec<_>>();
    match declarations.as_slice() {
        [] => {
            // 生成宏被删改时不能仅因不认识旧文案便插入第二份说明。
            if source
                .match_indices("*/")
                .any(|(end, _)| configuration_section_start(&source[..end + 2]).is_some())
            {
                return Err(ConsoleError::new(
                    "模型中存在配置式说明但没有可识别的字段宏，未修改源码。",
                ));
            }
        }
        [(start, end, true)] if begin.is_none() => {
            let item = syn::parse_str::<syn::ItemMacro>(&source[*start..*end]).map_err(error)?;
            let (_, generated) = model_tokens(item.mac.tokens)?;
            if generated {
                if !file.items.iter().any(|item| match item {
                    syn::Item::Struct(item) => item.ident == class,
                    syn::Item::Enum(item) => item.ident == class,
                    _ => false,
                }) {
                    return Err(ConsoleError::new("找不到目标模型类型，未修改源码。"));
                }
                // 原地替换整个宏；边界外的注释、换行和业务项保持原文。
                source.replace_range(*start..*end, block.trim_end_matches('\n'));
                return bind_table(&source, class);
            }
            let section = match configuration_section_start(&source[..*start]) {
                Some(section) => section,
                // 说明被完整删除时，仍可凭唯一且无附加属性的字段宏恢复。
                None if !source[..*start].trim_end().ends_with("*/") => *start,
                None => {
                    return Err(ConsoleError::new("模型字段说明边界已损坏，未修改源码。"));
                }
            };
            let trailing =
                source[*end..].len() - source[*end..].trim_start_matches(['\r', '\n']).len();
            source.replace_range(section..end + trailing, "");
        }
        _ => {
            return Err(ConsoleError::new(
                "模型字段说明或声明不完整、重复或有歧义，未修改源码。",
            ));
        }
    }
    source = bind_table(&source, class)?;
    let (file, base) = parse_source(&source)?;
    if !file.items.iter().any(|item| match item {
        syn::Item::Struct(item) => item.ident == class,
        syn::Item::Enum(item) => item.ident == class,
        _ => false,
    }) {
        return Err(ConsoleError::new(format!(
            "找不到模型类型 [{class}]，未修改源码。"
        )));
    }
    // 跟随连续的文件头导入，保留 inner 属性、文档、derive 和所有业务项的原始文本。
    let mut insertion = file.attrs.last().map_or(base, |attr| {
        base + attr.bracket_token.span.close().byte_range().end
    });
    for item in &file.items {
        insertion = match item {
            syn::Item::Use(item) => base + item.semi_token.span.byte_range().end,
            syn::Item::ExternCrate(item) => base + item.semi_token.span.byte_range().end,
            _ => break,
        };
        // 分号后的同行注释仍属于原导入行。
        let line_end = source[insertion..]
            .find('\n')
            .map_or(source.len(), |line| insertion + line);
        let tail = source[insertion..line_end].trim();
        if tail.is_empty() || tail.starts_with("//") {
            insertion = (line_end + usize::from(line_end < source.len())).min(source.len());
        }
    }
    insertion += source[insertion..].len()
        - source[insertion..]
            .trim_start_matches(char::is_whitespace)
            .len();
    let separator = if insertion != 0 && !source[..insertion].ends_with("\n\n") {
        "\n"
    } else {
        ""
    };
    Ok(format!(
        "{}{separator}{block}\n{}",
        &source[..insertion],
        &source[insertion..]
    ))
}

// 仅迁移标准 metadata 静态初始化中的表名字面量，不重写用户的动态表名逻辑。
fn table_literals(
    source: &str,
    class: &str,
) -> Result<Vec<(std::ops::Range<usize>, String)>, ConsoleError> {
    let (file, base) = parse_source(source)?;
    let mut ranges = Vec::new();
    for item in &file.items {
        let syn::Item::Impl(item) = item else {
            continue;
        };
        let syn::Type::Path(model) = item.self_ty.as_ref() else {
            continue;
        };
        if model
            .path
            .segments
            .last()
            .is_none_or(|segment| segment.ident != class)
        {
            continue;
        }
        if item.trait_.is_none()
            && item
                .items
                .iter()
                .any(|item| matches!(item, syn::ImplItem::Const(item) if item.ident == "TABLE"))
        {
            return Err(ConsoleError::new(
                "模型已手写 TABLE 常量，请先消除与自动生成表名的重复声明。",
            ));
        }
        if !item.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "Model")
        }) {
            continue;
        }
        if item.items.iter().any(
            |member| matches!(member, syn::ImplItem::Fn(method) if method.sig.ident == "getTable"),
        ) {
            continue;
        }
        for member in &item.items {
            let syn::ImplItem::Fn(method) = member else {
                continue;
            };
            if method.sig.ident != "metadata" {
                continue;
            }
            for statement in &method.block.stmts {
                let syn::Stmt::Item(syn::Item::Static(metadata)) = statement else {
                    continue;
                };
                if metadata.ident != "METADATA" {
                    continue;
                }
                let mut expression = metadata.expr.as_ref();
                while let syn::Expr::MethodCall(call) = expression {
                    expression = call.receiver.as_ref();
                }
                let syn::Expr::Call(call) = expression else {
                    continue;
                };
                let syn::Expr::Path(path) = call.func.as_ref() else {
                    continue;
                };
                let mut segments = path.path.segments.iter().rev();
                if segments.next().is_none_or(|segment| segment.ident != "new")
                    || segments
                        .next()
                        .is_none_or(|segment| segment.ident != "ModelMetadata")
                    || call.args.len() != 1
                {
                    continue;
                }
                if let Some(syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(table),
                    ..
                })) = call.args.first()
                {
                    let range = table.span().byte_range();
                    ranges.push((base + range.start..base + range.end, table.value()));
                }
            }
        }
    }
    Ok(ranges)
}

fn bind_table(source: &str, class: &str) -> Result<String, ConsoleError> {
    let ranges = table_literals(source, class)?;
    let mut updated = source.to_owned();
    for (range, _) in ranges.into_iter().rev() {
        updated.replace_range(range, &format!("{class}::TABLE"));
    }
    Ok(updated)
}

// 旧宏的注册类型和常量所属类型都必须匹配，只迁移生成器支持的列常量语法。
fn legacy_owns_model(tokens: proc_macro2::TokenStream, class: &str) -> bool {
    use syn::parse::Parser as _;

    let parser = |input: syn::parse::ParseStream<'_>| -> syn::Result<bool> {
        let _: syn::Token![@] = input.parse()?;
        let keyword: syn::Ident = input.parse()?;
        let model: syn::Ident = input.parse()?;
        let _: syn::Token![;] = input.parse()?;
        let _ = input.call(syn::Attribute::parse_outer)?;
        let _: syn::Token![impl] = input.parse()?;
        let owner: syn::Ident = input.parse()?;
        let fields;
        syn::braced!(fields in input);
        while !fields.is_empty() {
            let _ = fields.call(syn::Attribute::parse_outer)?;
            let _: syn::Visibility = fields.parse()?;
            let _: syn::Token![const] = fields.parse()?;
            let _: syn::Ident = fields.parse()?;
            let _: syn::Token![=] = fields.parse()?;
            let _: syn::LitStr = fields.parse()?;
            let _: syn::Token![;] = fields.parse()?;
        }
        Ok(keyword == "model" && model == class && owner == class)
    };
    parser.parse2(tokens).unwrap_or(false)
}

// 新模型尚未编入命令程序时，复用生成区中唯一维护的表名，不退回类名推断。
fn generated_table(source: &str, class: &str) -> Result<Option<String>, ConsoleError> {
    use syn::parse::Parser as _;

    let (file, _) = parse_source(source)?;
    let mut table = None;
    for item in &file.items {
        let syn::Item::Macro(item) = item else {
            continue;
        };
        if item
            .mac
            .path
            .segments
            .last()
            .is_none_or(|segment| segment.ident != "model_fields")
            || !matches!(item.mac.tokens.clone().into_iter().next(), Some(proc_macro2::TokenTree::Ident(model)) if model == class)
        {
            continue;
        }
        let parser = |input: syn::parse::ParseStream<'_>| -> syn::Result<Option<String>> {
            let _: syn::Ident = input.parse()?;
            let content;
            syn::braced!(content in input);
            let _ = content.call(syn::Attribute::parse_outer)?;
            if content.is_empty() {
                return Ok(None);
            }
            let field: syn::Ident = content.parse()?;
            let value = if field == "TABLE" {
                let _: syn::Token![=] = content.parse()?;
                let value: syn::LitStr = content.parse()?;
                let _: syn::Token![;] = content.parse()?;
                Some(value.value())
            } else {
                None
            };
            let _: proc_macro2::TokenStream = content.parse()?;
            Ok(value)
        };
        if let Some(value) = parser.parse2(item.mac.tokens.clone()).map_err(error)?
            && table.replace(value).is_some()
        {
            return Err(ConsoleError::new("模型存在多份生成表名，未修改文件。"));
        }
    }
    Ok(table)
}

// generated 是宏输入元信息，不生成编译器属性；命令文本只用于展示，不执行。
fn model_tokens(
    tokens: proc_macro2::TokenStream,
) -> Result<(proc_macro2::TokenStream, bool), ConsoleError> {
    use syn::parse::Parser as _;
    let parser =
        |input: syn::parse::ParseStream<'_>| -> syn::Result<(proc_macro2::TokenStream, bool)> {
            let attributes = input.call(syn::Attribute::parse_outer)?;
            if !attributes
                .iter()
                .any(|attr| attr.path().is_ident("generated"))
            {
                let _: proc_macro2::TokenStream = input.parse()?;
                return Ok((tokens.clone(), false));
            }
            if attributes.len() != 1 {
                return Err(input.error("generated 元信息必须唯一，且不能混入其他属性"));
            }
            attributes[0].parse_args_with(|args: syn::parse::ParseStream<'_>| {
                let key: syn::Ident = args.parse()?;
                if key != "command" {
                    return Err(args.error("generated 仅接受 command 参数"));
                }
                let _: syn::Token![=] = args.parse()?;
                let _: syn::LitStr = args.parse()?;
                if args.peek(syn::Token![,]) {
                    let _: syn::Token![,] = args.parse()?;
                }
                if !args.is_empty() {
                    return Err(args.error("generated 参数重复或无效"));
                }
                Ok(())
            })?;
            Ok((input.parse()?, true))
        };
    parser.parse2(tokens.clone()).map_err(error)
}

fn parse_source(source: &str) -> Result<(syn::File, usize), ConsoleError> {
    let mut file = syn::parse_file(source)
        .map_err(|error| ConsoleError::new(format!("模型源码语法无效，未修改文件：{error}")))?;
    let models = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(item) => Some(item.ident.to_string()),
            syn::Item::Enum(item) => Some(item.ident.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for item in &mut file.items {
        if let syn::Item::Macro(item) = item {
            let mut head = item.mac.tokens.clone().into_iter();
            let has_metadata = matches!(head.next(), Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == '#')
                && matches!(head.next(), Some(proc_macro2::TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Bracket && matches!(group.stream().into_iter().next(), Some(proc_macro2::TokenTree::Ident(ident)) if ident == "generated"));
            if has_metadata
                && item
                    .mac
                    .path
                    .segments
                    .last()
                    .is_none_or(|segment| segment.ident != "model_fields")
            {
                return Err(ConsoleError::new(
                    "生成元信息不属于 model_fields 宏，未修改源码。",
                ));
            }
        }
        if let syn::Item::Macro(item) = item
            && item
                .mac
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "model_fields")
        {
            let (tokens, generated) = model_tokens(item.mac.tokens.clone())?;
            if generated && !tokens.clone().into_iter().next().is_some_and(|token| matches!(token, proc_macro2::TokenTree::Ident(ident) if models.contains(&ident.to_string()))) {
                return Err(ConsoleError::new("生成元信息对应的模型类型不存在，未修改源码。"));
            }
            item.mac.tokens = tokens;
        }
    }
    // syn 剥离 BOM 和 shebang 后计算 span，补回这部分字节才能精确编辑原文。
    let base = usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8()
        + file.shebang.as_ref().map_or(0, String::len);
    Ok((file, base))
}

fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

fn model_path(app: &Application, parts: &[&str]) -> PathBuf {
    let mut path = app.base_path().join("app/models");
    for part in parts {
        path.push(snake_case(part));
    }
    path.with_extension("rs")
}

fn compiled_definition(parts: &[&str]) -> Result<Option<(String, Option<String>)>, ConsoleError> {
    let module = parts
        .iter()
        .map(|part| snake_case(part))
        .collect::<Vec<_>>()
        .join("::");
    let suffix = format!("::app::models::{module}");
    let class = *parts.last().expect("validated");
    let models = inventory::iter::<FieldModel>
        .into_iter()
        .filter(|model| model.module.ends_with(&suffix) && model.class == class)
        .collect::<Vec<_>>();
    match models.as_slice() {
        [] => Ok(None),
        [model] => (model.definition)().map(Some).map_err(error),
        _ => Err(ConsoleError::new(format!(
            "模型路径 [{}] 存在多个已编译类型，请消除歧义。",
            parts.join("/")
        ))),
    }
}

fn compiled_models() -> Result<Vec<String>, ConsoleError> {
    let mut names = Vec::new();
    for model in inventory::iter::<FieldModel> {
        let Some((_, relative)) = model.module.split_once("::app::models::") else {
            continue;
        };
        let mut parts = relative.split("::").collect::<Vec<_>>();
        let class = model.class;
        let Some(module) = parts.pop() else {
            continue;
        };
        if snake_case(class) != module {
            continue;
        }
        let mut path = parts
            .into_iter()
            .map(|part| {
                part.split('_')
                    .map(|word| {
                        let mut chars = word.chars();
                        chars
                            .next()
                            .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                            .unwrap_or_default()
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        path.push(class.to_owned());
        let name = path.join("/");
        validated_parts(&name)?;
        names.push(name);
    }
    Ok(names)
}
