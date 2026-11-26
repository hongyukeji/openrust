use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct Definition;

impl Migration for Definition {
    fn get_connection(&self, config: &dyn Repository) -> DatabaseResult<Option<String>> {
        match config.get("ai.conversations.connection") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(name)) if !name.trim().is_empty() => Ok(Some(name)),
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
            context
                .get_schema_builder()
                .create("agent_tool_approvals", |table| {
                    table.string("id", 26).primary();
                    table.string("conversation_id", 36);
                    table.string("participant_key", 64);
                    table.string("state", 16);
                    table.string("claim_id", 26).nullable();
                    table.long_text("snapshot");
                    table.big_integer("revision");
                    table.big_integer("expires_at");
                    table.big_integer("created_at");
                    table.big_integer("updated_at");
                    table.index(
                        &["participant_key", "conversation_id"],
                        Some("agent_tool_approvals_scope_idx"),
                    );
                    table.index(&["expires_at"], Some("agent_tool_approvals_expiry_idx"));
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .drop_if_exists("agent_tool_approvals")
                .await
        })
    }
}
