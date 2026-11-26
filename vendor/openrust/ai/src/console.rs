use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
    json_schema::JsonSchema,
    support::facades::Artisan,
};
use serde_json::json;

use crate::{
    Agent, Ai, AiError, Message, PromptOptions, StructuredAgentResponse, StructuredOutput, agent,
    localization,
};

mod make;
mod select_model;

pub(crate) use make::STUBS;

pub(crate) fn register() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(Models {
        definition: CommandDefinition::from_signature(
            "ai:models {provider? : AI 连接名称} {--json : 输出 JSON}",
        )?
        .with_help_namespace("ai"),
    }))?;
    Artisan::add(Box::new(Prompt {
        definition: CommandDefinition::from_signature(
            "ai:prompt {prompt : 提示词} {--provider= : AI 连接名称} {--model= : 上游模型 ID} {--messages= : JSON 格式的历史消息} {--schema= : JSON 对象模式}",
        )?.with_help_namespace("ai"),
    }))?;
    make::register()?;
    select_model::register()
}

struct Models {
    definition: CommandDefinition,
}

impl Command for Models {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let provider = input
                .matches()
                .get_one::<String>("provider")
                .map(String::as_str);
            let catalog = Ai::models(provider).await.map_err(console_error)?;
            let status = i32::from(catalog.models.is_empty() && catalog.discovery_error.is_some());
            if input.matches().get_flag("json") {
                write_line(
                    output,
                    &json!({
                        "models": catalog.models,
                        "discovery_error": catalog.discovery_error.map(|error| error.to_string())
                    })
                    .to_string(),
                )?;
            } else {
                for model in catalog.models {
                    write_line(output, &model.id)?;
                }
                if let Some(error) = catalog.discovery_error {
                    output.error(&error_message(&error));
                }
            }
            Ok(status)
        })
    }
}

struct Prompt {
    definition: CommandDefinition,
}

impl Command for Prompt {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let prompt = input
                .matches()
                .get_one::<String>("prompt")
                .ok_or_else(|| ConsoleError::new("请提供 AI 提示词"))?;
            let options = PromptOptions {
                provider: input
                    .matches()
                    .get_one::<String>("provider")
                    .map(String::as_str),
                model: input
                    .matches()
                    .get_one::<String>("model")
                    .map(String::as_str),
                timeout: None,
            };
            let messages = input
                .matches()
                .get_one::<String>("messages")
                .map(|history| serde_json::from_str::<Vec<Message>>(history))
                .transpose()
                .map_err(|_| {
                    ConsoleError::new("--messages 必须为带 role 和 content 的 JSON 消息数组")
                })?
                .unwrap_or_default();
            let agent = agent("").with_messages(messages);
            let schema = input
                .matches()
                .get_one::<String>("schema")
                .map(|schema| {
                    let schema = serde_json::from_str::<serde_json::Value>(schema)
                        .map_err(|_| console_error(crate::AiError::InvalidSchema))?;
                    let schema = JsonSchema::from_array(schema)
                        .and_then(|schema| schema.to_array())
                        .map_err(|_| console_error(crate::AiError::InvalidSchema))?;
                    if schema["type"] != "object" {
                        return Err(console_error(crate::AiError::InvalidSchema));
                    }
                    Ok(schema)
                })
                .transpose()?;
            if let Some(schema) = schema.as_ref() {
                let response = Ai::manager()
                    .map_err(console_error)?
                    .prompt_with_schema(
                        &agent,
                        prompt,
                        options,
                        Some(StructuredOutput {
                            schema,
                            strict: false,
                        }),
                    )
                    .await
                    .map_err(console_error)?;
                let structured = StructuredAgentResponse::parse(response).map_err(console_error)?;
                write_line(
                    output,
                    &serde_json::to_string(&structured).map_err(ConsoleError::from_source)?,
                )?;
            } else {
                let response = agent
                    .prompt_with(prompt, options)
                    .await
                    .map_err(console_error)?;
                write_line(output, &response.text)?;
            }
            Ok(0)
        })
    }
}

fn console_error(error: AiError) -> ConsoleError {
    ConsoleError::new(error_message(&error))
}

fn error_message(error: &AiError) -> String {
    let key = match error {
        AiError::DefaultNotConfigured => "default_not_configured",
        AiError::ProviderNotConfigured => "provider_not_configured",
        AiError::MissingCredential => "missing_credential",
        AiError::MissingModel => "missing_model",
        AiError::ModelsNotSupported => "models_not_supported",
        AiError::Transport => "transport",
        AiError::Timeout => "timeout",
        AiError::ProviderStatus(status) => {
            return localization::text(
                "ai::errors.console.provider_status",
                &[("status".into(), status.to_string())].into(),
                &error.to_string(),
            );
        }
        AiError::InvalidResponse => "invalid_response",
        AiError::Refused => "refused",
        AiError::InvalidSchema => "invalid_schema",
        AiError::InvalidPrompt => "invalid_prompt",
        _ => return error.to_string(),
    };
    localization::text(
        &format!("ai::errors.console.{key}"),
        &Default::default(),
        &error.to_string(),
    )
}

fn write_line(output: &mut Output, line: &str) -> Result<(), ConsoleError> {
    output
        .write_raw(&format!("{line}\n"))
        .map_err(ConsoleError::from_source)
}
