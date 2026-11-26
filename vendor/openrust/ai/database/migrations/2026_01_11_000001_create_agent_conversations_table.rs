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
            let mut schema = context.get_schema_builder();
            schema
                .create("agent_conversations", |table| {
                    table.string("id", 36).primary();
                    table.string("participant_type", 255);
                    table.string("participant_id", 255);
                    table.string("title", 255);
                    table.big_integer("created_at");
                    table.big_integer("updated_at");
                })
                .await?;
            schema
                .create("agent_conversation_messages", |table| {
                    table.string("id", 36).primary();
                    table.string("conversation_id", 36);
                    table.string("participant_type", 255);
                    table.string("participant_id", 255);
                    table.string("participant_key", 64);
                    table.string("agent", 255);
                    table.string("role", 25);
                    table.long_text("content");
                    table.text("attachments");
                    table.long_text("steps");
                    table.text("usage");
                    table.text("meta");
                    table.string("status", 25);
                    table.big_integer("created_at");
                    table.big_integer("updated_at");
                    table.index(
                        &["conversation_id", "id"],
                        Some("agent_conversation_id_idx"),
                    );
                    table.index(
                        &["participant_key", "id"],
                        Some("agent_conversation_participant_idx"),
                    );
                    table
                        .foreign(&["conversation_id"], Some("agent_conversation_fk"))
                        .references("id")
                        .on("agent_conversations")
                        .cascade_on_delete();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = context.get_schema_builder();
            schema.drop_if_exists("agent_conversation_messages").await?;
            schema.drop_if_exists("agent_conversations").await
        })
    }
}
