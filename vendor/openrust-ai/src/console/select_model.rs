use std::{io::ErrorKind, path::PathBuf};

use openrust::{
    config::{Environment, EnvironmentDocument, EnvironmentFile, EnvironmentFileError},
    console::{
        ChoiceQuestion, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::facades::Artisan,
};
use serde_json::Value;

use crate::{Ai, AiError, AiManager, localization};

use super::{console_error, error_message, write_line};

pub(super) fn register() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(SelectModel {
        definition: CommandDefinition::from_signature(
            "ai:select-model {provider? : AI 连接名称} {--model= : 手填模型 ID} {--save : 将模型保存到当前应用环境文件} {--default : 同时设为应用默认 AI 连接}",
        )?.with_help_namespace("ai"),
    }))
}

struct SelectModel {
    definition: CommandDefinition,
}

impl Command for SelectModel {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let manager = Ai::manager().map_err(console_error)?;
            let interactive = input.option("no-interaction") != Some(Value::Bool(true));
            let save = input.matches().get_flag("save");
            let make_default = input.matches().get_flag("default");
            if make_default && !save {
                return Err(ConsoleError::new(message(
                    "default_requires_save",
                    "保存默认连接时须同时指定 --save",
                )));
            }
            let provider = match input.matches().get_one::<String>("provider") {
                Some(provider) => provider.clone(),
                None => choose_provider(app, &manager, input, output, interactive).await?,
            };
            let profile = manager
                .get_instance_config(&provider)
                .map_err(console_error)?;
            let model = match input.matches().get_one::<String>("model") {
                Some(model) => model.clone(),
                None if !interactive => {
                    return Err(ConsoleError::new(message(
                        "model_required_non_interactive",
                        "非交互模式请用 --model 指定要选择的模型",
                    )));
                }
                None => choose(input, output, &provider).await?,
            };
            if model.trim().is_empty() || model.chars().any(char::is_control) {
                return Err(ConsoleError::new(message(
                    "invalid_model",
                    "模型 ID 不能为空或包含控制字符",
                )));
            }
            if save {
                let variable = profile["models"]["text"]["default_env"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        ConsoleError::new(message(
                            "model_variable_missing",
                            "当前连接由 config/ai.rs 管理，未配置可写入的模型环境变量",
                        ))
                    })?;
                let profile_key = profile["key_env"].as_str();
                if !valid_variable(variable)
                    || profile_key == Some(variable)
                    || variable == "AI_PROVIDER"
                {
                    return Err(ConsoleError::new(message(
                        "invalid_variable",
                        "模型的环境变量配置无效",
                    )));
                }
                let mut changes = vec![(variable, model.as_str())];
                if make_default {
                    changes.push(("AI_PROVIDER", &provider));
                }
                for (name, _) in &changes {
                    if std::env::var_os(name).is_some() {
                        return Err(ConsoleError::new(localization::text(
                            "ai::console.select_model.variable_overridden",
                            &[("variable".into(), (*name).into())].into(),
                            &format!("进程环境中的 {name} 优先，请在设置该变量的位置更新"),
                        )));
                    }
                }
                let editor = match EnvironmentFile::for_application(app) {
                    Ok(file) => Some(file.edit().map_err(environment_error)?),
                    Err(EnvironmentFileError::Io {
                        kind: ErrorKind::NotFound,
                        ..
                    }) => None,
                    Err(error) => return Err(environment_error(error)),
                };
                if interactive {
                    let prompt = localization::text(
                        "ai::console.select_model.confirm_save",
                        &[
                            ("provider".into(), provider.clone()),
                            ("variable".into(), variable.into()),
                        ]
                        .into(),
                        &format!("将 {provider} 的模型保存到 {variable}？"),
                    );
                    if !input.io(output).confirm(&prompt).await? {
                        return Ok(0);
                    }
                }
                if let Some(mut editor) = editor {
                    editor.set_many(changes).map_err(environment_error)?;
                    editor.save().map_err(environment_error)?;
                } else {
                    let path = app
                        .container()
                        .get::<PathBuf>("env.file")
                        .map_err(ConsoleError::from_source)?;
                    let mut document = EnvironmentDocument::new();
                    document.set_many(changes).map_err(environment_error)?;
                    EnvironmentFile::create(path.as_ref(), document.content())
                        .map_err(environment_error)?;
                }
                if app.configuration_is_cached() {
                    output.info(&message(
                        "saved_cached",
                        "已保存模型。请重新生成应用配置缓存后使用。",
                    ));
                }
            }
            write_line(output, &model)?;
            Ok(0)
        })
    }
}

async fn choose_provider(
    app: &Application,
    manager: &AiManager,
    input: &Input,
    output: &mut Output,
    interactive: bool,
) -> Result<String, ConsoleError> {
    match manager.get_default_instance() {
        Ok(provider) => return Ok(provider),
        Err(AiError::DefaultNotConfigured) => {}
        Err(error) => return Err(console_error(error)),
    }
    let environment = app
        .make::<Environment>()
        .await
        .map_err(ConsoleError::from_source)?;
    let repository = app.config().map_err(ConsoleError::from_source)?;
    let profiles = repository.get("ai.providers").unwrap_or_default();
    let profiles = profiles.as_object().ok_or_else(|| {
        ConsoleError::new(message("connections_unavailable", "没有可用的 AI 连接"))
    })?;
    let mut available: Vec<_> = profiles
        .iter()
        .filter(|(_, profile)| {
            profile
                .get("key_env")
                .and_then(Value::as_str)
                .is_none_or(|key| {
                    environment
                        .get(key)
                        .is_some_and(|value| !value.trim().is_empty())
                })
        })
        .map(|(name, _)| name.clone())
        .collect();
    if available.len() == 1 {
        return Ok(available.remove(0));
    }
    if !interactive {
        return Err(ConsoleError::new(message(
            "connection_required_non_interactive",
            "非交互模式请指定 AI 连接名称",
        )));
    }
    if available.is_empty() {
        available.extend(profiles.keys().cloned());
    }
    if available.is_empty() {
        return Err(ConsoleError::new(message(
            "no_connections",
            "没有可供选择的 AI 连接",
        )));
    }
    available.sort();
    input
        .io(output)
        .choice(&message("choose_connection", "选择 AI 连接"), available)
        .await?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ConsoleError::new(message("connection_required", "请选择 AI 连接")))
}

async fn choose(
    input: &Input,
    output: &mut Output,
    provider: &str,
) -> Result<String, ConsoleError> {
    let catalog = Ai::models(Some(provider)).await.map_err(console_error)?;
    if let Some(error) = catalog.discovery_error {
        output.error(&error_message(&error));
    }
    let mut io = input.io(output);
    let models: Vec<String> = catalog
        .models
        .into_iter()
        .map(|model| model.id)
        .filter(|model| !model.chars().any(char::is_control))
        .collect();
    if !models.is_empty() {
        let labels = std::iter::once(message("manual_model", "手填模型 ID")).chain(
            models.iter().map(|model| {
                localization::text(
                    "ai::console.select_model.model_label",
                    &[("model".into(), model.clone())].into(),
                    &format!("模型：{model}"),
                )
            }),
        );
        let choice = ChoiceQuestion::from_pairs_with_associative(
            message("choose_model", "选择模型"),
            labels
                .enumerate()
                .map(|(index, label)| (index as i64, label)),
            None,
            |_| true,
        )?;
        let selected = io.get_output().ask_question(choice.as_question()).await?;
        if let Some(model) = selected
            .as_str()
            .and_then(|value| value.parse::<usize>().ok())
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| models.get(index))
        {
            return Ok(model.clone());
        }
    }
    io.ask(&message("input_model", "输入模型 ID"))
        .await?
        .ok_or_else(|| ConsoleError::new(message("model_required", "请提供模型 ID")))
}

fn message(key: &str, fallback: &str) -> String {
    localization::text(
        &format!("ai::console.select_model.{key}"),
        &Default::default(),
        fallback,
    )
}

fn valid_variable(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn environment_error(error: EnvironmentFileError) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
