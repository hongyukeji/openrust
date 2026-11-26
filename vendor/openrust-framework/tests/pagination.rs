use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use futures_util::TryStreamExt as _;
use openrust::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _,
        http::Kernel as _, view::View as _,
    },
    database::{
        Connection, DatabaseFuture, DatabaseManager, DatabaseResult, Db, Row, Transaction, Value,
        eloquent::{
            Attributes, Collection, Model, ModelMetadata, ModelState,
            casts::AttributeCast,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            relations::{
                BelongsTo, BelongsToMany, HasMany, HasManyThrough, HasOne, MorphMany, MorphOne,
                MorphTo, MorphToMany, Pivot,
            },
        },
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    pagination::{
        Cursor, CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, PaginationError,
        Paginator, PaginatorOptions, UrlWindow,
    },
    routing::RoutingError,
    support::facades::{Lang, Route},
    support::{ProviderResult, ServiceProvider},
    view::{ViewData, ViewError, ViewResult},
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::timeout,
};

struct Article(ModelState);

impl Model for Article {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("pagination_articles").timestamps(None, None);
        &META
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
}

struct IndexedArticle(ModelState);

impl Model for IndexedArticle {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("pagination_articles")
            .timestamps(None, None)
            .per_page(2);
        &META
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

    fn new_collection(models: Vec<Self>) -> Collection<Self> {
        Collection::from_entries(
            models
                .into_iter()
                .map(|model| (id(model.get_key()) * 10, model)),
        )
    }
}

struct Comment(ModelState);

impl Model for Comment {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("pagination_comments")
            .timestamps(None, None)
            .soft_deletes("deleted_at");
        &META
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
}

struct StreamedComment(ModelState);

impl Model for StreamedComment {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("pagination_comments")
            .timestamps(None, None)
            .soft_deletes("deleted_at");
        &META
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
}

struct RejectThirdComment;

impl ModelEventListener<StreamedComment> for RejectThirdComment {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut StreamedComment,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if id(model.get_key()) == 13 {
                return Err(openrust::database::DatabaseError::InvalidQuery(
                    "cursor observer",
                ));
            }
            model.set_attribute("retrieved", Value::Bool(true));
            Ok(ModelEventResponse::Continue)
        })
    }
}

struct Reaction(ModelState);

impl Model for Reaction {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("pagination_reactions").timestamps(None, None);
        &META
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
}

struct Activity(ModelState);

impl Model for Activity {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("pagination_activities").timestamps(None, None);
        &META
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
}

struct Label(ModelState);

impl Model for Label {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("pagination_labels").timestamps(None, None);
        &META
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
}

struct KeyedLabel(ModelState);

impl Model for KeyedLabel {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("pagination_labels").timestamps(None, None);
        &META
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
    fn new_collection(models: Vec<Self>) -> Collection<Self> {
        Collection::from_entries(models.into_iter().map(|label| {
            let prefix = if label.get_relation::<Pivot>("pivot").is_some() {
                "pivot"
            } else {
                "label"
            };
            (format!("{prefix}-{}", id(label.get_key())), label)
        }))
    }
}

struct PageMembership(ModelState);

impl Model for PageMembership {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("pagination_memberships").timestamps(None, None);
        &META
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
}

const REACTIONS: HasManyThrough<Article, Comment, Reaction> =
    HasManyThrough::new("article_id", "comment_id", "id", "id");

fn ids(rows: &[Row]) -> Vec<i64> {
    rows.iter().map(|row| id(row.get("id"))).collect()
}

fn id(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Integer(id)) => *id,
        Some(Value::Unsigned(id)) => i64::try_from(*id).unwrap(),
        _ => panic!("missing integer id"),
    }
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/length", length_page)?;
    Route::get("/multiple", multiple_pages)?;
    Route::get("/simple", simple_page)?;
    Route::get("/cursor", cursor_page)?;
    Route::get("/model-cursor", model_cursor_page)?;
    Route::get("/relationship-cursor", relationship_cursor_page)?;
    Route::get("/relationship-chunk", relationship_chunk_page)?;
    Route::get("/inverse-chunk", inverse_chunk_page)?;
    Route::get("/chunk-by-id", chunk_by_id_page)?;
    Route::get("/each-batch", each_batch_page)?;
    Route::get("/lazy-batches", lazy_batch_page)?;
    Route::get("/singular-pages", singular_relation_pages)?;
    Route::get("/page-defaults", page_defaults)?;
    Route::get("/page-resolvers", page_resolvers)?;
    Route::get("/state-resolvers", state_resolvers)?;
    Route::get("/resolver-view", resolver_view)?;
    Route::get("/resolver-input", resolver_input)?;
    Route::get("/resolver-query", resolver_query)?;
    Route::get("/links", pagination_links)?;
    Route::get("/manual", manual_length)?;
    Route::get("/manual-simple", manual_simple)?;
    Route::get("/manual-cursor", manual_cursor)?;
    Route::get("/related-length", related_length_page)?;
    Route::get("/related-cursor", related_cursor_page)?;
    Route::get("/through", through_page)?;
    Route::get("/pivot-length", pivot_length_page)?;
    Route::get("/pivot-cursor", pivot_cursor_page)?;
    Route::get("/morph", morph_page)?;
    Route::get("/keyed", keyed_page)?;
    Route::get("/keyed-pivot", keyed_pivot_page)?;
    Route::get("/relation-find", find_related_or_new)?;
    Route::get("/relation-lookup", relation_lookup)?;
    Route::get("/relation-many", find_related_many)?;
    Route::get("/relation-strict", find_strict_related_many)?;
    Ok(())
}

fn length_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut page = Article::query()?
            .paginate_with(PageOptions {
                per_page: Some(2),
                page_name: "articles",
                ..Default::default()
            })
            .await?;
        page.with_query_string().fragment(Some("results here"));
        Response::json(StatusCode::OK, page.map(|article| id(article.get_key())))
    })
}

fn multiple_pages(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut articles = Article::query()?
            .paginate_with(PageOptions {
                per_page: Some(2),
                page_name: "articles",
                ..Default::default()
            })
            .await?;
        let mut comments = Comment::query()?
            .simple_paginate_with(PageOptions {
                per_page: Some(2),
                page_name: "comments",
                ..Default::default()
            })
            .await?;
        articles.with_query_string().fragment(Some("article list"));
        comments.with_query_string().fragment(Some("comment list"));
        Response::json(
            StatusCode::OK,
            json!({
                "articles": articles.map(|article| id(article.get_key())),
                "comments": comments.map(|comment| id(comment.get_key())),
            }),
        )
    })
}

fn simple_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let page = Db::table("pagination_articles")?
            .order_by("id", "asc")
            .simple_paginate(2)
            .await?;
        Response::json(StatusCode::OK, page.map(|row| id(row.get("id"))))
    })
}

fn cursor_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut page = Article::query()?
            .order_by("rank_number", "asc")
            .order_by("id", "desc")
            .cursor_paginate(2)
            .await?;
        page.with_query_string();
        Response::json(StatusCode::OK, page.map(|article| id(article.get_key())))
    })
}

fn related_length_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()?.find(1).await?.unwrap();
        let mut page = HasMany::<Article, Comment>::new("id", "article_id")
            .constrain(|mut query| {
                query.r#where("visible", "=", 1).or_where("id", "=", 12);
                query
            })
            .paginate_with(
                &parent,
                PageOptions {
                    per_page: Some(2),
                    page_name: "comments",
                    ..Default::default()
                },
            )
            .await?;
        page.with_query_string();
        Response::json(StatusCode::OK, page.map(|comment| id(comment.get_key())))
    })
}

fn related_cursor_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()?.find(1).await?.unwrap();
        let mut page =
            MorphMany::<Article, Comment>::new("id", "article_id", "subject_type", "article")
                .cursor_paginate_with(
                    &parent,
                    CursorOptions {
                        per_page: Some(2),
                        cursor_name: "comments",
                        ..Default::default()
                    },
                )
                .await?;
        page.with_query_string();
        Response::json(StatusCode::OK, page.map(|comment| id(comment.get_key())))
    })
}

fn through_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()?.find(1).await?.unwrap();
        let mut page = REACTIONS
            .paginate_with(
                &parent,
                PageOptions {
                    per_page: Some(2),
                    page_name: "reactions",
                    columns: &["id"],
                    ..Default::default()
                },
            )
            .await?;
        page.with_query_string();
        Response::json(
            StatusCode::OK,
            page.map(|reaction| {
                assert!(reaction.get_attribute("comment_id").is_none());
                id(reaction.get_key())
            }),
        )
    })
}

fn labels() -> BelongsToMany<Article, Label> {
    BelongsToMany::new("pagination_memberships", "owner_id", "label_id", "id", "id")
        .where_pivot("subject_type", "=", "article")
        .with_pivot(["label"])
        .order_by_pivot("label_id", "asc")
}

fn pivot_length_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()?.find(1).await?.unwrap();
        let mut page = labels()
            .paginate_with(
                &parent,
                PageOptions {
                    per_page: Some(2),
                    page_name: "labels",
                    ..Default::default()
                },
            )
            .await?;
        page.with_query_string();
        Response::json(
            StatusCode::OK,
            page.map(|label| {
                let pivot = label.get_relation::<Pivot>("pivot").unwrap();
                let Some(Value::Text(color)) = pivot.get_attribute("label") else {
                    panic!("missing label in pivot");
                };
                json!([id(label.get_key()), color])
            }),
        )
    })
}

fn pivot_cursor_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()?.find(1).await?.unwrap();
        let mut page = MorphToMany::<Article, Label>::new(
            "pagination_memberships",
            "owner_id",
            "label_id",
            "id",
            "id",
            "subject_type",
            "article",
        )
        .using::<PageMembership>()
        .r#as("membership")
        .with_pivot(["label"])
        .order_by_pivot("weight", "asc")
        .order_by_pivot("label_id", "asc")
        .cursor_paginate_with(
            &parent,
            CursorOptions {
                per_page: Some(2),
                cursor_name: "labels",
                columns: &["id"],
                ..Default::default()
            },
        )
        .await?;
        page.with_query_string();
        Response::json(
            StatusCode::OK,
            page.map(|label| {
                assert!(
                    label
                        .state()
                        .get_attributes()
                        .keys()
                        .all(|key| !key.starts_with("laravel_pivot_"))
                );
                assert!(
                    label
                        .state()
                        .get_original()
                        .keys()
                        .all(|key| !key.starts_with("laravel_pivot_"))
                );
                let pivot = label.get_relation::<PageMembership>("membership").unwrap();
                assert!(pivot.get_attribute("weight").is_none());
                assert_eq!(id(pivot.get_attribute("owner_id")), 1);
                assert_eq!(id(pivot.get_attribute("label_id")), id(label.get_key()));
                let Some(Value::Text(color)) = pivot.get_attribute("label") else {
                    panic!("missing label in custom pivot");
                };
                json!([id(label.get_key()), id(pivot.get_key()), color])
            }),
        )
    })
}

fn keyed_page(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.query(Some("mode"))?.unwrap_or_default();
        let query = KeyedLabel::query()?.order_by("id", "asc");
        match mode.as_str() {
            Some("simple") => {
                let mut page = query.simple_paginate(2).await?;
                page.with_query_string();
                if let Some(label) = page.items().first() {
                    let key = format!("label-{}", id(label.get_key()));
                    assert!(page.get_by_key(key).is_some() && page.get_by_key(0_i64).is_none());
                }
                Response::json(StatusCode::OK, page.through(|label| id(label.get_key())))
            }
            Some("cursor") => {
                let mut page = query.cursor_paginate(2).await?;
                page.with_query_string();
                Response::json(StatusCode::OK, page.through(|label| id(label.get_key())))
            }
            _ => {
                let mut page = query.paginate(2).await?;
                page.with_query_string();
                if let Some(label) = page.items().first() {
                    let key = format!("label-{}", id(label.get_key()));
                    assert!(page.get_by_key(key).is_some() && page.get_by_key(0_i64).is_none());
                }
                Response::json(StatusCode::OK, page.through(|label| id(label.get_key())))
            }
        }
    })
}

fn keyed_pivot_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()?.find(1).await?.unwrap();
        let relation = BelongsToMany::<Article, KeyedLabel>::new(
            "pagination_memberships",
            "owner_id",
            "label_id",
            "id",
            "id",
        )
        .where_pivot("subject_type", "=", "article");
        let page = relation.paginate(&parent, 2).await?;
        assert!(page.get_by_key("pivot-1").is_some() && page.get_by_key("label-1").is_none());
        Response::json(StatusCode::OK, page.through(|label| id(label.get_key())))
    })
}

fn find_related_or_new(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()?.find(1).await?.unwrap();
        let children = HasMany::<Article, Comment>::new("id", "article_id").constrain(|query| {
            query.with_attributes(
                Attributes::from([("visible".into(), Value::Integer(1))]),
                true,
            )
        });
        let found = children.find_or_new(&parent, 11_i64).await?;
        let filtered = children.find_or_new(&parent, 12_i64).await?;
        for key in [14_i64, 16] {
            let draft = children.find_or_new(&parent, key).await?;
            assert!(!draft.state().exists() && draft.get_key().is_none());
            assert_eq!(id(draft.get_attribute("article_id")), 1);
        }
        assert!(found.state().exists());
        assert!(!filtered.state().exists() && filtered.get_key().is_none());

        let one = HasOne::<Article, Comment>::new("id", "article_id");
        let direct = one.find_or_new(&parent, 13_i64).await?;
        let morph_many =
            MorphMany::<Article, Comment>::new("id", "article_id", "subject_type", "article");
        let morph_one =
            MorphOne::<Article, Comment>::new("id", "article_id", "subject_type", "article");
        let draft = morph_one.find_or_new(&parent, 16_i64).await?;
        assert_eq!(
            draft.get_attribute("subject_type"),
            Some(&Value::Text("article".into()))
        );
        assert!(!draft.state().exists() && draft.get_key().is_none());
        let polymorphic = morph_many.find_or_new(&parent, 15_i64).await?;

        let through = REACTIONS.find_or_new(&parent, 103_i64).await?;
        let draft = REACTIONS.one().find_or_new(&parent, 104_i64).await?;
        assert!(!draft.state().exists() && draft.get_key().is_none());
        assert!(draft.get_attribute("comment_id").is_none());

        let attached = labels().find_or_new(&parent, 2_i64).await?;
        let pivot = attached.get_relation::<Pivot>("pivot").unwrap();
        let morph_to_many = MorphToMany::<Article, Label>::new(
            "pagination_memberships",
            "owner_id",
            "label_id",
            "id",
            "id",
            "subject_type",
            "article",
        );
        let draft = morph_to_many.find_or_new(&parent, 5_i64).await?;
        assert!(!draft.state().exists() && draft.get_key().is_none());
        assert!(draft.get_relation::<Pivot>("pivot").is_none());

        Response::json(
            StatusCode::OK,
            json!({
                "found": id(found.get_key()),
                "draft_owner": id(filtered.get_attribute("article_id")),
                "draft_default": id(filtered.get_attribute("visible")),
                "direct": id(direct.get_key()),
                "polymorphic": id(polymorphic.get_key()),
                "through": id(through.get_key()),
                "attached": id(attached.get_key()),
                "pivot_owner": id(pivot.get_attribute("owner_id")),
            }),
        )
    })
}

fn relation_lookup(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.query(Some("mode"))?.unwrap_or_default();
        let parent = Article::query()?.find(1).await?.unwrap();
        let children = HasMany::<Article, Comment>::new("id", "article_id");
        let labels = labels();
        match mode.as_str() {
            Some("missing") => {
                return match labels.find_sole(&parent, 4_i64).await {
                    Err(openrust::database::DatabaseError::ModelNotFound("pagination_labels")) => {
                        Err(HttpError::NotFound)
                    }
                    outcome => panic!("expected not found, found a model: {}", outcome.is_ok()),
                };
            }
            Some("duplicate") => {
                return match labels.find_sole(&parent, 2_i64).await {
                    Err(openrust::database::DatabaseError::MultipleModelsFound {
                        table: "pagination_labels",
                        count: 2,
                    }) => Response::json(StatusCode::CONFLICT, json!({"matches":2})),
                    outcome => panic!(
                        "expected duplicate rows, found a model: {}",
                        outcome.is_ok()
                    ),
                };
            }
            _ => {}
        }

        let hit = children
            .find_or(&parent, 11_i64, || async {
                panic!("matching row must suppress fallback");
            })
            .await?;
        let draft = children
            .find_or(&parent, 14_i64, || async {
                children.make(&parent, Attributes::new()).await
            })
            .await?;
        assert!(!draft.state().exists());
        assert_eq!(id(draft.get_attribute("article_id")), 1);
        assert!(children.find(&parent, 14_i64).await?.is_none());
        assert!(matches!(
            children.find_or_fail(&parent, 14_i64).await,
            Err(openrust::database::DatabaseError::ModelNotFound(_))
        ));
        assert!(matches!(
            children.sole(&parent).await,
            Err(openrust::database::DatabaseError::MultipleModelsFound { count: 2, .. })
        ));
        let first = children
            .first_or(&parent, || async {
                panic!("nonempty relation must suppress fallback");
            })
            .await?;
        assert_eq!(id(first.get_attribute("article_id")), 1);
        let broken = HasMany::<Article, Comment>::new("id", "article_id").constrain(|mut query| {
            query.r#where("pagination_comments.does_not_exist", "=", 1);
            query
        });
        assert!(
            broken
                .find_or(&parent, 11_i64, || async {
                    panic!("query failure must suppress fallback");
                })
                .await
                .is_err()
        );
        let through = REACTIONS.find_sole(&parent, 103_i64).await?;
        let attached = labels.find_sole(&parent, 2_i64).await?;
        assert!(attached.get_relation::<Pivot>("pivot").is_some());
        let empty = children.constrain(|mut query| {
            query.r#where("id", "<", 0);
            query
        });
        let no_rows = empty
            .first_or(&parent, || async {
                Err(openrust::database::DatabaseError::InvalidQuery(
                    "fallback error",
                ))
            })
            .await;
        assert!(matches!(
            no_rows,
            Err(openrust::database::DatabaseError::InvalidQuery(
                "fallback error"
            ))
        ));
        Response::json(
            StatusCode::OK,
            json!({
                "hit": id(hit.get_key()),
                "through": id(through.get_key()),
                "attached": id(attached.get_key()),
            }),
        )
    })
}

fn find_related_many(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()?.find(1).await?.unwrap();
        let children =
            HasMany::<Article, Comment>::new("id", "article_id").constrain(|mut query| {
                query.r#where("id", ">", 0).or_where("article_id", "=", 2);
                query.order_by("id", "desc")
            });
        let keys = [15_i64, 11, 11, 14, 16, 999].map(Value::Integer);
        let direct = children.find_many(&parent, &keys).await?;
        let invalid =
            HasMany::<Article, Comment>::new("id", "article_id").constrain(|mut query| {
                query.r#where("pagination_comments.does_not_exist", "=", 1);
                query
            });
        assert!(invalid.find_many(&parent, &[]).await?.is_empty());
        assert!(invalid.find_many(&parent, &keys).await.is_err());
        let unsaved = Article::new_instance(Attributes::new())?;
        assert!(children.find_many(&unsaved, &keys).await?.is_empty());

        let morph =
            MorphMany::<Article, Comment>::new("id", "article_id", "subject_type", "article")
                .constrain(|query| query.order_by("id", "asc"));
        let polymorphic = morph.find_many(&parent, &keys).await?;

        let through_keys = [103_i64, 102, 102, 104, 105].map(Value::Integer);
        let related = REACTIONS
            .constrain(|query| query.order_by("id", "asc"))
            .find_many(&parent, &through_keys)
            .await?;

        let pivot_keys = [3_i64, 1, 1, 4, 5].map(Value::Integer);
        let labels = BelongsToMany::<Article, KeyedLabel>::new(
            "pagination_memberships",
            "owner_id",
            "label_id",
            "id",
            "id",
        )
        .where_pivot("subject_type", "=", "article")
        .order_by_pivot("label_id", "asc")
        .with_pivot(["label"]);
        let attached = labels.find_many(&parent, &pivot_keys).await?;
        let red = attached
            .get_by_key("pivot-1")
            .unwrap()
            .get_relation::<Pivot>("pivot")
            .unwrap();
        assert_eq!(red.get_attribute("label"), Some(&Value::Text("red".into())));
        assert!(attached.get_by_key("pivot-3").is_some());
        let morph_labels = MorphToMany::<Article, Label>::new(
            "pagination_memberships",
            "owner_id",
            "label_id",
            "id",
            "id",
            "subject_type",
            "article",
        )
        .order_by_pivot("label_id", "asc");
        let morph_attached = morph_labels.find_many(&parent, &pivot_keys).await?;
        Response::json(
            StatusCode::OK,
            json!({
                "direct": direct.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
                "polymorphic": polymorphic.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
                "through": related.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
                "pivot": attached.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
                "morph_pivot": morph_attached.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
            }),
        )
    })
}

fn find_strict_related_many(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.query(Some("mode"))?.unwrap_or_default();
        let parent = Article::query()?.find(1).await?.unwrap();
        let labels = labels();
        let missing_keys = [2_i64, 4].map(Value::Integer);
        if mode.as_str() == Some("missing") {
            return match labels.find_many_or_fail(&parent, &missing_keys).await {
                Err(openrust::database::DatabaseError::ModelNotFound("pagination_labels")) => {
                    Err(HttpError::NotFound)
                }
                outcome => panic!("expected missing primary key: {}", outcome.is_ok()),
            };
        }
        let found = labels
            .find_many_or(&parent, &[Value::Integer(2), Value::Integer(2)], || async {
                panic!("matched primary keys must suppress fallback");
            })
            .await?;
        let pivot_labels = found
            .iter()
            .map(|model| {
                let pivot = model
                    .get_relation::<Pivot>("pivot")
                    .expect("pivot must be loaded");
                let Some(Value::Text(label)) = pivot.get_attribute("label") else {
                    panic!("pivot label must be loaded");
                };
                label
            })
            .collect::<Vec<_>>();
        let fallback = labels
            .find_many_or(&parent, &missing_keys, || async {
                labels.find_many(&parent, &[Value::Integer(3)]).await
            })
            .await?;

        let children = HasMany::<Article, Comment>::new("id", "article_id")
            .constrain(|query| query.order_by("id", "asc"));
        let direct = children
            .find_many_or_fail(
                &parent,
                &[
                    Value::Integer(11),
                    Value::Unsigned(11),
                    Value::Text("15".into()),
                ],
            )
            .await?;
        let bad_query =
            HasMany::<Article, Comment>::new("id", "article_id").constrain(|mut query| {
                query.r#where("pagination_comments.does_not_exist", "=", 1);
                query
            });
        assert!(
            bad_query
                .find_many_or(&parent, &[], || async {
                    panic!("an empty key list must suppress fallback");
                })
                .await?
                .is_empty()
        );
        assert!(
            bad_query
                .find_many_or(&parent, &[Value::Integer(11)], || async {
                    panic!("a query error must suppress fallback");
                })
                .await
                .is_err()
        );
        assert!(matches!(
            children.find_many_or_fail(&parent, &[Value::Null]).await,
            Err(openrust::database::DatabaseError::ModelNotFound(
                "pagination_comments"
            ))
        ));

        Response::json(
            StatusCode::OK,
            json!({
                "pivot_labels": pivot_labels,
                "fallback": fallback.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
                "direct": direct.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
            }),
        )
    })
}

fn morph_page(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.query(Some("mode"))?.unwrap_or_default();
        let query = Activity::query()?.order_by("id", "asc");
        let subject = MorphTo::<Activity>::new("subject_type", "subject_id")
            .target::<Article>("article", "id")
            .target::<Label>("label", "id");
        let comments = HasMany::<Article, Comment>::new("id", "article_id");
        let memberships = HasMany::<Label, PageMembership>::new("id", "label_id");
        macro_rules! respond {
            ($page:expr) => {{
                let mut page = $page.await?;
                let identity = std::ptr::from_ref(&page).addr();
                assert!(matches!(
                    page.load_morph("subject", &subject, "label", "comments", &comments)
                        .await,
                    Err(openrust::database::DatabaseError::InvalidQuery(_))
                ));
                let loaded = page
                    .load_morph("subject", &subject, "article", "comments", &comments)
                    .await?
                    .load_morph_count("subject", &subject, "article", "comments", &comments)
                    .await?
                    .load_morph_count("subject", &subject, "label", "memberships", &memberships)
                    .await?
                    .with_query_string();
                assert_eq!(std::ptr::from_ref(loaded).addr(), identity);
                Response::json(
                    StatusCode::OK,
                    page.map(|item| item.to_array().expect("activity serializes")),
                )
            }};
        }
        match mode.as_str() {
            Some("simple") => respond!(query.simple_paginate(2)),
            Some("cursor") => respond!(query.cursor_paginate(2)),
            _ => respond!(query.paginate(2)),
        }
    })
}

fn pagination_links(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.query(Some("mode"))?.unwrap_or_default();
        let locale = request.query(Some("locale"))?.unwrap_or_default();
        let locale = locale.as_str().unwrap_or("en");
        Lang::with_locale(locale, async {
            let query = Article::query()?.order_by("id", "asc");
            let mut view = match mode.as_str() {
                Some("json") => {
                    let page = query.paginate(2).await?;
                    return Response::json(StatusCode::OK, page.map(|_| ()));
                }
                Some("window") => {
                    let mut page = query.paginate(1).await?;
                    page.on_each_side(0);
                    page.links()
                }
                Some("simple") => {
                    let mut page = query.simple_paginate(2).await?;
                    page.with_query_string();
                    page.links()
                }
                Some("cursor") => {
                    let mut page = query.cursor_paginate(2).await?;
                    page.with_query_string();
                    page.links()
                }
                Some("semantic") => query
                    .paginate(2)
                    .await?
                    .links_with("pagination::semantic-ui", ViewData::new()),
                Some("custom") => {
                    let page = query.paginate(2).await?;
                    page.links_with(
                        "pagination::custom",
                        ViewData::from_iter([
                            ("title".into(), json!("<Custom>").into()),
                            ("paginator".into(), Json::Null.into()),
                        ]),
                    )
                }
                Some("single") => query.paginate(20).await?.links(),
                _ => {
                    let mut page = query.paginate(2).await?;
                    page.with_query_string().fragment(Some("results"));
                    page.links()
                }
            }?;
            let html = view.render()?;
            let mut response = Response::new(StatusCode::OK, html.into());
            response
                .parts_mut()
                .headers
                .insert("content-type", "text/html; charset=utf-8".parse().unwrap());
            Ok(response)
        })
        .await
        .map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?
    })
}

fn manual_length(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let current = request
            .query(Some("entries"))?
            .and_then(|v| v.as_str().and_then(|v| v.parse::<usize>().ok()))
            .unwrap_or(1)
            .max(1);
        let items = (1_i64..=5)
            .skip((current - 1).saturating_mul(2))
            .take(2)
            .collect();
        let mut page = LengthAwarePaginator::new(
            items,
            5,
            2,
            None,
            PaginatorOptions {
                page_name: Some("entries"),
                ..Default::default()
            },
        )
        .map_err(manual_error)?;
        assert_eq!(page.current_page(), current as u64);
        page.with_query_string()
            .with_path("/api/manual?fixed=1")
            .fragment(Some("results"));
        assert_eq!(page.get_options().path, Some("/api/manual?fixed=1"));
        assert_eq!(page.get_options().fragment, Some("results"));
        assert_eq!(page.get_page_name(), "entries");
        let urls: Vec<_> = page.get_url_range(1, 2).collect();
        Response::json(
            StatusCode::OK,
            json!({
                "count": page.count(),
                "first": page.on_first_page(),
                "last": page.on_last_page(),
                "range": urls,
                "page": page.through(|item| item.to_string()),
            }),
        )
    })
}

fn manual_simple(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut page = Paginator::new(vec![1, 2, 3, 4, 5], 2, Some(1), PaginatorOptions::default())
            .map_err(manual_error)?;
        page.set_page_name("after");
        assert_eq!(page.get_options().page_name, Some("after"));
        Response::json(StatusCode::OK, page.through(|item| item * 10))
    })
}

fn manual_cursor(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = request.query(Some("position"))?;
        let cursor = input
            .as_ref()
            .and_then(|v| v.as_str())
            .and_then(|v| Cursor::from_encoded(v).ok());
        let anchor = cursor
            .as_ref()
            .map(|cursor| cursor.parameter("id"))
            .transpose()
            .map_err(manual_error)?;
        let ordered: Vec<i64> = match anchor {
            Some(Value::Integer(anchor))
                if cursor
                    .as_ref()
                    .is_some_and(Cursor::points_to_previous_items) =>
            {
                (1..=5).rev().filter(|i| *i < anchor).take(3).collect()
            }
            Some(Value::Integer(anchor)) => (1..=5).filter(|i| *i > anchor).take(3).collect(),
            _ => vec![1, 2, 3],
        };
        let mut page = CursorPaginator::new(
            ordered,
            2,
            cursor.as_ref(),
            PaginatorOptions {
                cursor_name: Some("position"),
                ..Default::default()
            },
            |item| [("id", Value::Integer(*item))],
        )
        .map_err(manual_error)?;
        page.with_query_string();
        assert_eq!(page.get_cursor_name(), "position");
        assert_eq!(page.get_options().cursor_name, Some("position"));
        page.set_cursor_name("position");
        Response::json(
            StatusCode::OK,
            json!({
                "count": page.count(),
                "first": page.on_first_page(),
                "last": page.on_last_page(),
                "page": page.through(|item| item.to_string()),
            }),
        )
    })
}

fn manual_error(source: PaginationError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}

fn model_cursor_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut rows = StreamedComment::query()?
            .with_casts([("id", AttributeCast::String)])
            .order_by("pagination_comments.id", "desc")
            .with(
                "article",
                openrust::database::eloquent::relations::BelongsTo::<StreamedComment, Article>::new(
                    "article_id",
                    "id",
                ),
            )
            .cursor();
        let first = rows.try_next().await?.unwrap();
        assert!(!first.relation_loaded("article"));
        assert_eq!(first.get_attribute("retrieved"), Some(&Value::Bool(true)));
        drop(rows);

        let mut rows = StreamedComment::query()?.order_by("id", "asc").cursor();
        for expected in [11, 12] {
            assert_eq!(id(rows.try_next().await?.unwrap().get_key()), expected);
        }
        assert!(rows.try_next().await.is_err());
        assert!(rows.try_next().await?.is_none());
        Response::json(StatusCode::OK, first.to_array()?)
    })
}

fn relationship_cursor_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        async fn stream_relations() -> DatabaseResult<Json> {
            let parent = Article::query()?.find_or_fail(1).await?;
            let direct = HasMany::<Article, StreamedComment>::new("id", "article_id")
                .constrain(|query| query.order_by("id", "asc"));
            let mut rows = direct.cursor(&parent);
            assert_eq!(id(rows.try_next().await?.unwrap().get_key()), 11);
            drop(rows);

            let morph =
                MorphMany::<Article, Comment>::new("id", "article_id", "subject_type", "article")
                    .constrain(|query| query.order_by("id", "asc"));
            let comments: Vec<_> = morph.cursor(&parent).try_collect().await?;
            let owner = BelongsTo::<Comment, Article>::new("article_id", "id");
            let owners: Vec<_> = owner.cursor(&comments[0]).try_collect().await?;

            let subject = MorphTo::<Activity>::new("subject_type", "subject_id")
                .target::<Article>("article", "id")
                .target::<Label>("label", "id")
                .constrain::<Comment>("comment", "id", |mut query| {
                    query.r#where("visible", "=", 0);
                    query
                })
                .with_default::<Label>("label", |_, _| {})?
                .default_for_missing_type("label")?;
            let activities = Activity::query()?.order_by("id", "asc").get().await?;
            let mut rows = subject.cursor(&activities[0]);
            let article = rows.try_next().await?.unwrap();
            assert_eq!(article.alias(), "article");
            assert_eq!(id(article.downcast_ref::<Article>().unwrap().get_key()), 1);
            drop(rows);
            let mut rows = subject.cursor(&activities[1]);
            let label = rows.try_next().await?.unwrap();
            assert_eq!(label.alias(), "label");
            assert_eq!(id(label.downcast_ref::<Label>().unwrap().get_key()), 2);
            assert!(rows.try_next().await?.is_none());
            drop(rows);
            assert!(subject.cursor(&activities[4]).try_next().await?.is_none());

            let mut draft = Activity::new_instance(Attributes::new())?;
            draft
                .set_attribute("subject_type", "comment")
                .set_attribute("subject_id", 11);
            assert!(subject.cursor(&draft).try_next().await?.is_none());
            draft.set_attribute("subject_id", 16);
            assert!(subject.cursor(&draft).try_next().await?.is_none());
            let subject = subject.with_trashed();
            let target = subject.cursor(&draft).try_next().await?.unwrap();
            assert_eq!(id(target.downcast_ref::<Comment>().unwrap().get_key()), 16);
            draft.set_attribute("subject_type", "unregistered");
            assert!(matches!(
                subject.cursor(&draft).try_next().await,
                Err(openrust::database::DatabaseError::InvalidQuery(
                    "unknown morph type"
                ))
            ));

            let through =
                REACTIONS.constrain(|query| query.order_by("pagination_reactions.id", "asc"));
            let reactions: Vec<_> = through.cursor(&parent).try_collect().await?;

            let labels = MorphToMany::<Article, KeyedLabel>::new(
                "pagination_memberships",
                "owner_id",
                "label_id",
                "id",
                "id",
                "subject_type",
                "article",
            )
            .with_pivot(["label"])
            .order_by_pivot("label_id", "asc");
            let mut rows = labels.cursor(&parent);
            let mut pivot_values = Vec::new();
            while let Some(label) = rows.try_next().await? {
                let pivot = label.get_relation::<Pivot>("pivot").unwrap();
                let Some(Value::Text(value)) = pivot.get_attribute("label") else {
                    panic!("missing pivot value");
                };
                assert!(label.get_attribute("laravel_pivot_0").is_none());
                assert!(label.get_raw_original("laravel_pivot_0").is_none());
                pivot_values.push(value.clone());
            }
            let custom = BelongsToMany::<Article, Label>::new(
                "pagination_memberships",
                "owner_id",
                "label_id",
                "id",
                "id",
            )
            .using::<PageMembership>()
            .r#as("membership")
            .with_pivot(["label"]);
            let mut rows = custom.cursor(&parent);
            let label = rows.try_next().await?.unwrap();
            assert!(label.get_relation::<PageMembership>("membership").is_some());
            drop(rows);

            let one = REACTIONS.one().latest_of_many(None);
            let only: Vec<_> = one.cursor(&parent).try_collect().await?;
            assert_eq!(only.len(), 1);
            assert_eq!(id(only[0].get_key()), 103);

            let unsaved = Article::new_instance(Attributes::new())?;
            assert!(direct.cursor(&unsaved).try_next().await?.is_none());
            Ok(json!({
                "comments": comments.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
                "owner": id(owners[0].get_key()),
                "through": reactions.iter().map(|model| id(model.get_key())).collect::<Vec<_>>(),
                "pivot": pivot_values,
            }))
        }
        Response::json(StatusCode::OK, stream_relations().await?)
    })
}

fn relationship_chunk_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::database::DatabaseError;
        use std::future::ready;

        async fn batches() -> DatabaseResult<Json> {
            let parent = Article::query()?.find_or_fail(1).await?;
            let comments =
                MorphMany::<Article, Comment>::new("id", "article_id", "subject_type", "article")
                    .constrain(|query| {
                        query.order_by("id", "desc").offset(1).limit(3).with(
                            "article",
                            BelongsTo::<Comment, Article>::new("article_id", "id"),
                        )
                    });
            let mut chunks = Vec::new();
            assert!(
                comments
                    .chunk(&parent, 2, |batch, page| {
                        assert!(batch.iter().all(|model| {
                            model
                                .get_relation::<Option<Arc<Article>>>("article")
                                .and_then(Option::as_ref)
                                .is_some_and(|owner| id(owner.get_key()) == 1)
                        }));
                        chunks.push(json!([
                            page,
                            batch
                                .iter()
                                .map(|model| id(model.get_key()))
                                .collect::<Vec<_>>()
                        ]));
                        ready(Ok(true))
                    })
                    .await?
            );

            let direct = HasMany::<Article, StreamedComment>::new("id", "article_id");
            assert!(
                !direct
                    .chunk(&parent, 2, |batch, page| {
                        assert_eq!(page, 1);
                        assert_eq!(
                            batch
                                .iter()
                                .map(|model| id(model.get_key()))
                                .collect::<Vec<_>>(),
                            [11, 12]
                        );
                        ready(Ok(false))
                    })
                    .await?
            );
            assert!(matches!(
                direct.chunk(&parent, 0, |_, _| unreachable_future()).await,
                Err(DatabaseError::InvalidQuery("chunk size must be positive"))
            ));
            assert!(matches!(
                direct
                    .chunk(&parent, 1, |_, _| ready(Err(DatabaseError::InvalidQuery(
                        "callback failed"
                    ))))
                    .await,
                Err(DatabaseError::InvalidQuery("callback failed"))
            ));

            let mut reactions = Vec::new();
            REACTIONS
                .chunk(&parent, 2, |models, page| {
                    reactions.push(json!([
                        page,
                        models
                            .iter()
                            .map(|model| id(model.get_key()))
                            .collect::<Vec<_>>()
                    ]));
                    ready(Ok(true))
                })
                .await?;

            let labels = BelongsToMany::<Article, KeyedLabel>::new(
                "pagination_memberships",
                "owner_id",
                "label_id",
                "id",
                "id",
            )
            .where_pivot("subject_type", "=", "article")
            .with_pivot(["label"])
            .order_by_pivot("label_id", "asc");
            let mut pivots = Vec::new();
            labels
                .chunk(&parent, 2, |models, page| {
                    let color: Vec<_> = models
                        .iter()
                        .map(|model| {
                            assert!(model.get_attribute("laravel_pivot_0").is_none());
                            let key = format!("pivot-{}", id(model.get_key()));
                            assert!(models.get_by_key(key).is_some());
                            let pivot = model.get_relation::<Pivot>("pivot").unwrap();
                            let Some(Value::Text(color)) = pivot.get_attribute("label") else {
                                panic!("missing pivot label");
                            };
                            color.clone()
                        })
                        .collect();
                    pivots.push(json!([page, color]));
                    ready(Ok(true))
                })
                .await?;

            let connection = Db::connection(None)?;
            let mut raw = Vec::new();
            connection
                .table("pagination_articles")
                .order_by("id", "asc")
                .offset(2)
                .limit(3)
                .chunk(2, |rows, page| {
                    raw.push(json!([
                        page,
                        rows.iter().map(|row| id(row.get("id"))).collect::<Vec<_>>()
                    ]));
                    ready(Ok(true))
                })
                .await?;
            assert!(matches!(
                connection
                    .table("pagination_articles")
                    .chunk(2, |_, _| unreachable_future())
                    .await,
                Err(DatabaseError::InvalidQuery(
                    "chunk requires an order by clause"
                ))
            ));
            assert!(
                connection
                    .table("pagination_articles")
                    .order_by("id", "asc")
                    .limit(0)
                    .chunk(2, |_, _| unreachable_future())
                    .await?
            );
            Ok(json!({"comments": chunks, "reactions": reactions, "pivots": pivots, "raw": raw}))
        }

        fn unreachable_future() -> std::future::Ready<DatabaseResult<bool>> {
            panic!("empty batch invoked the callback")
        }

        Response::json(StatusCode::OK, batches().await?)
    })
}

fn inverse_chunk_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::database::DatabaseError;
        use std::future::ready;

        async fn verify_batches() -> DatabaseResult<Json> {
            let article_association = BelongsTo::<Activity, Article>::new("subject_id", "id")
                .with_default(|_, _| panic!("query should not create a default model"));
            let activities = Activity::query()?.order_by("id", "asc").get().await?;
            assert!(
                !article_association
                    .chunk(&activities[0], 1, |models, page| {
                        assert_eq!(page, 1);
                        assert_eq!(models.len(), 1);
                        assert_eq!(id(models[0].get_key()), 1);
                        ready(Ok(false))
                    })
                    .await?
            );
            assert!(
                article_association
                    .chunk(&activities[4], 1, |_, _| unexpected_batch())
                    .await?
            );
            assert!(matches!(
                article_association
                    .chunk(&activities[4], 0, |_, _| unexpected_batch())
                    .await,
                Err(DatabaseError::InvalidQuery("chunk size must be positive"))
            ));

            let subject = MorphTo::<Activity>::new("subject_type", "subject_id")
                .constrain::<Article>("article", "category", |query| {
                    query.order_by("id", "desc").with(
                        "comments",
                        HasMany::<Article, Comment>::new("id", "article_id"),
                    )
                })
                .target::<KeyedLabel>("label", "id")
                .constrain::<Comment>("comment", "id", |mut query| {
                    query.r#where("visible", "=", 0);
                    query
                })
                .with_default::<KeyedLabel>("label", |_, _| {
                    panic!("query should not create a default model")
                })?
                .default_for_missing_type("label")?;
            assert!(
                subject
                    .chunk(&activities[4], 1, |_, _| unexpected_batch())
                    .await?
            );
            let mut draft = Activity::new_instance(Attributes::new())?;
            draft
                .set_attribute("subject_type", "article")
                .set_attribute("subject_id", "a");
            let mut owners = Vec::new();
            assert!(
                subject
                    .chunk(&draft, 1, |models, page| {
                        assert_eq!(models.count(), 1);
                        let target = models.first().unwrap();
                        assert_eq!(target.alias(), "article");
                        let article = target.downcast_ref::<Article>().unwrap();
                        assert!(article.relation_loaded("comments"));
                        owners.push(json!([page, id(article.get_key())]));
                        ready(Ok(true))
                    })
                    .await?
            );

            assert!(
                !subject
                    .chunk(&draft, 1, |_, page| {
                        assert_eq!(page, 1);
                        ready(Ok(false))
                    })
                    .await?
            );
            assert!(matches!(
                subject
                    .chunk(&draft, 1, |_, page| {
                        assert_eq!(page, 1);
                        ready(Err(DatabaseError::InvalidQuery("inverse callback")))
                    })
                    .await,
                Err(DatabaseError::InvalidQuery("inverse callback"))
            ));

            subject
                .chunk(&activities[1], 10, |models, page| {
                    assert_eq!(page, 1);
                    let target = models.get("label-2").unwrap();
                    assert_eq!(target.alias(), "label");
                    assert_eq!(
                        id(target.downcast_ref::<KeyedLabel>().unwrap().get_key()),
                        2
                    );
                    ready(Ok(true))
                })
                .await?;
            draft
                .set_attribute("subject_type", "label")
                .set_attribute("subject_id", 999);
            assert!(subject.chunk(&draft, 1, |_, _| unexpected_batch()).await?);
            draft
                .set_attribute("subject_type", "comment")
                .set_attribute("subject_id", 11);
            assert!(subject.chunk(&draft, 1, |_, _| unexpected_batch()).await?);
            draft.set_attribute("subject_id", 16);
            assert!(subject.chunk(&draft, 1, |_, _| unexpected_batch()).await?);
            let subject = subject.with_trashed();
            subject
                .chunk(&draft, 1, |models, _| {
                    assert_eq!(
                        id(models
                            .first()
                            .unwrap()
                            .downcast_ref::<Comment>()
                            .unwrap()
                            .get_key()),
                        16
                    );
                    ready(Ok(true))
                })
                .await?;
            draft.set_attribute("subject_type", "unregistered");
            assert!(matches!(
                subject.chunk(&draft, 1, |_, _| unexpected_batch()).await,
                Err(DatabaseError::InvalidQuery("unknown morph type"))
            ));
            Ok(json!(owners))
        }

        fn unexpected_batch() -> std::future::Ready<DatabaseResult<bool>> {
            panic!("callback invoked for missing rows")
        }

        Response::json(StatusCode::OK, verify_batches().await?)
    })
}

fn chunk_by_id_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::database::DatabaseError;
        use std::future::ready;

        async fn verify_keyset() -> DatabaseResult<Json> {
            let db = Db::connection(None)?;
            let mut raw = Vec::new();
            db.table("pagination_articles")
                .select(["id as boundary"])
                .r#where("id", "<=", 5)
                .order_by("id", "asc")
                .offset(1)
                .limit(3)
                .chunk_by_id_desc(
                    2,
                    |rows, page| {
                        raw.push(json!([
                            page,
                            rows.iter()
                                .map(|row| id(row.get("boundary")))
                                .collect::<Vec<_>>()
                        ]));
                        ready(Ok(true))
                    },
                    None,
                    Some("boundary"),
                )
                .await?;

            let missing = || {
                db.table("pagination_articles")
                    .select(["category"])
                    .limit(1)
            };
            let mut callbacks = 0;
            assert!(matches!(
                missing()
                    .chunk_by_id(
                        10,
                        |rows, page| {
                            assert_eq!((rows.len(), page), (1, 1));
                            callbacks += 1;
                            ready(Ok(true))
                        },
                        None,
                        None
                    )
                    .await,
                Err(DatabaseError::InvalidQuery(
                    "chunkById key is missing from the result"
                ))
            ));
            assert_eq!(callbacks, 1);
            assert!(
                !missing()
                    .chunk_by_id(10, |_, _| ready(Ok(false)), None, None)
                    .await?
            );
            assert!(matches!(
                missing()
                    .chunk_by_id(
                        10,
                        |_, _| ready(Err(DatabaseError::InvalidQuery("callback failed"))),
                        None,
                        None
                    )
                    .await,
                Err(DatabaseError::InvalidQuery("callback failed"))
            ));

            let mut ids = Vec::new();
            Article::query()?
                .order_by("id", "desc")
                .limit(5)
                .chunk_by_id(
                    2,
                    |models, page| {
                        ids.push(json!([
                            page,
                            models
                                .iter()
                                .map(|model| id(model.get_key()))
                                .collect::<Vec<_>>()
                        ]));
                        ready(Ok(true))
                    },
                    None,
                    None,
                )
                .await?;

            let mut aliases = Vec::new();
            Article::query()?
                .select(["id as boundary"])
                .limit(5)
                .ordered_chunk_by_id(
                    2,
                    |models, _| {
                        aliases.extend(
                            models
                                .iter()
                                .map(|model| id(model.get_attribute("boundary"))),
                        );
                        ready(Ok(true))
                    },
                    Some("pagination_articles.id"),
                    Some("boundary"),
                    false,
                )
                .await?;
            assert_eq!(aliases, [1, 2, 3, 4, 5]);

            let owner = Article::query()?.find_or_fail(1).await?;
            let labels = BelongsToMany::<Article, KeyedLabel>::new(
                "pagination_memberships",
                "owner_id",
                "label_id",
                "id",
                "id",
            )
            .where_pivot("subject_type", "=", "article")
            .with_pivot(["label"]);
            let mut pivots = Vec::new();
            labels
                .chunk_by_id_desc(
                    &owner,
                    2,
                    |models, page| {
                        let entries: Vec<_> = models
                            .iter()
                            .map(|model| {
                                let key = id(model.get_key());
                                assert!(models.get_by_key(format!("pivot-{key}")).is_some());
                                let pivot = model.get_relation::<Pivot>("pivot").unwrap();
                                let Some(Value::Text(color)) = pivot.get_attribute("label") else {
                                    panic!("missing pivot label");
                                };
                                json!([key, color])
                            })
                            .collect();
                        pivots.push(json!([page, entries]));
                        ready(Ok(true))
                    },
                    None,
                    None,
                )
                .await?;

            let mut matches = Vec::new();
            db.table("pagination_articles")
                .r#where("id", "=", 1)
                .or_where("id", "=", 4)
                .chunk_by_id(
                    1,
                    |rows, _| {
                        matches.push(id(rows[0].get("id")));
                        ready(Ok(true))
                    },
                    None,
                    None,
                )
                .await?;
            assert_eq!(matches, [1, 4]);
            verify_relation_keys(&owner).await?;
            Ok(json!({ "raw": raw, "models": ids, "pivots": pivots }))
        }

        async fn verify_relation_keys(owner: &Article) -> DatabaseResult<()> {
            use openrust::database::DatabaseError;
            fn collect<M: Model>(
                keys: &mut Vec<i64>,
            ) -> impl FnMut(Collection<M>, u64) -> std::future::Ready<DatabaseResult<bool>> + '_
            {
                move |models, _| {
                    keys.extend(models.iter().map(|model| id(model.get_key())));
                    ready(Ok(true))
                }
            }

            let children = HasMany::<Article, Comment>::new("id", "article_id");
            let mut keys = Vec::new();
            children
                .chunk_by_id(owner, 2, collect(&mut keys), None, None)
                .await?;
            assert_eq!(keys, [11, 12, 13, 15]);
            let mut keys = Vec::new();
            children
                .one()
                .latest_of_many(None)
                .chunk_by_id(owner, 1, collect(&mut keys), None, None)
                .await?;
            assert_eq!(keys, [15]);

            let subjects =
                MorphMany::<Article, Comment>::new("id", "article_id", "subject_type", "article");
            let mut keys = Vec::new();
            subjects
                .chunk_by_id_desc(owner, 2, collect(&mut keys), None, None)
                .await?;
            assert_eq!(keys, [15, 13, 12, 11]);
            let mut keys = Vec::new();
            MorphOne::<Article, Comment>::new("id", "article_id", "subject_type", "article")
                .oldest_of_many(None)
                .chunk_by_id_desc(owner, 1, collect(&mut keys), None, None)
                .await?;
            assert_eq!(keys, [11]);

            let mut keys = Vec::new();
            REACTIONS
                .chunk_by_id_desc(
                    owner,
                    2,
                    collect(&mut keys),
                    Some("pagination_reactions.id"),
                    None,
                )
                .await?;
            assert_eq!(keys, [103, 102, 101]);
            let mut keys = Vec::new();
            REACTIONS
                .one()
                .latest_of_many(None)
                .chunk_by_id(owner, 1, collect(&mut keys), None, None)
                .await?;
            assert_eq!(keys, [103]);

            let activities = Activity::query()?.order_by("id", "asc").get().await?;
            let owner_relation = BelongsTo::<Activity, Article>::new("subject_id", "id");
            let mut keys = Vec::new();
            owner_relation
                .chunk_by_id_desc(&activities[0], 1, collect(&mut keys), None, None)
                .await?;
            assert_eq!(keys, [1]);
            assert!(
                owner_relation
                    .chunk_by_id(
                        &activities[4],
                        1,
                        |_, _| -> std::future::Ready<DatabaseResult<bool>> {
                            panic!("missing reference produced a batch");
                        },
                        None,
                        None
                    )
                    .await?
            );

            let reference = MorphTo::<Activity>::new("subject_type", "subject_id")
                .target::<Article>("article", "category")
                .target::<KeyedLabel>("label", "id")
                .target::<Comment>("comment", "id");
            let mut draft = Activity::new_instance(Attributes::new())?;
            draft
                .set_attribute("subject_type", "article")
                .set_attribute("subject_id", "a");
            let mut keys = Vec::new();
            reference
                .chunk_by_id_desc(
                    &draft,
                    1,
                    |models, page| {
                        let resolved = models.first().unwrap().downcast_ref::<Article>().unwrap();
                        keys.push((page, id(resolved.get_key())));
                        ready(Ok(true))
                    },
                    None,
                    None,
                )
                .await?;
            assert_eq!(keys, [(1, 2), (2, 1)]);
            reference
                .chunk_by_id(
                    &activities[1],
                    1,
                    |models, _| {
                        assert_eq!(models.get("label-2").unwrap().alias(), "label");
                        ready(Ok(true))
                    },
                    None,
                    None,
                )
                .await?;
            draft
                .set_attribute("subject_type", "comment")
                .set_attribute("subject_id", 16);
            let mut batches = 0;
            reference
                .chunk_by_id(
                    &draft,
                    1,
                    |_, _| {
                        batches += 1;
                        ready(Ok(true))
                    },
                    None,
                    None,
                )
                .await?;
            assert_eq!(batches, 0);
            reference
                .with_trashed()
                .chunk_by_id(
                    &draft,
                    1,
                    |models, _| {
                        assert_eq!(
                            id(models
                                .first()
                                .unwrap()
                                .downcast_ref::<Comment>()
                                .unwrap()
                                .get_key()),
                            16
                        );
                        batches += 1;
                        ready(Ok(true))
                    },
                    None,
                    None,
                )
                .await?;
            assert_eq!(batches, 1);
            assert!(matches!(
                owner_relation
                    .chunk_by_id(&activities[4], 0, |_, _| ready(Ok(true)), None, None)
                    .await,
                Err(DatabaseError::InvalidQuery("chunk size must be positive"))
            ));
            Ok(())
        }

        Response::json(StatusCode::OK, verify_keyset().await?)
    })
}

fn each_batch_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::{database::DatabaseError, support::ArrayKey};
        use std::future::ready;

        async fn verify_each() -> DatabaseResult<Json> {
            let db = Db::connection(None)?;
            let mut indices = Vec::new();
            assert!(
                db.table("pagination_articles")
                    .order_by("id", "asc")
                    .limit(5)
                    .each(
                        |row, index| {
                            indices.push([id(row.get("id")), index]);
                            ready(Ok(true))
                        },
                        2
                    )
                    .await?
            );
            assert_eq!(indices, [[1, 0], [2, 1], [3, 0], [4, 1], [5, 0]]);

            let mut indices = Vec::new();
            db.table("pagination_articles")
                .select(["id as boundary"])
                .limit(5)
                .each_by_id(
                    |row, index| {
                        indices.push([id(row.get("boundary")), index]);
                        ready(Ok(true))
                    },
                    2,
                    None,
                    Some("boundary"),
                )
                .await?;
            assert_eq!(indices, [[1, 0], [2, 1], [3, 2], [4, 3], [5, 4]]);

            let mut custom = Vec::new();
            IndexedArticle::query()?
                .limit(5)
                .each_by_id(
                    |model, index| {
                        custom.push([id(model.get_key()), index]);
                        ready(Ok(true))
                    },
                    2,
                    None,
                    None,
                )
                .await?;
            assert_eq!(custom, [[1, 10], [2, 20], [3, 32], [4, 42], [5, 54]]);

            let mut stopped = Vec::new();
            assert!(
                !StreamedComment::query()?
                    .each(
                        |model, key| {
                            assert_eq!(key, ArrayKey::Integer(0));
                            stopped.push(id(model.get_key()));
                            ready(Ok(false))
                        },
                        2
                    )
                    .await?
            );
            assert_eq!(stopped, [11]);

            let owner = Article::query()?.find_or_fail(1).await?;
            let children =
                MorphMany::<Article, Comment>::new("id", "article_id", "subject_type", "article");
            let mut relations = Vec::new();
            children
                .each_by_id(
                    &owner,
                    |model, index| {
                        relations.push([id(model.get_key()), index]);
                        ready(Ok(true))
                    },
                    2,
                    None,
                    None,
                )
                .await?;
            assert_eq!(relations, [[11, 0], [12, 1], [13, 2], [15, 3]]);
            let labels = BelongsToMany::<Article, KeyedLabel>::new(
                "pagination_memberships",
                "owner_id",
                "label_id",
                "id",
                "id",
            )
            .where_pivot("subject_type", "=", "article")
            .with_pivot(["label"]);
            let mut pivots = Vec::new();
            labels
                .each(
                    &owner,
                    |model, key| {
                        assert!(model.get_relation::<Pivot>("pivot").is_some());
                        pivots.push(key);
                        ready(Ok(true))
                    },
                    2,
                )
                .await?;
            assert_eq!(
                pivots,
                ["pivot-1", "pivot-2", "pivot-3"].map(ArrayKey::from)
            );
            assert!(matches!(
                labels
                    .each_by_id(&owner, |_, _| ready(Ok(true)), 2, None, None)
                    .await,
                Err(DatabaseError::InvalidQuery(
                    "eachById requires integer collection keys"
                ))
            ));

            let activity = Activity::query()?.find_or_fail(2).await?;
            let target = MorphTo::<Activity>::new("subject_type", "subject_id")
                .target::<KeyedLabel>("label", "id");
            assert!(matches!(
                target
                    .each(
                        &activity,
                        |model, key| {
                            assert_eq!(key, ArrayKey::from("label-2"));
                            assert_eq!(model.alias(), "label");
                            ready(Err(DatabaseError::InvalidQuery("item callback failed")))
                        },
                        1
                    )
                    .await,
                Err(DatabaseError::InvalidQuery("item callback failed"))
            ));
            Ok(json!({ "indices": indices, "relations": relations }))
        }

        Response::json(StatusCode::OK, verify_each().await?)
    })
}

fn lazy_batch_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::database::DatabaseError;
        use std::future::ready;

        async fn verify_lazy() -> DatabaseResult<Json> {
            let db = Db::connection(None)?;
            drop(
                db.table("missing_batch_table")
                    .order_by("id", "asc")
                    .lazy(2),
            );
            let mut invalid = db.table("pagination_articles").lazy(2);
            assert!(matches!(
                invalid.try_next().await,
                Err(DatabaseError::InvalidQuery(
                    "chunk requires an order by clause"
                ))
            ));
            let mut invalid = db.table("pagination_articles").lazy_by_id(0, None, None);
            assert!(matches!(
                invalid.try_next().await,
                Err(DatabaseError::InvalidQuery("chunk size must be positive"))
            ));

            let rows: Vec<_> = db
                .table("pagination_articles")
                .r#where("id", "<=", 5)
                .select(["id as boundary"])
                .offset(1)
                .limit(3)
                .lazy_by_id_desc(2, None, Some("boundary"))
                .try_collect()
                .await?;
            let descending: Vec<_> = rows.iter().map(|row| id(row.get("boundary"))).collect();
            let missing = || db.table("pagination_articles").select(["category"]);
            let short: Vec<_> = missing()
                .limit(1)
                .lazy_by_id(2, None, None)
                .try_collect()
                .await?;
            assert_eq!(short.len(), 1);
            let mut full = missing().limit(2).lazy_by_id(2, None, None);
            assert!(full.try_next().await?.is_some());
            assert!(full.try_next().await?.is_some());
            assert!(matches!(
                full.try_next().await,
                Err(DatabaseError::InvalidQuery(
                    "lazyById key is missing from the result"
                ))
            ));

            let mut models = StreamedComment::query()?.lazy(2);
            assert_eq!(id(models.try_next().await?.unwrap().get_key()), 11);
            drop(models);
            let mut models = StreamedComment::query()?.lazy(2);
            assert_eq!(id(models.try_next().await?.unwrap().get_key()), 11);
            assert_eq!(id(models.try_next().await?.unwrap().get_key()), 12);
            assert!(matches!(
                models.try_next().await,
                Err(DatabaseError::InvalidQuery("cursor observer"))
            ));

            let owner = Article::query()?.find_or_fail(1).await?;
            let eager = Article::query()?
                .with(
                    "comments",
                    HasMany::<Article, Comment>::new("id", "article_id"),
                )
                .limit(1)
                .lazy_by_id(2, None, None)
                .try_next()
                .await?
                .unwrap();
            assert!(eager.relation_loaded("comments"));

            let labels = BelongsToMany::<Article, KeyedLabel>::new(
                "pagination_memberships",
                "owner_id",
                "label_id",
                "id",
                "id",
            )
            .where_pivot("subject_type", "=", "article")
            .with_pivot(["label"]);
            let mut related = labels.lazy_by_id_desc(&owner, 2, None, None);
            let mut pivot_keys = Vec::new();
            while let Some(model) = related.try_next().await? {
                assert!(model.get_relation::<Pivot>("pivot").is_some());
                pivot_keys.push(id(model.get_key()));
            }

            let morph = MorphTo::<Activity>::new("subject_type", "subject_id")
                .target::<KeyedLabel>("label", "id");
            let activity = Activity::query()?.find_or_fail(2).await?;
            let target = morph
                .lazy_by_id(&activity, 2, None, None)
                .try_next()
                .await?
                .unwrap();
            assert_eq!(target.alias(), "label");

            let projected = REACTIONS
                .chunk_map(&owner, |model| ready(Ok(id(model.get_key()) * 2)), 2)
                .await?;
            assert_eq!(
                projected.into_iter().collect::<Vec<_>>(),
                [
                    (0_i64.into(), 202),
                    (1_i64.into(), 204),
                    (2_i64.into(), 206)
                ]
            );
            let mut seen = Vec::new();
            assert!(matches!(
                Article::query()?
                    .chunk_map(
                        |model| {
                            let key = id(model.get_key());
                            seen.push(key);
                            ready(if key == 2 {
                                Err(DatabaseError::InvalidQuery("mapping failed"))
                            } else {
                                Ok(key)
                            })
                        },
                        2
                    )
                    .await,
                Err(DatabaseError::InvalidQuery("mapping failed"))
            ));
            assert_eq!(seen, [1, 2]);
            let mapped = db
                .table("pagination_articles")
                .order_by("id", "asc")
                .limit(3)
                .chunk_map(|row| ready(Ok(id(row.get("id")))), 2)
                .await?;
            assert_eq!(
                mapped
                    .into_iter()
                    .map(|(_, value)| value)
                    .collect::<Vec<_>>(),
                [1, 2, 3]
            );
            Ok(json!({ "descending": descending, "pivot": pivot_keys }))
        }

        Response::json(StatusCode::OK, verify_lazy().await?)
    })
}

fn singular_relation_pages(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        async fn relation_pages() -> DatabaseResult<Json> {
            let owner = Article::query()?.find_or_fail(1).await?;
            let one = HasOne::<Article, Comment>::new("id", "article_id").latest_of_many(None);
            let page = one.paginate(&owner, 2).await?;
            assert_eq!(page.total(), 1);
            assert_eq!(id(page.items()[0].get_key()), 15);
            assert_eq!(
                id(one.simple_paginate(&owner, 2).await?.items()[0].get_key()),
                15
            );

            let morph_one =
                MorphOne::<Article, Comment>::new("id", "article_id", "subject_type", "article")
                    .oldest_of_many(None);
            let first = morph_one.cursor_paginate(&owner, 1).await?;
            assert_eq!(id(first.items()[0].get_key()), 11);
            assert!(first.next_cursor().is_none());

            let activities = Activity::query()?.order_by("id", "asc").get().await?;
            let belongs_to = BelongsTo::<Activity, Article>::new("subject_id", "id")
                .with_default(|_, _| panic!("paginators must read only persisted owners"));
            assert_eq!(
                id(belongs_to.simple_paginate(&activities[0], 1).await?.items()[0].get_key()),
                1
            );
            assert_eq!(
                id(belongs_to.cursor_paginate(&activities[0], 1).await?.items()[0].get_key()),
                1
            );
            let missing = belongs_to
                .paginate_with(
                    &activities[4],
                    PageOptions {
                        per_page: Some(2),
                        page_name: "owners",
                        ..Default::default()
                    },
                )
                .await?;
            assert_eq!(missing.total(), 0);
            assert!(missing.items().is_empty());
            assert!(
                belongs_to
                    .simple_paginate(&activities[4], 2)
                    .await?
                    .items()
                    .is_empty()
            );
            assert!(matches!(
                belongs_to.paginate(&activities[4], 0).await,
                Err(openrust::database::DatabaseError::Pagination(
                    PaginationError::InvalidPerPage
                ))
            ));

            let morph = MorphTo::<Activity>::new("subject_type", "subject_id")
                .target::<Article>("article", "category")
                .target::<KeyedLabel>("label", "id")
                .target::<Comment>("comment", "id");
            let labels = morph.paginate(&activities[1], 2).await?;
            assert_eq!(labels.total(), 1);
            let label = labels.get_by_key("label-2").unwrap();
            assert_eq!(label.alias(), "label");
            assert_eq!(id(label.downcast_ref::<KeyedLabel>().unwrap().get_key()), 2);
            assert!(
                morph
                    .cursor_paginate(&activities[4], 2)
                    .await?
                    .items()
                    .is_empty()
            );
            let mut draft = Activity::new_instance(Attributes::new())?;
            draft
                .set_attribute("subject_type", "comment")
                .set_attribute("subject_id", 16);
            assert!(morph.simple_paginate(&draft, 2).await?.items().is_empty());
            let morph = morph.with_trashed();
            assert_eq!(
                id(morph.simple_paginate(&draft, 2).await?.items()[0]
                    .downcast_ref::<Comment>()
                    .unwrap()
                    .get_key()),
                16
            );
            draft
                .set_attribute("subject_type", "article")
                .set_attribute("subject_id", "a");
            let cursor = morph
                .cursor_paginate_with(
                    &draft,
                    CursorOptions {
                        per_page: Some(1),
                        cursor_name: "subjects",
                        ..Default::default()
                    },
                )
                .await?;
            let cursor = cursor.map(|item| id(item.downcast_ref::<Article>().unwrap().get_key()));
            Ok(json!({"empty": missing.map(|model| id(model.get_key())), "subjects": cursor}))
        }

        Response::json(StatusCode::OK, relation_pages().await?)
    })
}

fn page_defaults(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        async fn verify_defaults() -> DatabaseResult<Json> {
            let page = IndexedArticle::query()?
                .paginate_with(PageOptions {
                    total: Some(41),
                    page_name: "articles",
                    ..Default::default()
                })
                .await?;
            assert_eq!(page.per_page(), 2);
            assert_eq!(page.total(), 41);
            assert_eq!(page.items().len(), 2);
            assert!(page.get_by_key(30_i64).is_some());

            let simple = IndexedArticle::query()?
                .simple_paginate_with(PageOptions::default())
                .await?;
            assert_eq!(simple.per_page(), 2);
            assert!(simple.has_more_pages());
            let cursor = IndexedArticle::query()?
                .cursor_paginate_with(CursorOptions::default())
                .await?;
            assert_eq!(cursor.per_page(), 2);
            assert_eq!(cursor.items().len(), 2);
            let explicit = IndexedArticle::query()?.paginate(1).await?;
            assert_eq!(explicit.per_page(), 1);

            let activities = Activity::query()?.order_by("id", "asc").get().await?;
            let relation = BelongsTo::<Activity, IndexedArticle>::new("subject_id", "id");
            let related = relation
                .paginate_with(&activities[0], PageOptions::default())
                .await?;
            assert_eq!(related.per_page(), 2);
            let empty = relation
                .cursor_paginate_with(&activities[4], CursorOptions::default())
                .await?;
            assert_eq!(empty.per_page(), 2);
            assert!(empty.items().is_empty());
            let empty = relation
                .paginate_with(
                    &activities[4],
                    PageOptions {
                        total: Some(7),
                        ..Default::default()
                    },
                )
                .await?;
            assert_eq!(empty.total(), 7);
            assert!(empty.items().is_empty());
            let morph = MorphTo::<Activity>::new("subject_type", "subject_id")
                .target::<IndexedArticle>("article", "id");
            let morph_page = morph
                .simple_paginate_with(&activities[0], PageOptions::default())
                .await?;
            assert_eq!(morph_page.per_page(), 2);

            let raw = Db::connection(None)?
                .table("pagination_articles")
                .paginate_with(PageOptions::default())
                .await?;
            assert_eq!(raw.per_page(), 15);
            Ok(json!({
                "length": page.map(|model| id(model.get_key())),
                "simple": simple.per_page(),
                "cursor": cursor.per_page(),
                "relation": related.per_page(),
                "raw": raw.per_page()
            }))
        }

        Response::json(StatusCode::OK, verify_defaults().await?)
    })
}

fn page_resolvers(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::database::DatabaseError;

        async fn verify_resolvers() -> DatabaseResult<Json> {
            let mut query = IndexedArticle::query()?;
            query.r#where("id", "<=", 5);
            let calculated = query
                .paginate_with(PageOptions {
                    page_name: "rows",
                    total_using: Some(&|| Ok(None)),
                    per_page_using: Some(&|total| {
                        assert_eq!(total, 5);
                        Ok(Some(total - 3))
                    }),
                    ..Default::default()
                })
                .await?;
            assert_eq!(calculated.total(), 5);
            assert_eq!(calculated.per_page(), 2);

            let supplied = IndexedArticle::query()?
                .paginate_with(PageOptions {
                    total_using: Some(&|| Ok(Some(7))),
                    per_page_using: Some(&|total| {
                        assert_eq!(total, 7);
                        Ok(Some(3))
                    }),
                    ..Default::default()
                })
                .await?;
            assert_eq!(supplied.total(), 7);
            assert_eq!(supplied.items().len(), 3);

            let fallback = IndexedArticle::query()?
                .paginate_with(PageOptions {
                    total: Some(9),
                    total_using: Some(&|| panic!("numeric total must take precedence")),
                    per_page_using: Some(&|total| {
                        assert_eq!(total, 9);
                        Ok(None)
                    }),
                    ..Default::default()
                })
                .await?;
            assert_eq!(fallback.per_page(), 2);

            let activities = Activity::query()?.order_by("id", "asc").get().await?;
            let relation = BelongsTo::<Activity, IndexedArticle>::new("subject_id", "id");
            let empty = relation
                .paginate_with(
                    &activities[4],
                    PageOptions {
                        per_page_using: Some(&|total| {
                            assert_eq!(total, 0);
                            Ok(Some(3))
                        }),
                        ..Default::default()
                    },
                )
                .await?;
            assert_eq!(empty.total(), 0);
            assert_eq!(empty.per_page(), 3);
            assert!(empty.items().is_empty());

            let morph = MorphTo::<Activity>::new("subject_type", "subject_id")
                .target::<IndexedArticle>("article", "category");
            let mut draft = Activity::new_instance(Attributes::new())?;
            draft
                .set_attribute("subject_type", "article")
                .set_attribute("subject_id", "a");
            let targets = morph
                .paginate_with(
                    &draft,
                    PageOptions {
                        per_page_using: Some(&|total| {
                            assert_eq!(total, 2);
                            Ok(Some(1))
                        }),
                        ..Default::default()
                    },
                )
                .await?;
            assert_eq!(targets.total(), 2);
            assert_eq!(targets.per_page(), 1);
            assert_eq!(targets.items().len(), 1);

            let db = Db::connection(None)?;
            let invalid = db
                .table("pagination_articles")
                .paginate_with(PageOptions {
                    total: Some(5),
                    per_page_using: Some(&|_| Ok(Some(0))),
                    ..Default::default()
                })
                .await;
            assert!(matches!(
                invalid,
                Err(DatabaseError::Pagination(PaginationError::InvalidPerPage))
            ));
            Ok(json!({
                "calculated": calculated.map(|model| id(model.get_key())),
                "supplied": supplied.total(),
                "empty": empty.per_page(),
            }))
        }

        Response::json(StatusCode::OK, verify_resolvers().await?)
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

async fn request(address: SocketAddr, path: &str) -> Json {
    serde_json::from_str(&request_body(address, path).await).unwrap()
}

async fn request_body(address: SocketAddr, path: &str) -> String {
    request_with_status(address, path, 200).await
}

async fn request_with_status(address: SocketAddr, path: &str, status: u16) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{path} {response}");
        response.split_once("\r\n\r\n").unwrap().1.to_owned()
    }).await.unwrap()
}

async fn select_count(connection: &mut Transaction) -> DatabaseResult<u64> {
    let row = connection
        .select_one("SHOW SESSION STATUS LIKE 'Com_select'", &[])
        .await?
        .unwrap();
    let Some(Value::Text(count)) = row.get_at(1) else {
        panic!("invalid MySQL counter");
    };
    Ok(count.parse().unwrap())
}

async fn verify(config: Json) -> DatabaseResult<()> {
    let mysql = config.get("url").is_some();
    let base = tempfile::tempdir().unwrap();
    let lang = base.path().join("lang/zh-Hant");
    std::fs::create_dir_all(&lang).unwrap();
    std::fs::write(
        lang.join("pagination.json"),
        r#"{"previous":"上一頁","next":"下一頁 <more>"}"#,
    )
    .unwrap();
    let views = base.path().join("resources/views/vendor/pagination");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(views.join("custom.blade.html"),
        r#"{{ $title }}:{{ $paginator->total }} @foreach ($elements as $element){{ $element->label }}@endforeach"#
    ).unwrap();
    let mut app = Application::configure(base.path().to_path_buf())
        .with_routing(Routing::default().api(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    StreamedComment::register_model_event(&app, ModelEvent::Retrieved, RejectThirdComment)?;
    let repository = app.container().get::<dyn Repository>("config").unwrap();
    // 此夹具用 locale 参数独立验证翻译器回退，不让 HTTP 语言候选校验提前接管。
    repository
        .set("app.locale_query_parameter", Json::Null)
        .unwrap();
    repository
        .set("database.default", json!("pagination_test"))
        .unwrap();
    repository
        .set("database.connections.pagination_test", config)
        .unwrap();
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)?;
    for sql in [
        "DROP TABLE IF EXISTS pagination_memberships",
        "DROP TABLE IF EXISTS pagination_labels",
        "CREATE TABLE pagination_labels (id BIGINT PRIMARY KEY, name VARCHAR(30))",
        "INSERT INTO pagination_labels VALUES (1, 'first'), (2, 'second'), (3, 'third'), (4, 'other'), (5, 'image')",
        "CREATE TABLE pagination_memberships (id BIGINT PRIMARY KEY, owner_id BIGINT, label_id BIGINT, subject_type VARCHAR(30), label VARCHAR(30), weight INTEGER)",
        "INSERT INTO pagination_memberships VALUES (201, 1, 1, 'article', 'red', 2), (202, 1, 2, 'article', 'blue', 1), (203, 1, 3, 'article', 'green', 3), (204, 2, 4, 'article', 'other', 0), (205, 1, 5, 'image', 'excluded', 0)",
        "DROP TABLE IF EXISTS pagination_activities",
        "CREATE TABLE pagination_activities (id BIGINT PRIMARY KEY, subject_type VARCHAR(30), subject_id BIGINT)",
        "INSERT INTO pagination_activities VALUES (1, 'article', 1), (2, 'label', 2), (3, 'article', 1), (4, 'article', 2), (5, NULL, NULL)",
        "DROP TABLE IF EXISTS pagination_reactions",
        "DROP TABLE IF EXISTS pagination_comments",
        "DROP TABLE IF EXISTS pagination_articles",
        "CREATE TABLE pagination_articles (id BIGINT PRIMARY KEY, rank_number INTEGER, category VARCHAR(30))",
        "INSERT INTO pagination_articles VALUES (1, 1, 'a'), (2, 1, 'a'), (3, 2, 'b'), (4, 2, 'b'), (5, 3, 'b')",
        "CREATE TABLE pagination_comments (id BIGINT PRIMARY KEY, article_id BIGINT, subject_type VARCHAR(30), visible INTEGER, deleted_at TIMESTAMP NULL)",
        "INSERT INTO pagination_comments (id, article_id, subject_type, visible) VALUES (11, 1, 'article', 1), (12, 1, 'article', 0), (13, 1, 'article', 1), (14, 2, 'article', 1), (15, 1, 'article', 0), (16, 1, 'image', 0), (17, NULL, 'article', 1)",
        "UPDATE pagination_comments SET deleted_at = '2025-01-01 00:00:00' WHERE id = 16",
        "CREATE TABLE pagination_reactions (id BIGINT PRIMARY KEY, comment_id BIGINT)",
        "INSERT INTO pagination_reactions VALUES (101, 11), (102, 11), (103, 12), (104, 14), (105, 16)",
    ] {
        connection.statement(sql, &[]).await?;
    }
    let table = || connection.table("pagination_articles");
    if mysql {
        let mut session = connection.begin_transaction().await?;
        let before = select_count(&mut session).await?;
        session
            .table("pagination_articles")
            .order_by("id", "asc")
            .paginate(2)
            .await?;
        let counted = select_count(&mut session).await?;
        session
            .table("pagination_articles")
            .order_by("id", "asc")
            .simple_paginate(2)
            .await?;
        let simple = select_count(&mut session).await?;
        session
            .table("pagination_articles")
            .order_by("id", "asc")
            .cursor_paginate(2)
            .await?;
        assert_eq!(counted - before, 2);
        assert_eq!(simple - counted, 1);
        assert_eq!(select_count(&mut session).await? - simple, 1);
        let before_injected = select_count(&mut session).await?;
        let injected = session
            .table("pagination_articles")
            .order_by("id", "asc")
            .paginate_with(PageOptions {
                per_page: Some(2),
                page: Some(2),
                total: Some(1),
                ..Default::default()
            })
            .await?;
        assert_eq!(injected.total(), 1);
        assert_eq!(ids(injected.items()), [3, 4]);
        let after_injected = select_count(&mut session).await?;
        assert_eq!(after_injected - before_injected, 1);
        let empty = session
            .table("pagination_articles")
            .paginate_with(PageOptions {
                total: Some(0),
                ..Default::default()
            })
            .await?;
        assert_eq!(empty.total(), 0);
        assert!(empty.items().is_empty());
        assert_eq!(select_count(&mut session).await? - after_injected, 0);
        let before_resolver = select_count(&mut session).await?;
        let supplied = session
            .table("pagination_articles")
            .order_by("id", "asc")
            .paginate_with(PageOptions {
                total_using: Some(&|| Ok(Some(5))),
                per_page_using: Some(&|total| Ok(Some(total - 3))),
                ..Default::default()
            })
            .await?;
        assert_eq!(ids(supplied.items()), [1, 2]);
        let resolved = select_count(&mut session).await?;
        assert_eq!(resolved - before_resolver, 1);

        let bad_size = session
            .table("pagination_articles")
            .paginate_with(PageOptions {
                per_page_using: Some(&|total| {
                    assert_eq!(total, 5);
                    Err(openrust::database::DatabaseError::InvalidQuery(
                        "size callback failed",
                    ))
                }),
                ..Default::default()
            })
            .await;
        assert!(matches!(
            bad_size,
            Err(openrust::database::DatabaseError::InvalidQuery(
                "size callback failed"
            ))
        ));
        let counted_only = select_count(&mut session).await?;
        assert_eq!(counted_only - resolved, 1);

        let bad_total = session
            .table("pagination_articles")
            .paginate_with(PageOptions {
                total_using: Some(&|| {
                    Err(openrust::database::DatabaseError::InvalidQuery(
                        "total callback failed",
                    ))
                }),
                per_page_using: Some(&|_| panic!("total resolution failed")),
                ..Default::default()
            })
            .await;
        assert!(matches!(
            bad_total,
            Err(openrust::database::DatabaseError::InvalidQuery(
                "total callback failed"
            ))
        ));
        assert_eq!(select_count(&mut session).await? - counted_only, 0);
        session.roll_back(None).await?;
    }
    let grouped = table()
        .select(["category"])
        .group_by(&["category"])
        .order_by("category", "asc")
        .paginate(1)
        .await?;
    assert_eq!(grouped.total(), 2);
    let distinct = table()
        .select(["rank_number"])
        .distinct()
        .order_by("rank_number", "asc")
        .paginate(2)
        .await?;
    assert_eq!(distinct.total(), 3);
    let empty = table().r#where("id", "<", 0).paginate(2).await?;
    assert!(empty.items().is_empty());
    assert_eq!((empty.total(), empty.last_page()), (0, 1));
    assert!(table().paginate(0).await.is_err());
    assert!(table().cursor_paginate(2).await.is_err());
    assert!(
        table()
            .paginate_with(PageOptions {
                per_page: Some(2),
                page: Some(u64::MAX),
                ..Default::default()
            })
            .await
            .is_err()
    );

    let seek = || {
        table()
            .select(["id", "rank_number as score"])
            .r#where("category", "=", "a")
            .or_where("category", "=", "b")
            .order_by("score", "asc")
            .order_by("id", "desc")
    };
    let first = seek().cursor_paginate(2).await?;
    assert_eq!(ids(first.items()), vec![2, 1]);
    let token = first.next_cursor().unwrap().encode()?;
    let cursor = Cursor::from_encoded(&token)?;
    let second = seek()
        .cursor_paginate_with(CursorOptions {
            per_page: Some(2),
            cursor: Some(&cursor),
            ..Default::default()
        })
        .await?;
    assert_eq!(ids(second.items()), vec![4, 3]);
    let previous = second.previous_cursor().unwrap();
    let back = seek()
        .cursor_paginate_with(CursorOptions {
            per_page: Some(2),
            cursor: Some(previous),
            ..Default::default()
        })
        .await?;
    assert_eq!(ids(back.items()), vec![2, 1]);
    assert!(back.previous_cursor().is_none());
    assert!(
        table()
            .order_by("category", "asc")
            .cursor_paginate_with(CursorOptions {
                per_page: Some(2),
                cursor: Some(&cursor),
                ..Default::default()
            })
            .await
            .is_err()
    );

    let parent = Article::query_on(&connection).find(1).await?.unwrap();
    let comments = HasMany::<Article, Comment>::new("id", "article_id");
    let simple = comments.simple_paginate(&parent, 1).await?;
    assert_eq!(
        simple
            .items()
            .iter()
            .map(|comment| id(comment.get_key()))
            .collect::<Vec<_>>(),
        [11]
    );
    assert!(simple.next_page_url().is_some());
    let unsaved = Article::new_instance_on(&connection, Attributes::new())?;
    let empty = comments.paginate(&unsaved, 2).await?;
    assert_eq!(empty.total(), 0);
    assert!(empty.items().is_empty());

    let simple_labels = labels().simple_paginate(&parent, 1).await?;
    assert_eq!(simple_labels.items().len(), 1);
    assert!(
        simple_labels.items()[0]
            .get_relation::<Pivot>("pivot")
            .is_some()
    );
    assert!(simple_labels.next_page_url().is_some());
    assert_eq!(labels().paginate(&unsaved, 2).await?.total(), 0);

    let through = REACTIONS
        .cursor_paginate_with(
            &parent,
            CursorOptions {
                per_page: Some(2),
                columns: &["id"],
                ..Default::default()
            },
        )
        .await?;
    assert_eq!(
        through
            .items()
            .iter()
            .map(|reaction| id(reaction.get_key()))
            .collect::<Vec<_>>(),
        [101, 102]
    );
    let following = REACTIONS
        .cursor_paginate_with(
            &parent,
            CursorOptions {
                per_page: Some(2),
                cursor: through.next_cursor(),
                ..Default::default()
            },
        )
        .await?;
    assert_eq!(
        following
            .items()
            .iter()
            .map(|reaction| id(reaction.get_key()))
            .collect::<Vec<_>>(),
        [103]
    );
    let all = REACTIONS
        .with_trashed_parents()
        .simple_paginate(&parent, 10)
        .await?;
    assert_eq!(
        all.items()
            .iter()
            .map(|reaction| id(reaction.get_key()))
            .collect::<Vec<_>>(),
        [101, 102, 103, 105]
    );
    assert_eq!(REACTIONS.paginate(&unsaved, 2).await?.total(), 0);

    let latest = REACTIONS
        .one()
        .latest_of_many(None)
        .paginate(&parent, 2)
        .await?;
    assert_eq!(latest.total(), 1);
    assert_eq!(id(latest.items()[0].get_key()), 103);
    let (address, server) = serve(app).await;
    let manual = request(address, "/api/manual?entries=2&filter=active").await;
    assert_eq!(manual["count"], 2);
    assert_eq!(manual["first"], false);
    assert_eq!(manual["last"], false);
    assert_eq!(manual["page"]["data"], json!(["3", "4"]));
    assert_eq!(manual["page"]["total"], 5);
    assert_eq!(manual["page"]["from"], 3);
    assert_eq!(manual["page"]["to"], 4);
    assert_eq!(
        manual["range"][0][1],
        "/api/manual?fixed=1&filter=active&entries=1#results"
    );

    let manual = request(address, "/api/manual-simple").await;
    assert_eq!(manual["data"], json!([10, 20]));
    assert_eq!(manual["next_page_url"], "/api/manual-simple?after=2");
    let first = request(address, "/api/manual-cursor?state=open").await;
    assert_eq!(first["first"], true);
    assert_eq!(first["page"]["data"], json!(["1", "2"]));
    let second = request(address, first["page"]["next_page_url"].as_str().unwrap()).await;
    assert_eq!(second["count"], 2);
    assert_eq!(second["first"], false);
    assert_eq!(second["page"]["data"], json!(["3", "4"]));
    let last = request(address, second["page"]["next_page_url"].as_str().unwrap()).await;
    assert_eq!(last["last"], true);
    assert_eq!(last["page"]["data"], json!(["5"]));
    let first_again = request(address, second["page"]["prev_page_url"].as_str().unwrap()).await;
    assert_eq!(first_again["first"], true);
    assert_eq!(first_again["page"]["data"], first["page"]["data"]);
    assert!(first_again["page"]["prev_page_url"].is_null());

    let (length, simple) = tokio::join!(
        request(address, "/api/length?articles=2&state=in+review",),
        request(address, "/api/simple?page=3",),
    );
    assert_eq!(length["data"], json!([3, 4]));
    assert_eq!(length["total"], 5);
    assert_eq!(
        length["next_page_url"],
        "/api/length?state=in+review&articles=3#results%20here"
    );
    assert_eq!(length["links"][2]["active"], true);
    assert_eq!(length["links"][2]["page"], 2);
    assert_eq!(simple["data"], json!([5]));
    assert!(simple["next_page_url"].is_null());
    assert!(simple.get("total").is_none());
    let multiple = request(
        address,
        "/api/multiple?articles=2&comments=1&state=in+review",
    )
    .await;
    assert_eq!(multiple["articles"]["data"], json!([3, 4]));
    assert_eq!(multiple["comments"]["data"], json!([11, 12]));
    let articles = multiple["articles"]["next_page_url"].as_str().unwrap();
    let comments = multiple["comments"]["next_page_url"].as_str().unwrap();
    assert_eq!(
        articles,
        "/api/multiple?comments=1&state=in+review&articles=3#article%20list"
    );
    assert_eq!(
        comments,
        "/api/multiple?articles=2&state=in+review&comments=2#comment%20list"
    );
    let next = request(address, articles.split_once('#').unwrap().0).await;
    assert_eq!(next["articles"]["data"], json!([5]));
    assert_eq!(next["comments"]["data"], json!([11, 12]));
    let next = request(address, comments.split_once('#').unwrap().0).await;
    assert_eq!(next["articles"]["data"], json!([3, 4]));
    assert_eq!(next["comments"]["data"], json!([13, 14]));

    let first = request(address, "/api/cursor?state=open&cursor=malformed").await;
    assert_eq!(first["data"], json!([2, 1]));
    let second = request(address, first["next_page_url"].as_str().unwrap()).await;
    assert_eq!(second["data"], json!([4, 3]));
    let last = request(address, second["next_page_url"].as_str().unwrap()).await;
    assert_eq!(last["data"], json!([5]));
    assert!(last["next_page_url"].is_null());
    assert_eq!(
        request(address, second["prev_page_url"].as_str().unwrap()).await["data"],
        json!([2, 1])
    );
    let related = request(address, "/api/related-length?comments=2&visible=yes").await;
    assert_eq!(related["total"], 3);
    assert_eq!(related["data"], json!([13]));
    let previous = related["prev_page_url"].as_str().unwrap();
    assert!(previous.contains("visible=yes"));
    assert_eq!(request(address, previous).await["data"], json!([11, 12]));
    let first = request(address, "/api/related-cursor?filter=all").await;
    assert_eq!(first["data"], json!([11, 12]));
    let next = first["next_page_url"].as_str().unwrap();
    assert!(next.contains("comments=") && next.contains("filter=all"));
    let second = request(address, next).await;
    assert_eq!(second["data"], json!([13, 15]));
    assert!(second["next_page_url"].is_null());
    let previous = second["prev_page_url"].as_str().unwrap();
    assert_eq!(request(address, previous).await["data"], json!([11, 12]));
    let through = request(address, "/api/through?reactions=2&filter=all").await;
    assert_eq!(through["total"], 3);
    assert_eq!(through["data"], json!([103]));
    let previous = through["prev_page_url"].as_str().unwrap();
    assert!(previous.contains("filter=all"));
    assert_eq!(request(address, previous).await["data"], json!([101, 102]));
    let labels = request(address, "/api/pivot-length?labels=2&filter=all").await;
    assert_eq!(labels["total"], 3);
    assert_eq!(labels["data"], json!([[3, "green"]]));
    let previous = labels["prev_page_url"].as_str().unwrap();
    assert!(previous.contains("filter=all"));
    assert_eq!(
        request(address, previous).await["data"],
        json!([[1, "red"], [2, "blue"]])
    );

    let first = request(address, "/api/pivot-cursor?filter=all").await;
    assert_eq!(first["data"], json!([[2, 202, "blue"], [1, 201, "red"]]));
    let next = first["next_page_url"].as_str().unwrap();
    assert!(next.contains("labels=") && next.contains("filter=all"));
    let second = request(address, next).await;
    assert_eq!(second["data"], json!([[3, 203, "green"]]));
    assert!(second["next_page_url"].is_null());
    let previous = second["prev_page_url"].as_str().unwrap();
    assert_eq!(request(address, previous).await["data"], first["data"]);
    let (chinese, english) = tokio::join!(
        request_body(address, "/api/links?locale=zh-Hant&filter=all"),
        request_body(address, "/api/links?filter=all"),
    );
    assert!(chinese.contains("上一頁"));
    assert!(chinese.contains("下一頁 &lt;more&gt;"));
    assert!(english.contains("« Previous"));
    assert!(english.contains("Next »"));
    let regional = request_body(address, "/api/links?locale=zh-TW").await;
    assert!(regional.contains("« Previous"));
    assert!(!regional.contains("上一頁"));
    assert!(english.contains("aria-current=\"page\""));
    assert!(english.contains("aria-disabled=\"true\""));
    assert!(english.contains("filter=all&amp;page=2#results"));
    let last = request_body(address, "/api/links?page=3").await;
    assert!(last.contains("rel=\"prev\""));
    assert!(!last.contains("rel=\"next\""));
    let simple = request_body(address, "/api/links?mode=simple&page=2").await;
    assert!(simple.contains("rel=\"prev\"") && simple.contains("rel=\"next\""));
    assert!(!simple.contains("aria-current"));
    let cursor = request_body(address, "/api/links?mode=cursor").await;
    assert!(cursor.contains("rel=\"next\"") && cursor.contains("cursor="));
    assert!(!cursor.contains("aria-current"));
    let (_, next) = cursor.split_once(r#"rel="next" href=""#).unwrap();
    let (next, _) = next.split_once('"').unwrap();
    let following = request_body(address, &next.replace("&amp;", "&")).await;
    assert!(following.contains(r#"rel="prev""#));

    let json_links = request(address, "/api/links?mode=json&locale=zh-Hant").await;
    assert_eq!(json_links["links"][0]["label"], "上一頁");
    assert_eq!(json_links["links"][4]["label"], "下一頁 <more>");
    assert!(
        request_body(address, "/api/links?mode=single")
            .await
            .trim()
            .is_empty()
    );
    let custom = request_body(address, "/api/links?mode=custom").await;
    assert!(custom.contains("&lt;Custom&gt;:5 123"));
    std::fs::write(
        views.join("simple-tailwind.blade.html"),
        "Override {{ $paginator->next }}",
    )
    .unwrap();
    let overridden = request_body(address, "/api/links?mode=simple").await;
    assert!(overridden.contains("Override Next »"));

    for id in 6..=17 {
        connection
            .statement(
                "INSERT INTO pagination_articles VALUES (?, 0, 'window')",
                &[Value::Integer(id)],
            )
            .await?;
    }
    let window = request_body(address, "/api/links?mode=window&page=9").await;
    assert_eq!(window.matches(">...</span>").count(), 2);
    assert!(window.contains(r#"aria-current="page""#) && window.contains(">9</span>"));
    assert!(!window.contains(">8</a>") && !window.contains(">10</a>"));
    let found = request(address, "/api/relation-find").await;
    assert_eq!(
        found,
        json!({
            "found": 11, "draft_owner": 1, "draft_default": 1, "direct": 13,
            "polymorphic": 15, "through": 103, "attached": 2, "pivot_owner": 1
        })
    );
    for (table, count) in [
        ("pagination_comments", 7),
        ("pagination_reactions", 5),
        ("pagination_labels", 5),
        ("pagination_memberships", 5),
    ] {
        assert_eq!(connection.table(table).count("*").await?, count);
    }

    let batch = request(address, "/api/relation-many").await;
    assert_eq!(
        batch,
        json!({
            "direct": [15, 11], "polymorphic": [11, 15], "through": [102, 103],
            "pivot": [1, 3], "morph_pivot": [1, 3]
        })
    );

    let selected = request(address, "/api/relation-lookup").await;
    assert_eq!(selected, json!({"hit": 11, "through": 103, "attached": 2}));
    request_with_status(address, "/api/relation-lookup?mode=missing", 404).await;
    connection
        .statement(
            "INSERT INTO pagination_memberships VALUES (206, 1, 2, 'article', 'duplicate', 4)",
            &[],
        )
        .await?;
    let duplicate = request_with_status(address, "/api/relation-lookup?mode=duplicate", 409).await;
    assert_eq!(
        serde_json::from_str::<Json>(&duplicate).unwrap(),
        json!({"matches":2})
    );
    let strict = request(address, "/api/relation-strict").await;
    let mut pivot_labels = strict["pivot_labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|label| label.as_str().unwrap())
        .collect::<Vec<_>>();
    pivot_labels.sort_unstable();
    assert_eq!(pivot_labels, ["blue", "duplicate"]);
    assert_eq!(strict["fallback"], json!([3]));
    assert_eq!(strict["direct"], json!([11, 15]));
    request_with_status(address, "/api/relation-strict?mode=missing", 404).await;
    connection
        .statement("DELETE FROM pagination_memberships WHERE id = 206", &[])
        .await?;
    assert_eq!(request(address, "/api/relation-lookup").await, selected);

    let length = request(address, "/api/keyed").await;
    assert_eq!(length["total"], 5);
    assert_eq!(length["data"], json!({"label-1": 1, "label-2": 2}));
    let simple = request(address, "/api/keyed?mode=simple").await;
    assert_eq!(simple["data"], length["data"]);
    assert_eq!(simple["next_page_url"], "/api/keyed?mode=simple&page=2");
    let second = request(address, simple["next_page_url"].as_str().unwrap()).await;
    assert_eq!(second["data"], json!({"label-3": 3, "label-4": 4}));
    let cursor = request(address, "/api/keyed?mode=cursor").await;
    assert_eq!(cursor["data"], json!([1, 2]));
    let next = request(address, cursor["next_page_url"].as_str().unwrap()).await;
    assert_eq!(next["data"], json!([3, 4]));
    let previous = request(address, next["prev_page_url"].as_str().unwrap()).await;
    assert_eq!(previous["data"], cursor["data"]);
    let pivot = request(address, "/api/keyed-pivot").await;
    assert_eq!(pivot["data"], json!({"pivot-1": 1, "pivot-2": 2}));

    for path in [
        "/api/morph",
        "/api/morph?mode=simple",
        "/api/morph?mode=cursor",
    ] {
        let first = request(address, path).await;
        let subject = &first["data"][0]["subject"];
        assert_eq!(first["data"][0]["id"], 1);
        assert_eq!(subject["id"], 1);
        assert_eq!(subject["comments_count"], 4);
        let mut ids: Vec<_> = subject["comments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|comment| comment["id"].as_i64().unwrap())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, [11, 12, 13, 15]);
        let label = &first["data"][1]["subject"];
        assert_eq!(first["data"][1]["id"], 2);
        assert_eq!(label["id"], 2);
        assert_eq!(label["memberships_count"], 1);
        assert!(label.get("comments").is_none());

        let second = request(address, first["next_page_url"].as_str().unwrap()).await;
        assert_eq!(second["data"][0]["id"], 3);
        assert_eq!(second["data"][0]["subject"]["comments_count"], 4);
        assert_eq!(second["data"][1]["id"], 4);
        assert_eq!(second["data"][1]["subject"]["comments_count"], 1);
        assert_eq!(second["data"][1]["subject"]["comments"][0]["id"], 14);
        let last = request(address, second["next_page_url"].as_str().unwrap()).await;
        assert_eq!(last["data"][0]["id"], 5);
        assert!(last["data"][0]["subject"].is_null());
        assert!(last["next_page_url"].is_null());
    }
    connection
        .statement(
            "INSERT INTO pagination_activities VALUES (6, 'unregistered', 1)",
            &[],
        )
        .await?;
    let clean = request(address, "/api/morph?mode=cursor").await;
    assert_eq!(clean["data"][0]["subject"]["comments_count"], 4);
    let clean = request(address, clean["next_page_url"].as_str().unwrap()).await;
    let third = clean["next_page_url"].as_str().unwrap();
    request_with_status(address, third, 500).await;
    connection
        .statement("DELETE FROM pagination_activities WHERE id = 6", &[])
        .await?;
    let recovered = request(address, third).await;
    assert_eq!(recovered["data"][0]["id"], 5);

    let empty = request(address, "/api/morph?page=99").await;
    assert_eq!(empty["total"], 5);
    assert_eq!(empty["data"], json!([]));

    let related = request(address, "/api/relationship-cursor").await;
    assert_eq!(
        related,
        json!({
            "comments": [11, 12, 13, 15], "owner": 1,
            "through": [101, 102, 103], "pivot": ["red", "blue", "green"],
        })
    );
    let chunks = request(address, "/api/relationship-chunk").await;
    assert_eq!(chunks["comments"], json!([[1, [13, 12]], [2, [11]]]));
    assert_eq!(chunks["reactions"], json!([[1, [101, 102]], [2, [103]]]));
    assert_eq!(
        chunks["pivots"],
        json!([[1, ["red", "blue"]], [2, ["green"]]])
    );
    assert_eq!(chunks["raw"], json!([[1, [3, 4]], [2, [5]]]));
    assert_eq!(
        request(address, "/api/inverse-chunk").await,
        json!([[1, 2], [2, 1]])
    );
    let batches = request(address, "/api/chunk-by-id").await;
    assert_eq!(batches["raw"], json!([[1, [4, 3]], [2, [2]]]));
    assert_eq!(
        batches["models"],
        json!([[1, [1, 2]], [2, [3, 4]], [3, [5]]])
    );
    assert_eq!(
        batches["pivots"],
        json!([[1, [[3, "green"], [2, "blue"]]], [2, [[1, "red"]]]])
    );
    let visited = request(address, "/api/each-batch").await;
    assert_eq!(
        visited["indices"],
        json!([[1, 0], [2, 1], [3, 2], [4, 3], [5, 4]])
    );
    assert_eq!(
        visited["relations"],
        json!([[11, 0], [12, 1], [13, 2], [15, 3]])
    );
    let lazy = request(address, "/api/lazy-batches").await;
    assert_eq!(lazy, json!({"descending": [4, 3, 2], "pivot": [3, 2, 1]}));
    let singular = request(address, "/api/singular-pages?owners=3").await;
    assert_eq!(singular["empty"]["current_page"], 3);
    assert_eq!(singular["empty"]["data"], json!([]));
    assert_eq!(
        singular["empty"]["prev_page_url"],
        "/api/singular-pages?owners=2"
    );
    assert_eq!(singular["subjects"]["data"], json!([1]));
    let next = request(
        address,
        singular["subjects"]["next_page_url"].as_str().unwrap(),
    )
    .await;
    assert_eq!(next["subjects"]["data"], json!([2]));
    let previous = request(address, next["subjects"]["prev_page_url"].as_str().unwrap()).await;
    assert_eq!(previous["subjects"]["data"], json!([1]));
    let defaults = request(address, "/api/page-defaults?articles=2").await;
    assert_eq!(defaults["length"]["total"], 41);
    assert_eq!(defaults["length"]["per_page"], 2);
    assert_eq!(defaults["length"]["current_page"], 2);
    assert_eq!(defaults["length"]["data"], json!({"30": 3, "40": 4}));
    assert_eq!(defaults["simple"], 2);
    assert_eq!(defaults["cursor"], 2);
    assert_eq!(defaults["relation"], 2);
    assert_eq!(defaults["raw"], 15);
    let resolvers = request(address, "/api/page-resolvers?rows=2").await;
    assert_eq!(resolvers["calculated"]["total"], 5);
    assert_eq!(resolvers["calculated"]["per_page"], 2);
    assert_eq!(resolvers["calculated"]["current_page"], 2);
    assert_eq!(resolvers["calculated"]["data"], json!({"30": 3, "40": 4}));
    assert_eq!(resolvers["supplied"], 7);
    assert_eq!(resolvers["empty"], 3);
    let model = request(address, "/api/model-cursor").await;
    assert_eq!(model["id"], "17");
    assert_eq!(model["retrieved"], true);
    assert!(model.get("article").is_none());
    let soft_scoped = StreamedComment::query_on(&connection)
        .order_by("id", "desc")
        .limit(2);
    let keys: Vec<_> = soft_scoped.cursor().try_collect().await?;
    assert_eq!(
        keys.iter().map(|row| id(row.get_key())).collect::<Vec<_>>(),
        [17, 15]
    );

    server.abort();
    connection
        .statement("DROP TABLE pagination_activities", &[])
        .await?;
    connection
        .statement("DROP TABLE pagination_memberships", &[])
        .await?;
    connection
        .statement("DROP TABLE pagination_labels", &[])
        .await?;
    connection
        .statement("DROP TABLE pagination_reactions", &[])
        .await?;
    connection
        .statement("DROP TABLE pagination_comments", &[])
        .await?;
    connection
        .statement("DROP TABLE pagination_articles", &[])
        .await?;
    Ok(())
}

#[tokio::test]
async fn pagination_on_sqlite_and_http() {
    verify(json!({"driver":"sqlite","database":":memory:"}))
        .await
        .unwrap();
}

#[tokio::test]
async fn pagination_on_mysql_and_http() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url})).await.unwrap();
    }
}

async fn serve(app: Application) -> (SocketAddr, JoinHandle<()>) {
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (address, server)
}

struct StyleProvider(fn() -> ViewResult<()>);

impl ServiceProvider for StyleProvider {
    fn boot(&mut self, _: &Application) -> HttpFuture<'_, ProviderResult> {
        Box::pin(async { Ok(self.0()?) })
    }
}

async fn styled_server(
    style: fn() -> ViewResult<()>,
) -> (tempfile::TempDir, SocketAddr, JoinHandle<()>) {
    styled_server_with_config(style, json!({"driver":"sqlite", "database":":memory:"})).await
}

async fn styled_server_with_config(
    style: fn() -> ViewResult<()>,
    config: Json,
) -> (tempfile::TempDir, SocketAddr, JoinHandle<()>) {
    let base = tempfile::tempdir().unwrap();
    let views = base.path().join("resources/views");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(
        views.join("numbered.blade.html"),
        "numbered {{ $paginator->total }}",
    )
    .unwrap();
    std::fs::write(
        views.join("simple.blade.html"),
        "simple {{ $paginator->next }}",
    )
    .unwrap();
    let mut app = Application::configure(base.path().to_path_buf())
        .with_providers(vec![Box::new(StyleProvider(style))])
        .with_routing(Routing::default().api(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.container().get::<dyn Repository>("config").unwrap();
    repository
        .set("database.default", json!("pagination"))
        .unwrap();
    repository
        .set("database.connections.pagination", config)
        .unwrap();
    let connection = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    connection
        .statement(
            "CREATE TABLE pagination_articles (id BIGINT PRIMARY KEY)",
            &[],
        )
        .await
        .unwrap();
    connection
        .statement("INSERT INTO pagination_articles VALUES (1), (2), (3)", &[])
        .await
        .unwrap();
    let (address, server) = serve(app).await;
    (base, address, server)
}

#[tokio::test]
async fn pagination_default_styles_are_scoped_to_the_application() {
    let (_root_v3, v3, server_v3) = styled_server(Paginator::use_bootstrap_three).await;
    let (_root_v4, v4, server_v4) = styled_server(Paginator::use_bootstrap).await;
    let (_root_v5, v5, server_v5) = styled_server(Paginator::use_bootstrap_five).await;
    let (_root_tailwind, tailwind, server_tailwind) = styled_server(|| {
        Paginator::use_bootstrap_four()?;
        Paginator::use_tailwind()
    })
    .await;
    let (_root_custom, custom, server_custom) = styled_server(|| {
        Paginator::default_view("numbered")?;
        Paginator::default_simple_view("simple")
    })
    .await;

    let (three, four, five, tail, own) = tokio::join!(
        request_body(v3, "/api/links"),
        request_body(v4, "/api/links"),
        request_body(v5, "/api/links"),
        request_body(tailwind, "/api/links"),
        request_body(custom, "/api/links"),
    );
    assert!(three.contains(r#"class="pagination""#));
    assert!(three.contains(r#"class="active" aria-current="page""#));
    assert!(!three.contains("page-link"));
    assert!(four.contains(r#"class="page-item active" aria-current="page""#));
    assert!(!four.contains("d-sm-none"));
    assert!(five.contains("d-sm-none") && five.contains("fw-semibold"));
    assert!(five.contains(r#"class="page-item active" aria-current="page""#));
    assert!(tail.contains("rounded-md shadow-sm"));
    assert_eq!(own, "numbered 3");

    for address in [v3, v4, v5] {
        let (simple, cursor) = tokio::join!(
            request_body(address, "/api/links?mode=simple"),
            request_body(address, "/api/links?mode=cursor"),
        );
        assert!(simple.contains(r#"class="pagination""#) && simple.contains("Next »"));
        assert!(cursor.contains(r#"class="pagination""#) && cursor.contains("cursor="));
        assert!(!simple.contains("aria-current") && !cursor.contains("aria-current"));
    }
    assert_eq!(
        request_body(custom, "/api/links?mode=simple").await,
        "simple Next »"
    );
    assert_eq!(
        request_body(custom, "/api/links?mode=cursor").await,
        "simple Next »"
    );

    let semantic = request_body(v5, "/api/links?mode=semantic").await;
    assert!(semantic.contains("ui pagination menu") && semantic.contains("right chevron icon"));
    assert!(semantic.contains(r#"aria-current="page""#));

    for server in [
        server_v3,
        server_v4,
        server_v5,
        server_tailwind,
        server_custom,
    ] {
        server.abort();
    }
}

#[test]
fn rejects_invalid_manual_page_sizes_and_cursor_parameters() {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    for token in [
        "%%",
        &URL_SAFE_NO_PAD.encode("not JSON"),
        &URL_SAFE_NO_PAD.encode("[1,2]"),
        &URL_SAFE_NO_PAD.encode(r#"{"id":2}"#),
    ] {
        assert!(matches!(
            Cursor::from_encoded(token),
            Err(PaginationError::InvalidCursor)
        ));
    }
    assert!(matches!(
        Paginator::new(Vec::<i64>::new(), 0, None, PaginatorOptions::default()),
        Err(PaginationError::InvalidPerPage)
    ));
    assert!(matches!(
        LengthAwarePaginator::new(vec![1], 1, 2, Some(u64::MAX), PaginatorOptions::default()),
        Err(PaginationError::OffsetOverflow)
    ));
    assert!(matches!(
        Cursor::new([("id", Value::Null)], true),
        Err(PaginationError::InvalidCursor)
    ));
    assert!(matches!(
        Cursor::new([("id", Value::Integer(1)), ("id", Value::Integer(2))], true),
        Err(PaginationError::InvalidCursor)
    ));
    let cursor = Cursor::new(
        [
            ("id", Value::Integer(1)),
            ("slug", Value::Text("entry".into())),
        ],
        false,
    )
    .unwrap();
    let restored = Cursor::from_encoded(&cursor.encode().unwrap()).unwrap();
    assert!(restored.points_to_previous_items());
    assert_eq!(
        restored.parameters(&["slug", "id"]).unwrap(),
        [Value::Text("entry".into()), Value::Integer(1)]
    );
}

#[test]
fn length_aware_url_window_preserves_each_segment() {
    fn numbers(segment: Option<Vec<(u64, String)>>) -> Vec<u64> {
        segment
            .unwrap_or_default()
            .into_iter()
            .map(|(page, url)| {
                assert_eq!(url, format!("/?page={page}"));
                page
            })
            .collect()
    }
    let make = |total, current| {
        LengthAwarePaginator::new(
            vec!["item"],
            total,
            1,
            Some(current),
            PaginatorOptions::default(),
        )
        .unwrap()
    };

    let small = make(2, 1);
    assert!(UrlWindow::new(&small).has_pages());
    let window = UrlWindow::make(&small);
    assert_eq!(numbers(window.first), [1, 2]);
    assert!(window.slider.is_none() && window.last.is_none());

    let opening = UrlWindow::make(&make(20, 3));
    assert_eq!(numbers(opening.first), (1..=10).collect::<Vec<_>>());
    assert!(opening.slider.is_none());
    assert_eq!(numbers(opening.last), [19, 20]);

    let mut page = make(20, 8);
    page.on_each_side(1);
    let middle = UrlWindow::new(&page).get();
    assert_eq!(numbers(middle.first), [1, 2]);
    assert_eq!(numbers(middle.slider), [7, 8, 9]);
    assert_eq!(numbers(middle.last), [19, 20]);
    assert_eq!(
        page.link_collection()
            .iter()
            .filter(|link| link["label"] == "...")
            .count(),
        2
    );

    let closing = UrlWindow::make(&make(20, 18));
    assert_eq!(numbers(closing.first), [1, 2]);
    assert!(closing.slider.is_none());
    assert_eq!(numbers(closing.last), (11..=20).collect::<Vec<_>>());
}

#[test]
fn manual_paginators_serialize_and_keep_link_boundaries() {
    let options = PaginatorOptions {
        path: Some("https://example.test/list///"),
        ..Default::default()
    };
    macro_rules! assert_json {
        ($page:expr) => {{
            let object = $page.to_array().unwrap();
            let compact = $page.to_json().unwrap();
            let pretty = $page.to_pretty_json().unwrap();
            assert_eq!(serde_json::from_str::<Json>(&compact).unwrap(), object);
            assert_eq!(serde_json::from_str::<Json>(&pretty).unwrap(), object);
            assert!(!compact.contains('\n') && pretty.contains("\n  "));
        }};
    }
    let simple = Paginator::new(vec![1, 2], 1, None, options).unwrap();
    assert_eq!(simple.path(), "https://example.test/list");
    assert_eq!(
        simple.next_page_url().as_deref(),
        Some("https://example.test/list?page=2")
    );
    assert_json!(simple);

    let mut length = LengthAwarePaginator::new(vec![1, 2, 3, 4], 4, 0, None, options).unwrap();
    assert_eq!(length.per_page(), 0);
    assert_eq!(length.last_page(), 4);
    assert_eq!(length.items(), [1, 2, 3, 4]);
    length
        .set_path("https://example.test?key=value%20with%20spaces")
        .set_page_name("foo");
    assert_eq!(
        length.url(2),
        "https://example.test?key=value%20with%20spaces&foo=2"
    );
    assert_json!(length);

    let empty = LengthAwarePaginator::<i32>::new(vec![], 0, 2, None, options).unwrap();
    assert_eq!(empty.last_page(), 1);
    assert!(
        empty.is_empty() && empty.on_first_page() && empty.on_last_page() && !empty.has_pages()
    );

    let cursor = CursorPaginator::new(vec![1, 2], 1, None, options, |id| {
        [("id", Value::Integer(*id))]
    })
    .unwrap();
    assert!(cursor.on_first_page() && !cursor.on_last_page());
    assert_eq!(
        cursor.next_cursor().unwrap().parameter("id").unwrap(),
        Value::Integer(1)
    );
    assert_json!(cursor);

    let previous = Cursor::new([("id", Value::Integer(1))], false).unwrap();
    let empty = CursorPaginator::new(Vec::<i64>::new(), 2, Some(&previous), options, |id| {
        [("id", Value::Integer(*id))]
    })
    .unwrap();
    assert!(empty.next_page_url().unwrap().is_none());
    assert!(empty.previous_page_url().unwrap().is_none());
    assert!(empty.is_empty() && empty.on_first_page());
}

fn custom_pagination_resolvers() -> ViewResult<()> {
    use openrust::view::{Factory, FileViewFinder};
    // 解析回调中重新登记其他回调，验证未持有状态锁执行应用代码。
    LengthAwarePaginator::current_page_resolver(|name| if name == "entries" { 2 } else { 3 })?;
    LengthAwarePaginator::current_path_resolver(|| "/configured".to_owned())?;
    Paginator::query_string_resolver(|| {
        Paginator::current_path_resolver(|| "/configured".to_owned()).unwrap();
        vec![
            ("filter".into(), "resolved".into()),
            ("entries".into(), "99".into()),
        ]
    })?;
    CursorPaginator::current_cursor_resolver(|name| {
        (name == "position").then(|| Cursor::new([("id", Value::Integer(1))], true).unwrap())
    })?;
    let root = Arc::new(tempfile::tempdir().unwrap());
    std::fs::write(
        root.path().join("numbered.blade.html"),
        "resolved {{ $paginator->total }}",
    )
    .unwrap();
    let factory = Arc::new(Factory::new(
        FileViewFinder::new([root.path().to_owned()]),
        root.path().join("compiled"),
    ));
    Paginator::default_view("numbered")?;
    LengthAwarePaginator::view_factory_resolver(move || {
        let _keep_root = &root;
        // 游标和偏移分页必须取得同一工厂，不能分别保存一份解析器。
        Ok(factory.clone())
    })
}

fn resolver_view(_: &mut Request) -> HttpFuture<'_, Result<openrust::view::View, HttpError>> {
    Box::pin(async {
        LengthAwarePaginator::new(vec![1], 3, 1, Some(1), PaginatorOptions::default())
            .map_err(manual_error)?
            .links()
            .map_err(Into::into)
    })
}

fn state_resolvers(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let connection = Db::connection(None)?;
        let mut numbered = connection
            .table("pagination_articles")
            .order_by("id", "asc")
            .paginate_with(PageOptions {
                per_page: Some(1),
                page_name: "entries",
                ..Default::default()
            })
            .await?;
        numbered
            .appends(&[("filter".into(), "manual".into())])
            .with_query_string();
        let simple = connection
            .table("pagination_articles")
            .order_by("id", "asc")
            .simple_paginate_with(PageOptions {
                per_page: Some(1),
                page_name: "entries",
                ..Default::default()
            })
            .await?;
        let cursor = connection
            .table("pagination_articles")
            .order_by("id", "asc")
            .cursor_paginate_with(CursorOptions {
                per_page: Some(1),
                cursor_name: "position",
                ..Default::default()
            })
            .await?;
        let explicit = Paginator::new(
            vec![1, 2],
            1,
            Some(1),
            PaginatorOptions {
                path: Some("/explicit"),
                ..Default::default()
            },
        )
        .map_err(manual_error)?;
        let manual = LengthAwarePaginator::new(
            vec![1],
            3,
            1,
            None,
            PaginatorOptions {
                page_name: Some("entries"),
                ..Default::default()
            },
        )
        .map_err(manual_error)?;
        let versions = if Paginator::resolve_current_path("/") == "/configured" {
            use std::sync::atomic::{AtomicUsize, Ordering};
            let mut dynamic = Paginator::new(
                vec![1],
                1,
                Some(1),
                PaginatorOptions {
                    page_name: Some("entries"),
                    ..Default::default()
                },
            )
            .map_err(manual_error)?;
            dynamic.appends(&[("filter".into(), "resolved".into())]);
            let counter = AtomicUsize::new(0);
            LengthAwarePaginator::query_string_resolver(move || {
                vec![(
                    "version".into(),
                    (counter.fetch_add(1, Ordering::SeqCst) + 1).to_string(),
                )]
            })?;
            dynamic.with_query_string();
            let first = dynamic.url(1);
            dynamic.with_query_string();
            Some((first, dynamic.url(1)))
        } else {
            None
        };
        let factory_shared = Arc::ptr_eq(
            &Paginator::view_factory()?,
            &CursorPaginator::view_factory()?,
        );
        let mut view = numbered.links()?;
        let html = view.render()?;
        Response::json(
            StatusCode::OK,
            json!({
                "numbered": numbered.map(|row| id(row.get("id"))),
                "simple": simple.map(|row| id(row.get("id"))),
                "cursor": cursor.map(|row| id(row.get("id"))),
                "manual_page": manual.current_page(),
                "explicit_page": explicit.current_page(), "explicit_url": explicit.url(2),
                "factory_shared": factory_shared, "html": html, "versions": versions,
                "resolved_page": LengthAwarePaginator::resolve_current_page("entries", 7),
            }),
        )
    })
}

async fn verify_pagination_resolvers(config: Json) {
    let (_custom_root, custom, custom_server) =
        styled_server_with_config(custom_pagination_resolvers, config).await;
    let (_default_root, default, default_server) =
        styled_server(|| Paginator::default_view("numbered")).await;
    let (configured, ordinary) = tokio::join!(
        request(custom, "/api/state-resolvers?entries=1&filter=incoming"),
        request(default, "/api/state-resolvers?entries=1&filter=incoming")
    );
    let reference: Json =
        serde_json::from_str(include_str!("reference/pagination-resolvers.json")).unwrap();
    assert_eq!(configured["resolved_page"], reference["resolved_page"]);
    assert_eq!(
        configured["numbered"]["first_page_url"],
        reference["first_url"]
    );
    assert_eq!(configured["versions"][0], reference["version_one"]);
    assert_eq!(configured["versions"][1], reference["version_two"]);
    assert_eq!(
        configured["factory_shared"],
        reference["shared_view_factory"]
    );
    assert_eq!(configured["numbered"]["current_page"], 2);
    assert_eq!(configured["numbered"]["data"], json!([2]));
    assert_eq!(configured["simple"]["data"], json!([2]));
    assert_eq!(configured["cursor"]["data"], json!([2]));
    assert_eq!(configured["manual_page"], 2);
    assert_eq!(configured["resolved_page"], 2);
    assert_eq!(configured["explicit_page"], 1);
    assert_eq!(configured["explicit_url"], "/explicit?page=2");
    assert_eq!(
        configured["numbered"]["first_page_url"],
        "/configured?filter=resolved&entries=1"
    );
    assert_eq!(configured["html"], "resolved 3");
    assert_eq!(configured["factory_shared"], true);
    assert_eq!(ordinary["numbered"]["data"], json!([1]));
    assert_eq!(ordinary["cursor"]["data"], json!([1]));
    assert_eq!(ordinary["html"], "numbered 3");
    assert!(
        ordinary["numbered"]["first_page_url"]
            .as_str()
            .unwrap()
            .starts_with("/api/state-resolvers?filter=incoming")
    );
    assert_eq!(
        request_body(custom, "/api/resolver-view").await,
        "resolved 3"
    );
    assert_eq!(
        request_body(default, "/api/resolver-view").await,
        "numbered 3"
    );
    custom_server.abort();
    default_server.abort();
}

#[tokio::test]
async fn pagination_resolvers_drive_sqlite_queries_and_isolate_applications() {
    verify_pagination_resolvers(json!({"driver":"sqlite", "database":":memory:"})).await;
}

#[tokio::test]
async fn pagination_resolvers_drive_postgres_queries() {
    let url = std::env::var("OPENRUST_TEST_PAGINATION_POSTGRES_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_PAGINATION_POSTGRES").is_none());
    if let Some(url) = url {
        verify_pagination_resolvers(json!({"url":url})).await;
    }
}

#[tokio::test]
async fn pagination_resolvers_drive_mysql_queries() {
    let url = std::env::var("OPENRUST_TEST_PAGINATION_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_PAGINATION_MYSQL").is_none());
    if let Some(url) = url {
        verify_pagination_resolvers(json!({"url":url})).await;
    }
}

#[test]
fn pagination_resolvers_preserve_unscoped_defaults() {
    assert_eq!(Paginator::resolve_current_path("/fallback"), "/fallback");
    assert_eq!(LengthAwarePaginator::resolve_current_page("page", 7), 7);
    assert_eq!(
        Paginator::resolve_query_string(Some(vec![("a".into(), "b".into())])),
        Some(vec![("a".into(), "b".into())])
    );
    let cursor = Cursor::new([("id", Value::Integer(8))], true).unwrap();
    assert_eq!(
        CursorPaginator::resolve_current_cursor("position", Some(cursor.clone())),
        Some(cursor)
    );
    assert!(Paginator::current_page_resolver(|_| 2).is_err());
}

#[tokio::test]
async fn pagination_view_factory_errors_reach_the_http_response() {
    let (_root, address, server) =
        styled_server(|| Paginator::view_factory_resolver(|| Err(ViewError::NotFound))).await;
    request_with_status(address, "/api/resolver-view", 500).await;
    server.abort();
}

#[tokio::test]
async fn explicit_paginator_options_do_not_invoke_position_or_path_resolvers() {
    let (_root, _, server) = styled_server(|| {
        Paginator::current_path_resolver(|| panic!("显式路径不应调用解析器"))?;
        Paginator::current_page_resolver(|_| panic!("显式页码不应调用解析器"))?;
        CursorPaginator::current_cursor_resolver(|_| panic!("显式游标不应调用解析器"))?;
        let options = PaginatorOptions {
            path: Some("/explicit"),
            ..Default::default()
        };
        assert_eq!(
            Paginator::new(vec![1], 1, Some(2), options)
                .unwrap()
                .current_page(),
            2
        );
        assert_eq!(
            LengthAwarePaginator::new(vec![1], 3, 1, Some(2), options)
                .unwrap()
                .url(3),
            "/explicit?page=3"
        );
        let cursor = Cursor::new([("id", Value::Integer(1))], true).unwrap();
        let page = CursorPaginator::new(vec![2_i64], 1, Some(&cursor), options, |item| {
            [("id", Value::Integer(*item))]
        })
        .unwrap();
        assert_eq!(page.cursor(), Some(&cursor));
        Ok(())
    })
    .await;
    server.abort();
}

fn resolver_input(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Response::json(
            StatusCode::OK,
            Paginator::resolve_current_page("entries", 7),
        )
    })
}

fn resolver_query(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut page = Paginator::new(
            vec![1],
            1,
            Some(1),
            PaginatorOptions {
                path: Some("/items"),
                page_name: Some("entries"),
                ..Default::default()
            },
        )
        .map_err(manual_error)?;
        page.appends(&[("filter".into(), "manual".into())])
            .with_query_string();
        Response::json(
            StatusCode::OK,
            json!({"query": LengthAwarePaginator::resolve_query_string(Some(vec![("fallback".into(), "yes".into())])), "url":page.url(1)}),
        )
    })
}

#[tokio::test]
async fn pagination_default_page_input_matches_the_fixed_filter_rules() {
    let reference: Json =
        serde_json::from_str(include_str!("reference/pagination-resolvers.json")).unwrap();
    let (_root, address, server) = styled_server(|| Ok(())).await;
    assert_eq!(request(address, "/api/resolver-input").await, json!(1));
    for case in reference["page_inputs"].as_array().unwrap() {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("entries", case["input"].as_str().unwrap())
            .finish();
        assert_eq!(
            request(address, &format!("/api/resolver-input?{query}")).await,
            case["page"],
            "{case}"
        );
    }
    server.abort();
}

#[tokio::test]
async fn pagination_null_and_empty_query_callbacks_suppress_request_and_fallback_sources() {
    let (_null_root, null, null_server) =
        styled_server(|| Paginator::query_string_resolver(|| None)).await;
    let (_empty_root, empty, empty_server) =
        styled_server(|| LengthAwarePaginator::query_string_resolver(Vec::<(String, String)>::new))
            .await;
    let (_default_root, ordinary, ordinary_server) = styled_server(|| Ok(())).await;
    let (null, empty, ordinary) = tokio::join!(
        request(null, "/api/resolver-query?filter=incoming"),
        request(empty, "/api/resolver-query?filter=incoming"),
        request(ordinary, "/api/resolver-query?filter=incoming")
    );
    let reference: Json =
        serde_json::from_str(include_str!("reference/pagination-resolvers.json")).unwrap();
    assert_eq!(null["query"], reference["null_query"]);
    assert_eq!(empty["query"], reference["empty_query"]);
    assert_eq!(null["url"], "/items?filter=manual&entries=1");
    assert_eq!(empty["url"], null["url"]);
    assert_eq!(ordinary["url"], "/items?filter=incoming&entries=1");
    for server in [null_server, empty_server, ordinary_server] {
        server.abort();
    }
}
