use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

const CONVERSATIONS: &str = "agent_conversations";
const MESSAGES: &str = "agent_conversation_messages";

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
            let (conversations, messages) = table_names(context.configuration())?;
            let mut schema = context.get_schema_builder();
            schema
                .create(&conversations, |table| {
                    table.string("id", 36).primary();
                    table.string("participant_type", 255);
                    table.string("participant_id", 255);
                    table.string("title", 255);
                    table.big_integer("created_at");
                    table.big_integer("updated_at");
                })
                .await?;
            let default_messages = messages == MESSAGES;
            schema
                .create(&messages, |table| {
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
                        default_messages.then_some("agent_conversation_id_idx"),
                    );
                    table.index(
                        &["participant_key", "id"],
                        default_messages.then_some("agent_conversation_participant_idx"),
                    );
                    table
                        .foreign(
                            &["conversation_id"],
                            default_messages.then_some("agent_conversation_fk"),
                        )
                        .references("id")
                        .on(&conversations)
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
            let (conversations, messages) = table_names(context.configuration())?;
            let mut schema = context.get_schema_builder();
            schema.drop_if_exists(&messages).await?;
            schema.drop_if_exists(&conversations).await
        })
    }
}

fn table_names(config: &dyn Repository) -> DatabaseResult<(String, String)> {
    let conversations = configured_table(
        config,
        "ai.conversations.tables.conversations",
        CONVERSATIONS,
    )?;
    let messages = configured_table(config, "ai.conversations.tables.messages", MESSAGES)?;
    if conversations.eq_ignore_ascii_case(&messages) {
        return Err(DatabaseError::InvalidConfiguration(
            "ai.conversations.tables.messages",
        ));
    }
    Ok((conversations, messages))
}

fn configured_table(
    config: &dyn Repository,
    key: &'static str,
    default: &str,
) -> DatabaseResult<String> {
    match config.get(key) {
        None | Some(serde_json::Value::Null) => Ok(default.to_owned()),
        Some(serde_json::Value::String(name)) if valid_table_name(&name) => Ok(name),
        _ => Err(DatabaseError::InvalidConfiguration(key)),
    }
}

fn valid_table_name(name: &str) -> bool {
    let (schema, table) = name.rsplit_once('.').unwrap_or(("", name));
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    };
    valid_segment(table) && (!name.contains('.') || valid_segment(schema))
}
