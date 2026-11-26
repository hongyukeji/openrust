use futures_util::StreamExt;
use openrust::{
    database::migrations::MigrationEntry,
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    json_schema::{JsonSchemaTypeFactory, types::ObjectType},
    pagination::Cursor,
    support::facades::{Config, Route},
    testing::TestApp,
};
use openrust_ai::{
    Agent, Ai, AiError, ConversationParticipant, HasStructuredOutput, Message, PromptOptions,
    RemembersConversations, StreamEventData,
};
use serde_json::json;

#[path = "../database/migrations/2026_01_11_000001_create_agent_conversations_table.rs"]
mod migration;

static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "database/migrations/2026_01_11_000001_create_agent_conversations_table.rs",
    &migration::Definition,
)];

struct Support;

impl Agent for Support {
    fn instructions(&self) -> &str {
        "Support the customer."
    }
}

impl RemembersConversations for Support {}

impl HasStructuredOutput for Support {
    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType {
        schema.object([("answer", schema.string().required(true).into())])
    }
}

#[tokio::test]
async fn conversation_history_is_owned_and_paginates_newest_first()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| {
        builder
            .with_migrations(MIGRATIONS)
            .with_providers(openrust_ai::openrust_providers())
            .with_config(|_, _| {
                Ok([(
                    "database".into(),
                    json!({
                        "default":"sqlite",
                        "connections":{
                            "sqlite":{"driver":"sqlite","database":":memory:"},
                            "agent_store":{"driver":"sqlite","database":":memory:"},
                        },
                        "migrations":{"table":"migrations"},
                    }),
                )]
                .into())
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/conversation", || async {
                    Config::set("ai.conversations.max_messages", json!(2)).unwrap();
                    let fake = Support::fake(["one", "two", "three"].map(str::to_owned)).unwrap();
                    let owner = ConversationParticipant::new("Customer", "Account-A");
                    let other = ConversationParticipant::new("Customer", "account-a");
                    let first = Support
                        .for_participant(owner.clone())
                        .prompt("first")
                        .await
                        .unwrap();
                    let id = first.conversation_id;
                    let third_party = Support.r#continue(&id, other.clone()).prompt("read").await;
                    assert!(matches!(third_party, Err(AiError::ConversationNotFound)));
                    let second = Support
                        .r#continue(&id, owner.clone())
                        .prompt("second")
                        .await
                        .unwrap();
                    assert_eq!(second.response.text, "two");
                    let third = Support
                        .continue_last_conversation(owner.clone())
                        .prompt("third")
                        .await
                        .unwrap();
                    assert_eq!(third.conversation_id, id);
                    let calls = fake.recorded();
                    assert_eq!(calls.len(), 3);
                    assert_eq!(
                        calls[1].messages,
                        [Message::user("first"), Message::assistant("one")]
                    );
                    assert_eq!(
                        calls[2].messages,
                        [Message::user("second"), Message::assistant("two")]
                    );

                    let store = Ai::conversation_store().unwrap();
                    let page = store
                        .paginate_conversation_messages(&id, &owner, 2, None)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(page.messages[0].message, Message::assistant("three"));
                    assert_eq!(page.messages[1].message, Message::user("third"));
                    let cursor =
                        Cursor::from_encoded(page.next_cursor.as_deref().unwrap()).unwrap();
                    let page = store
                        .paginate_conversation_messages(&id, &owner, 2, Some(&cursor))
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(page.messages[0].message, Message::assistant("two"));
                    assert!(
                        store
                            .paginate_conversation_messages(&id, &other, 2, None)
                            .await
                            .unwrap()
                            .is_none()
                    );
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })?;
                Route::get("/conversation-stream", || async {
                    let fake = Support::fake(
                        [
                            r#"{"answer":"one"}"#,
                            r#"{"answer":"two"}"#,
                            "aborted",
                            "invalid",
                        ]
                        .map(str::to_owned),
                    )
                    .unwrap();
                    let owner = ConversationParticipant::new("Customer", "StreamAccount");
                    let prompt = Support
                        .for_participant(owner.clone())
                        .prompt_structured("first")
                        .await
                        .unwrap();
                    assert_eq!(prompt.response.structured["answer"], "one");
                    let id = prompt.conversation_id;
                    let response = Support
                        .r#continue(&id, owner.clone())
                        .stream_structured("second", PromptOptions::default())
                        .await
                        .unwrap();
                    assert_eq!(response.conversation_id, id);
                    let mut events = response.response.into_stream();
                    let store = Ai::conversation_store().unwrap();
                    let mut ended = false;
                    while let Some(event) = events.next().await {
                        let event = event.unwrap();
                        let history = store
                            .get_latest_conversation_messages(&id, &owner, 10)
                            .await
                            .unwrap()
                            .unwrap();
                        if matches!(event.data, StreamEventData::StreamEnd { .. }) {
                            assert_eq!(history[3], Message::assistant(r#"{"answer":"two"}"#));
                            ended = true;
                        } else {
                            assert_eq!(history.len(), 2);
                        }
                    }
                    assert!(ended);
                    let response = Support
                        .for_participant(owner.clone())
                        .stream("aborted")
                        .await
                        .unwrap();
                    let mut events = response.response.into_stream();
                    assert!(events.next().await.unwrap().is_ok());
                    drop(events);
                    assert!(
                        !store
                            .conversation_belongs_to(&response.conversation_id, &owner)
                            .await
                            .unwrap()
                    );
                    assert!(matches!(
                        Support
                            .r#continue(&id, owner.clone())
                            .prompt_structured("invalid")
                            .await,
                        Err(AiError::InvalidStructuredResponse)
                    ));
                    assert_eq!(
                        store
                            .get_latest_conversation_messages(&id, &owner, 10)
                            .await
                            .unwrap()
                            .unwrap()
                            .len(),
                        4
                    );
                    let recorded = fake.recorded();
                    assert_eq!(recorded[1].messages.len(), 2);
                    assert!(recorded[0].schema.is_some() && recorded[1].schema.is_some());
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    std::fs::write(
        app.root().join(".env.testing"),
        "AI_CONVERSATIONS_CONNECTION=agent_store\n",
    )?;
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    assert_eq!(app.get("/conversation").await?.status(), StatusCode::OK);
    assert_eq!(
        app.get("/conversation-stream").await?.status(),
        StatusCode::OK
    );
    app.artisan(["migrate:rollback", "--force"])
        .await
        .assert_successful();
    Ok(())
}
