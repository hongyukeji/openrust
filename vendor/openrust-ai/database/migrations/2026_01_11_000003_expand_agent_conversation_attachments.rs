use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};
use serde_json::Value;

pub struct Definition;

impl Migration for Definition {
    fn get_connection(&self, config: &dyn Repository) -> DatabaseResult<Option<String>> {
        match config.get("ai.conversations.connection") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(Some(name)),
            _ => Err(DatabaseError::InvalidConfiguration(
                "ai.conversations.connection",
            )),
        }
    }

    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if !matches!(context.get_driver_name(), "mysql" | "mariadb") {
                return Ok(());
            }
            const TABLE_KEY: &str = "ai.conversations.tables.messages";
            let messages = match context.configuration().get(TABLE_KEY) {
                None | Some(Value::Null) => "agent_conversation_messages".to_owned(),
                Some(Value::String(name)) if !name.trim().is_empty() => name,
                _ => return Err(DatabaseError::InvalidConfiguration(TABLE_KEY)),
            };
            context
                .get_schema_builder()
                .table(&messages, |table| {
                    table.long_text("attachments").change();
                })
                .await
        })
    }

    fn down<'a>(&'a self, _: &'a mut MigrationContext) -> DatabaseFuture<'a, DatabaseResult<()>> {
        // 保留扩容后的列，避免回滚时截断已保存的附件。
        Box::pin(async { Ok(()) })
    }
}
