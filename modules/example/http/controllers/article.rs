//! 文章 CRUD：所有查询受当前账户约束，写入在主库事务内完成。

use crate::modules::example::events::article::ArticleChanged;
use crate::modules::example::http::requests::article::ArticleInput;
#[cfg(feature = "encryption")]
use crate::modules::example::http::requests::article::PrivateNoteInput;
#[cfg(feature = "signing")]
use crate::modules::example::http::requests::article::ShareTokenInput;
use crate::modules::example::http::resources::article::ArticleResource;
#[cfg(feature = "signing")]
use crate::modules::example::http::resources::article::ArticleShare;
#[cfg(feature = "encryption")]
use crate::modules::example::http::resources::article::PrivateNoteResource;
use crate::modules::example::models::article;
use crate::modules::example::services::article_write::ArticleWriter;
#[cfg(feature = "storage-local")]
use crate::modules::example::services::attachments::ArticleAttachments;
use crate::modules::example::services::telemetry::{self, ArticleOperation};
#[cfg(feature = "http-client")]
use crate::modules::example::services::upstream::{ArticleSummary, ArticleSummaryService};
use openrust::audit::{Audit, AuditEvent, AuditOutcome, AuditSubject};
use openrust::auth::{CurrentUser, QueryRule, QueryScope};
use openrust::database::SortDirection;
use openrust::database::sea_orm::{
    ColumnTrait, Condition, EntityTrait, QueryFilter, sea_query::Expr,
};
#[cfg(feature = "encryption")]
use openrust::encryption::Encrypter;
use openrust::events::Events;
use openrust::http::Created;
use openrust::http::Json;
use openrust::http::PageParams;
use openrust::http::Paginated;
use openrust::http::Path;
use openrust::http::Validated;
#[cfg(feature = "encryption")]
use openrust::http::axum::{
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use openrust::prelude::{Db, Inject};
use openrust::routing::Resource;
use openrust::routing::ResourceActions;
#[cfg(feature = "signing")]
use openrust::signing::SignedLinks;
use openrust::{Error, Result};

#[cfg(feature = "signing")]
const SHARE_PURPOSE: &str = "example.article.read";

fn article_rule() -> QueryRule<article::Entity> {
    QueryRule::new(|user| {
        Ok(QueryScope::new(
            article::Column::OwnerId.eq(user.integer_id()?),
        ))
    })
}

fn not_found() -> Error {
    Error::NotFound("article not found".into())
}

pub(crate) struct Articles;

impl Resource for Articles {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions
            .list(index)
            .get(show)
            .create(store)
            .update(update)
            .delete(destroy)
    }
}

pub(crate) async fn index(
    user: CurrentUser,
    Inject(db): Inject<Db>,
) -> Result<Paginated<ArticleResource>> {
    let page = db
        .table("example_articles")?
        .where_eq("owner_id", user.integer_id()?)?
        .order_by("id", SortDirection::Asc)?
        .paginate(15)
        .await?;
    let rows = page
        .items
        .iter()
        .map(ArticleResource::from_query_row)
        .collect::<Result<Vec<_>>>()?;
    Ok(Paginated::new(
        rows,
        PageParams {
            page: page.current_page,
            per_page: page.per_page,
        },
        page.total,
    ))
}

pub(crate) async fn show(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(audit): Inject<Audit>,
    Path(id): Path<i64>,
) -> Result<Json<ArticleResource>> {
    let model = article_rule()
        .select(&user, Condition::all().add(article::Column::Id.eq(id)))?
        .one(&db.read_connection()?)
        .await?
        .ok_or_else(not_found)?;
    audit
        .record(
            AuditEvent::new(
                "example.article.view",
                AuditSubject::new("article", id.to_string())?,
                AuditOutcome::Succeeded,
            )?
            .actor(AuditSubject::new("user", user.integer_id()?.to_string())?),
        )
        .await?;
    Ok(Json(model.into()))
}

#[cfg(feature = "http-client")]
pub(crate) async fn external_summary(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(upstream): Inject<ArticleSummaryService>,
    Path(id): Path<i64>,
) -> Result<Json<ArticleSummary>> {
    article_rule()
        .select(&user, Condition::all().add(article::Column::Id.eq(id)))?
        .one(&db.primary_connection()?)
        .await?
        .ok_or_else(not_found)?;
    Ok(Json(upstream.fetch(id).await?))
}

pub(crate) async fn store(
    user: CurrentUser,
    Inject(writer): Inject<ArticleWriter>,
    Validated(Json(input)): Validated<Json<ArticleInput>>,
) -> Result<Created<Json<ArticleResource>>> {
    let model = writer.create(user.integer_id()?, input).await?;
    Ok(Created(Json(model.into())))
}

pub(crate) async fn update(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(events): Inject<Events>,
    Path(id): Path<i64>,
    Validated(Json(input)): Validated<Json<ArticleInput>>,
) -> Result<Json<ArticleResource>> {
    let model = telemetry::observe(ArticleOperation::Update, async {
        db.transaction(move |transaction| {
            Box::pin(async move {
                let result = article::Entity::update_many()
                    .col_expr(article::Column::Title, Expr::value(input.title))
                    .col_expr(article::Column::Body, Expr::value(input.body))
                    .filter(article::Column::Id.eq(id))
                    .filter(article::Column::OwnerId.eq(user.integer_id()?))
                    .exec(transaction)
                    .await
                    .map_err(Error::internal)?;
                if result.rows_affected != 1 {
                    return Ok(None);
                }
                let model = article::Entity::find()
                    .filter(article::Column::Id.eq(id))
                    .filter(article::Column::OwnerId.eq(user.integer_id()?))
                    .one(transaction)
                    .await
                    .map_err(Error::internal)?;
                if let Some(model) = &model {
                    events
                        .dispatch_in_transaction(
                            transaction,
                            &ArticleChanged {
                                owner_id: user.integer_id()?,
                                article_id: model.id,
                                action: "updated",
                            },
                        )
                        .await?;
                }
                Ok(model)
            })
        })
        .await?
        .ok_or_else(not_found)
    })
    .await?;
    Ok(Json(model.into()))
}

pub(crate) async fn destroy(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(events): Inject<Events>,
    #[cfg(feature = "storage-local")] Inject(attachments): Inject<ArticleAttachments>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>> {
    let owner_id = user.integer_id()?;
    #[cfg(feature = "storage-local")]
    let transaction_attachments = attachments.clone();
    telemetry::observe(ArticleOperation::Delete, async {
        let deleted = db
            .transaction(move |transaction| {
                Box::pin(async move {
                    let result = article::Entity::delete_many()
                        .filter(article::Column::Id.eq(id))
                        .filter(article::Column::OwnerId.eq(user.integer_id()?))
                        .exec(transaction)
                        .await
                        .map_err(Error::internal)?;
                    if result.rows_affected == 1 {
                        #[cfg(feature = "storage-local")]
                        transaction_attachments
                            .record_deleted(transaction, owner_id, id)
                            .await?;
                        events
                            .dispatch_in_transaction(
                                transaction,
                                &ArticleChanged {
                                    owner_id: user.integer_id()?,
                                    article_id: id,
                                    action: "deleted",
                                },
                            )
                            .await?;
                    }
                    Ok(result.rows_affected == 1)
                })
            })
            .await?;
        if !deleted {
            return Err(not_found());
        }
        #[cfg(feature = "storage-local")]
        attachments.cleanup_deleted(owner_id, id).await?;
        #[cfg(not(feature = "storage-local"))]
        let _ = owner_id;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[cfg(feature = "encryption")]
pub(crate) async fn put_private_note(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(encryptor): Inject<Encrypter>,
    Path(id): Path<i64>,
    Validated(Json(input)): Validated<Json<PrivateNoteInput>>,
) -> Result<Response> {
    let context = format!("example.article.private_note:{}:{id}", user.integer_id()?);
    let ciphertext = encryptor.encrypt_with_context(input.note.as_bytes(), context.as_bytes())?;
    let updated = db
        .transaction(move |transaction| {
            Box::pin(async move {
                let result = article::Entity::update_many()
                    .col_expr(
                        article::Column::PrivateNoteCiphertext,
                        Expr::value(ciphertext),
                    )
                    .filter(article::Column::Id.eq(id))
                    .filter(article::Column::OwnerId.eq(user.integer_id()?))
                    .exec(transaction)
                    .await
                    .map_err(Error::internal)?;
                Ok(result.rows_affected == 1)
            })
        })
        .await?;
    if !updated {
        return Err(not_found());
    }
    Ok(private_note_response(input.note))
}

#[cfg(feature = "encryption")]
pub(crate) async fn get_private_note(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(encryptor): Inject<Encrypter>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let model = article_rule()
        .select(&user, Condition::all().add(article::Column::Id.eq(id)))?
        .one(&db.primary_connection()?)
        .await?
        .ok_or_else(not_found)?;
    let ciphertext = model.private_note_ciphertext.ok_or_else(not_found)?;
    let context = format!("example.article.private_note:{}:{id}", user.integer_id()?);
    let bytes = encryptor
        .decrypt_with_context(&ciphertext, context.as_bytes())
        .map_err(|_| {
            Error::internal(std::io::Error::other("stored article private note invalid"))
        })?;
    let note = String::from_utf8(bytes).map_err(|_| {
        Error::internal(std::io::Error::other("stored article private note invalid"))
    })?;
    Ok(private_note_response(note))
}

#[cfg(feature = "encryption")]
fn private_note_response(note: String) -> Response {
    let mut response = Json(PrivateNoteResource { note }).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

#[cfg(feature = "signing")]
pub(crate) async fn issue_share(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(links): Inject<SignedLinks>,
    Path(id): Path<i64>,
) -> Result<Json<ArticleShare>> {
    article_rule()
        .select(&user, Condition::all().add(article::Column::Id.eq(id)))?
        .one(&db.read_connection()?)
        .await?
        .ok_or_else(not_found)?;
    let token = links.issue(SHARE_PURPOSE, &format!("{}:{id}", user.integer_id()?), 300)?;
    Ok(Json(ArticleShare {
        token: token.expose().to_owned(),
    }))
}

#[cfg(feature = "signing")]
pub(crate) async fn revoke_share(
    user: CurrentUser,
    Inject(links): Inject<SignedLinks>,
    Path(id): Path<i64>,
    Json(input): Json<ShareTokenInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = links.verify(&input.token, SHARE_PURPOSE).await?;
    let (owner_id, article_id) = parse_share_subject(claims.subject())?;
    if owner_id != user.integer_id()? || article_id != id {
        return Err(Error::Unauthorized);
    }
    links.revoke(&input.token).await?;
    Ok(Json(serde_json::json!({ "status": "revoked" })))
}

#[cfg(feature = "signing")]
pub(crate) async fn shared_show(
    Inject(db): Inject<Db>,
    Inject(links): Inject<SignedLinks>,
    Path(token): Path<String>,
) -> Result<Json<ArticleResource>> {
    let claims = links.verify(&token, SHARE_PURPOSE).await?;
    let (owner_id, article_id) = parse_share_subject(claims.subject())?;
    let model = article::Entity::find()
        .filter(article::Column::Id.eq(article_id))
        .filter(article::Column::OwnerId.eq(owner_id))
        .one(&db.read_connection()?)
        .await
        .map_err(Error::internal)?
        .ok_or_else(not_found)?;
    Ok(Json(model.into()))
}

#[cfg(feature = "signing")]
fn parse_share_subject(subject: &str) -> Result<(i64, i64)> {
    let (owner_id, article_id) = subject.split_once(':').ok_or(Error::Unauthorized)?;
    let owner_id = owner_id.parse::<i64>().map_err(|_| Error::Unauthorized)?;
    let article_id = article_id.parse::<i64>().map_err(|_| Error::Unauthorized)?;
    if owner_id <= 0 || article_id <= 0 {
        return Err(Error::Unauthorized);
    }
    Ok((owner_id, article_id))
}
