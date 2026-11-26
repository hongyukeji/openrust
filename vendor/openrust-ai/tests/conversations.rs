use futures_util::StreamExt;
use openrust::{
    database::{Db, Value, migrations::MigrationEntry},
    filesystem::{Storage, Visibility},
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    json_schema::{JsonSchemaTypeFactory, types::ObjectType},
    pagination::Cursor,
    support::facades::{Config, Route},
    testing::TestApp,
};
use openrust_ai::{
    Agent, Ai, AiError, Attachment, ConversationParticipant, DocumentAttachment,
    HasStructuredOutput, Message, PromptOptions, RemembersConversations, StreamEventData,
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

struct ReportAgent(Vec<Attachment>);

impl Agent for ReportAgent {
    fn instructions(&self) -> &str {
        "Read the report."
    }

    fn attachments(&self) -> &[Attachment] {
        &self.0
    }
}

impl RemembersConversations for ReportAgent {}

#[tokio::test]
async fn conversation_history_is_owned_and_paginates_newest_first()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    assert_conversation_history("AI_CONVERSATIONS_CONNECTION=agent_store\n").await
}

#[tokio::test]
async fn conversation_history_uses_configured_tables()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    assert_conversation_history(concat!(
        "AI_CONVERSATIONS_CONNECTION=agent_store\n",
        "AI_CONVERSATIONS_TABLE=support_threads\n",
        "AI_CONVERSATION_MESSAGES_TABLE=support_messages\n",
        "AI_CONVERSATION_ATTACHMENTS_DISK=ai_private\n",
    ))
    .await
}

async fn assert_conversation_history(
    env: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| {
        builder
            .with_migrations(MIGRATIONS)
            .with_providers(openrust_ai::openrust_providers())
            .with_config(|base, _| {
                Ok([
                    (
                        "filesystems".into(),
                        json!({"default":"public","disks":{
                            "local":{"driver":"local","root":base.join("private"),"throw":true},
                            "ai_private":{"driver":"local","root":base.join("ai-files"),"throw":true},
                        }}),
                    ),
                    (
                        "database".into(),
                        json!({
                            "default":"sqlite",
                            "connections":{
                                "sqlite":{"driver":"sqlite","database":":memory:"},
                                "agent_store":{"driver":"sqlite","database":":memory:"},
                            },
                            "migrations":{"table":"migrations"},
                        }),
                    ),
                ]
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
                Route::get("/conversation-files", || async {
                    let attachment =
                        DocumentAttachment::from_bytes("report.pdf", b"%PDF-1.4".as_slice())
                            .unwrap()
                            .into();
                    let fake = ReportAgent::fake(["first".into(), "second".into()]).unwrap();
                    let owner = ConversationParticipant::new("Customer", "FileAccount");
                    let first = ReportAgent(vec![attachment])
                        .for_participant(owner.clone())
                        .prompt("")
                        .await
                        .unwrap();
                    ReportAgent(Vec::new())
                        .r#continue(&first.conversation_id, owner.clone())
                        .prompt("Follow up")
                        .await
                        .unwrap();
                    let calls = fake.recorded();
                    assert_eq!(calls[1].messages[0].attachments, calls[0].attachments);
                    let store = Ai::conversation_store().unwrap();
                    let page = store
                        .paginate_conversation_messages(&first.conversation_id, &owner, 10, None)
                        .await
                        .unwrap()
                        .unwrap();
                    let source = &page.messages[3].message;
                    assert_eq!(source.attachments, calls[0].attachments);
                    assert_eq!(json!(source), json!({"role":"user","content":""}));
                    let messages = Config::get("ai.conversations.tables.messages")
                        .unwrap()
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_owned();
                    let row = Db::connection(Some("agent_store"))
                        .unwrap()
                        .table(&messages)
                        .r#where("id", "=", page.messages[3].id.as_str())
                        .first()
                        .await
                        .unwrap()
                        .unwrap();
                    let Some(Value::Text(attachments)) = row.get("attachments") else {
                        panic!("attachments metadata missing");
                    };
                    let references: serde_json::Value = serde_json::from_str(attachments).unwrap();
                    let reference = &references[0];
                    assert!(reference.get("base64").is_none());
                    let selected = Config::get("ai.conversations.attachments.disk")
                        .unwrap().unwrap();
                    assert_eq!(reference["disk"], selected);
                    assert_eq!(reference["name"], "report.pdf");
                    let path = reference["path"].as_str().unwrap();
                    let disk = Storage::disk(selected.as_str()).unwrap();
                    assert_eq!(disk.get(path).await.unwrap().unwrap(), b"%PDF-1.4");
                    assert_eq!(
                        disk.get_visibility(path).await.unwrap(),
                        Visibility::Private
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
                })?;
                Ok(())
            }))
    })?;
    std::fs::write(app.root().join(".env.testing"), env)?;
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    assert_eq!(app.get("/conversation").await?.status(), StatusCode::OK);
    assert_eq!(
        app.get("/conversation-files").await?.status(),
        StatusCode::OK
    );
    assert_eq!(
        app.get("/conversation-stream").await?.status(),
        StatusCode::OK
    );
    app.artisan(["migrate:rollback", "--force"])
        .await
        .assert_successful();
    Ok(())
}
