use openrust::{
    database::migrations::MigrationEntry,
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    pagination::Cursor,
    support::facades::{Config, Route},
    testing::TestApp,
};
use openrust_ai::{Agent, Ai, AiError, ConversationParticipant, Message, RemembersConversations};
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
    app.artisan(["migrate:rollback", "--force"])
        .await
        .assert_successful();
    Ok(())
}
