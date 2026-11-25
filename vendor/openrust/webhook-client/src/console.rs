use crate::{DatabaseWebhookStore, Error, models::WebhookCall};
use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    database::DatabaseManager,
    foundation::Application,
    support::facades::{Artisan, Config},
};

pub(crate) fn register() -> Result<(), ConsoleError> {
    let syntax = Syntax::new("webhook-client:prune")
        .about("清理当前范围内的过期 Webhook 记录或附件暂存对象")
        .arg(Arg::new("scope").long("scope").required(true))
        .arg(Arg::new("database").long("database"))
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("100")
                .value_parser(value_parser!(u32).range(1..=500)),
        )
        .arg(
            Arg::new("verification-audit")
                .long("verification-audit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("attachment-staging")
                .long("attachment-staging")
                .conflicts_with("verification-audit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("pretend")
                .long("pretend")
                .action(ArgAction::SetTrue),
        );
    Artisan::add(Box::new(Prune(CommandDefinition::new(syntax))))
}

struct Prune(CommandDefinition);

impl Command for Prune {
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
            let matches = input.matches();
            let staging = matches.get_flag("attachment-staging");
            let days = if staging {
                0
            } else {
                let value = Config::get("webhook-client.delete_after_days")
                    .map_err(ConsoleError::from_source)?;
                match value {
                    None => 30,
                    Some(serde_json::Value::Null) => {
                        output.info("Webhook 接收记录保留期清理已停用");
                        return Ok(0);
                    }
                    Some(value) => value
                        .as_u64()
                        .filter(|days| *days <= 36_500)
                        .and_then(|days| u32::try_from(days).ok())
                        .ok_or_else(|| {
                            ConsoleError::from_source(Error::InvalidConfig("delete_after_days"))
                        })?,
                }
            };
            let scope = matches.get_one::<String>("scope").expect("必填范围");
            let connection = matches.get_one::<String>("database").map(String::as_str);
            let limit = *matches.get_one::<u32>("limit").expect("默认批次上限");
            let manager = app
                .make::<DatabaseManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let connection = manager
                .connection(connection)
                .map_err(ConsoleError::from_source)?;
            if matches.get_flag("verification-audit") {
                let audit = crate::verification::DatabaseVerificationAudit::new(connection, scope)
                    .map_err(ConsoleError::from_source)?;
                let pretend = matches.get_flag("pretend");
                let rows = audit
                    .prune(days, limit, pretend)
                    .await
                    .map_err(ConsoleError::from_source)?;
                let action = if pretend { "可清理" } else { "已清理" };
                output.info(&format!("{action} {rows} 条 Webhook 验签判定记录"));
                return Ok(0);
            }
            let store =
                DatabaseWebhookStore::new(connection, scope).map_err(ConsoleError::from_source)?;
            if staging {
                let pretend = matches.get_flag("pretend");
                let report = store
                    .prune_attachment_staging(limit, pretend)
                    .await
                    .map_err(ConsoleError::from_source)?;
                if pretend {
                    output.info(&format!("可清理 {} 个 Webhook 附件对象", report.candidates));
                } else {
                    output.info(&format!(
                        "已清理 {} 个 Webhook 附件对象，{} 个待重试",
                        report.removed, report.failed
                    ));
                }
                return Ok(i32::from(report.failed > 0));
            }
            if matches.get_flag("pretend") {
                let ids = WebhookCall::prunable(&store, days, limit)
                    .await
                    .map_err(ConsoleError::from_source)?;
                output.info(&format!("可清理 {} 条 Webhook 接收记录", ids.len()));
                return Ok(0);
            }
            let removed = store
                .prune(days, limit)
                .await
                .map_err(ConsoleError::from_source)?;
            output.info(&format!("已清理 {removed} 条 Webhook 接收记录"));
            Ok(0)
        })
    }
}
