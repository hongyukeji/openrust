use std::{collections::BTreeMap, path::PathBuf};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, generator_command::snake_case,
    },
    contracts::{container::Container as _, foundation::Application as _},
    database::{DatabaseManager, eloquent::FieldModel},
    foundation::Application,
};

use super::{model_make_command::validated_parts, source_edit};

const BEGIN: &str = "// openrust:model-fields begin";
const END: &str = "// openrust:model-fields end";

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
                let inferred = pluralizer::pluralize(&snake_case(class), 2, false);
                let selected_table = table
                    .map(String::as_str)
                    .or_else(|| definition.as_ref().map(|(table, _)| table.as_str()))
                    .unwrap_or(&inferred);
                let selected_database = database.map(String::as_str).or_else(|| {
                    definition
                        .as_ref()
                        .and_then(|(_, database)| database.as_deref())
                });
                let block = generate_for(app, class, selected_table, selected_database).await?;
                let updated = merge(&previous, &block)?;
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
    generate_for(app, class, &table, database).await
}

async fn generate_for(
    app: &Application,
    class: &str,
    table: &str,
    database: Option<&str>,
) -> Result<String, ConsoleError> {
    let manager = app
        .container()
        .get::<DatabaseManager>("db")
        .map_err(error)?;
    let columns = manager
        .connection(database)
        .map_err(error)?
        .getSchemaBuilder()
        .getColumns(table)
        .await
        .map_err(error)?;
    if columns.is_empty() {
        return Err(ConsoleError::new(format!(
            "表 [{table}] 不存在或没有可生成的列。"
        )));
    }
    let mut fields = BTreeMap::new();
    for column in columns {
        let field = constant_name(&column.name)?;
        let name = column.name.clone();
        if let Some(previous) = fields.insert(field.clone(), column) {
            return Err(ConsoleError::new(format!(
                "列 [{}] 与 [{name}] 都映射为 [{field}]，未修改模型。",
                previous.name
            )));
        }
    }
    let mut block = format!(
        "{BEGIN}\n// 自动生成；请使用 model:fields 刷新，不要手工修改此区域。\nopenrust::database::eloquent::model_fields! {{\n    @model {class};\n    #[allow(dead_code, reason = \"生成的字段引用按需使用\")]\n    impl {class} {{\n"
    );
    for (field, column) in fields {
        if let Some(comment) = column
            .comment
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            // 文档属性等价于 ///；使用 Rust 字符串转义，防止数据库注释破坏生成源码。
            block.push_str(&format!("        #[doc = {comment:?}]\n"));
        }
        block.push_str(&format!("        pub const {field} = {:?};\n", column.name));
    }
    block.push_str(&format!("    }}\n}}\n{END}\n"));
    Ok(block)
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
    Ok(field)
}

pub(super) fn merge(source: &str, block: &str) -> Result<String, ConsoleError> {
    // 只接受整行且唯一的成对标记；损坏标记时拒绝写入，避免删除业务代码。
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
    match (begin, end) {
        (None, None) => Ok(format!("{}\n\n{block}", source)),
        (Some(begin), Some(end)) if begin < end => {
            Ok(format!("{}{block}{}", &source[..begin], &source[end..]))
        }
        _ => Err(ConsoleError::new(
            "模型字段生成区域的标记不完整或顺序错误。",
        )),
    }
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
