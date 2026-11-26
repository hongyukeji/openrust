use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
    support::facades::Artisan,
};
use serde_json::json;

use crate::{Agent, Ai, PromptOptions, agent};

mod select_model;

pub(crate) fn register() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(Models {
        definition: CommandDefinition::from_signature(
            "ai:models {provider? : AI 连接名称} {--json : 输出 JSON}",
        )?,
    }))?;
    Artisan::add(Box::new(Prompt {
        definition: CommandDefinition::from_signature(
            "ai:prompt {prompt : 提示词} {--provider= : AI 连接名称} {--model= : 上游模型 ID}",
        )?,
    }))?;
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
                    output.error(&error.to_string());
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
            let response = agent("")
                .prompt_with(prompt, options)
                .await
                .map_err(console_error)?;
            write_line(output, &response.text)?;
            Ok(0)
        })
    }
}

fn console_error(error: crate::AiError) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

fn write_line(output: &mut Output, line: &str) -> Result<(), ConsoleError> {
    output
        .write_raw(&format!("{line}\n"))
        .map_err(ConsoleError::from_source)
}
