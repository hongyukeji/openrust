use openrust::{
    container::Callback,
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Builder, ModelKeyType, ModelMetadata, ModelState, Observer, Scope,
            concerns::{ModelEvent, ModelEventResponse},
        },
    },
    pagination::PageOptions,
    testing::TestApp,
};
use serde_json::json;

#[path = "../app/mod.rs"]
#[allow(dead_code, unused_imports, reason = "测试只调用默认应用的分层入口")]
mod app;

use app::{
    models::{User, model::Model},
    repositories::{
        repository::{EloquentRepository, Repository},
        user_repository::UserRepository,
    },
    services::{service::TransactionalService, user_service::UserService},
};

struct Contact(ModelState);

struct Visible;

impl Scope<Contact> for Visible {
    fn apply(&self, query: &mut Builder<'_, Contact>) -> DatabaseResult<()> {
        query.r#where("visible", "=", true);
        Ok(())
    }
}

impl Model for Contact {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("contacts")
            .connection("contacts")
            .primary_key("code")
            .incrementing(false)
            .timestamps(None, None)
            .soft_deletes("deleted_at")
            .fillable(&["code", "name", "visible"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }

    fn get_key_type(&self) -> ModelKeyType {
        ModelKeyType::String
    }

    fn global_scopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("visible", &Visible)]
    }
}

struct NamedContacts;

impl Repository for NamedContacts {
    type Model = Contact;

    fn query(&self) -> DatabaseResult<Builder<'static, Contact>> {
        let mut query = Contact::query()?;
        query.r#where("name", "=", "Changed");
        Ok(query)
    }
}

struct ContactObserver;

impl Observer<Contact> for ContactObserver {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Updating];

    fn updating<'a>(
        &'a self,
        contact: &'a mut Contact,
        _connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if contact.get_attribute("name") == Some(&"Rejected".into()) {
                return Ok(ModelEventResponse::Cancel);
            }
            Ok(ModelEventResponse::Continue)
        })
    }
}

fn database_configuration() -> serde_json::Value {
    match std::env::var("LAYERING_TEST_DRIVER").as_deref() {
        Ok("mysql") => json!({"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}),
        Ok("pgsql") => json!({"url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()}),
        Ok("sqlite") | Err(std::env::VarError::NotPresent) => {
            json!({"driver":"sqlite","database":":memory:"})
        }
        other => panic!("未知验收数据库：{other:?}"),
    }
}

#[tokio::test]
async fn application_entries_reuse_and_override_eloquent() {
    let mut fixture = TestApp::create(|builder| {
        builder.with_config(|_, _| {
            let mut database = database_configuration();
            database["prefix"] = json!("layer_");
            Ok([(
                "database".into(),
                json!({
                    "default":"users",
                    "connections":{"contacts":database,"users":database}
                }),
            )]
            .into())
        })
    })
    .unwrap();
    fixture.boot().await.unwrap();
    Contact::observe(fixture.app(), ContactObserver).unwrap();
    let connection = fixture
        .app()
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("contacts"))
        .unwrap();
    let mut schema = connection.get_schema_builder();
    schema
        .create("contacts", |table| {
            table.string("code", 30).primary();
            table.string("name", 120);
            table.boolean("visible");
            table.timestamp("deleted_at", None).nullable();
        })
        .await
        .unwrap();
    fixture
        .app()
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("users"))
        .unwrap()
        .get_schema_builder()
        .create("users", |table| {
            table.big_increments("id");
            table.string("name", 255);
            table.string("email", 255);
            table.timestamp("created_at", None).nullable();
            table.timestamp("updated_at", None).nullable();
        })
        .await
        .unwrap();

    fixture
        .app()
        .call(
            Callback::new(&[], |_| async move {
                verify_repository(&connection).await;
                verify_service().await;
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
}

async fn verify_repository(connection: &Connection) {
    let contacts = EloquentRepository::<Contact>::new();
    let mut contact = contacts
        .create(contact_attributes("member", true))
        .await
        .unwrap();
    contacts
        .create(contact_attributes("hidden", false))
        .await
        .unwrap();
    assert!(contacts.find("hidden").await.unwrap().is_none());
    assert!(NamedContacts.find("member").await.unwrap().is_none());
    assert!(matches!(
        contacts.find_or_fail("missing").await,
        Err(DatabaseError::ModelNotFound("contacts"))
    ));

    assert!(
        !contacts
            .update(&mut contact, [("name".into(), "Rejected".into())].into())
            .await
            .unwrap()
    );
    assert_eq!(
        contacts
            .find_or_fail("member")
            .await
            .unwrap()
            .get_attribute("name"),
        Some(&"Original".into())
    );
    assert!(
        contacts
            .update(&mut contact, [("name".into(), "Changed".into())].into())
            .await
            .unwrap()
    );
    let page = NamedContacts
        .paginate(NamedContacts.query().unwrap(), PageOptions::default())
        .await
        .unwrap();
    assert_eq!(page.total(), 1);
    let mut contact = NamedContacts.find_or_fail("member").await.unwrap();
    assert!(contacts.delete(&mut contact).await.unwrap());
    assert!(contacts.find("member").await.unwrap().is_none());
    let stored = connection
        .table("contacts")
        .r#where("code", "=", "member")
        .first()
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(stored.get("deleted_at"), Some(value) if *value != Value::Null));
    connection
        .get_schema_builder()
        .drop("contacts")
        .await
        .unwrap();
    assert!(contacts.find("member").await.is_err());
}

fn contact_attributes(code: &str, visible: bool) -> openrust::database::eloquent::Attributes {
    [
        ("code".into(), code.into()),
        ("name".into(), "Original".into()),
        ("visible".into(), visible.into()),
    ]
    .into()
}

async fn verify_service() {
    let user = UserRepository
        .create(
            [
                (User::NAME.into(), "Before".into()),
                (User::EMAIL.into(), "member@example.test".into()),
            ]
            .into(),
        )
        .await
        .unwrap();
    let Value::Integer(id) = user.get_key().unwrap() else {
        panic!("用户主键应为整数");
    };
    let service = UserService::new(UserRepository);
    assert!(service.rename(*id, "After").await.unwrap());
    assert!(!service.rename(*id, "After").await.unwrap());

    let id = *id;
    let failed: DatabaseResult<()> = service
        .transaction(
            |transaction| {
                Box::pin(async move {
                    let mut user = UserRepository.find_for_update(transaction, id).await?;
                    assert!(
                        user.update_in_transaction(
                            [(User::NAME.into(), "Rolled back".into())].into(),
                            transaction,
                        )
                        .await?
                    );
                    Err(DatabaseError::InvalidQuery("验收回滚"))
                })
            },
            1,
        )
        .await;
    assert!(matches!(
        failed,
        Err(DatabaseError::InvalidQuery("验收回滚"))
    ));
    let stored = service
        .find_by_email("member@example.test")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.get_attribute(User::NAME), Some(&"After".into()));
    assert!(matches!(
        service.rename(id + 1, "Missing").await,
        Err(DatabaseError::ModelNotFound("users"))
    ));
}
