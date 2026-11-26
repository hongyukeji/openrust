use std::{collections::BTreeMap, sync::Arc};

use openrust::database::eloquent::RelatedCollection;
use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Builder, Collection, DynamicRelation, EagerLoads, MissingRelation, Model,
            ModelMetadata, ModelState, NamedRelations, RelationAggregate, RelationAggregateQuery,
            Touches,
            casts::{AttributeCast, CastableEnum, EnumCast},
            relations::{
                BelongsTo, BelongsToMany, HasMany, HasManyThrough, HasOne, HasOneThrough,
                MorphMany, MorphOne, MorphPivot, MorphTarget, MorphTo, MorphToMany,
                OneOfManyAggregate, Pivot, RelatedValues, Relation,
            },
        },
    },
};
use serde_json::{Value as Json, json};

struct Author(ModelState);

struct StrictAuthor(ModelState);

impl Model for StrictAuthor {
    fn metadata() -> &'static ModelMetadata {
        Author::metadata()
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

    fn prevents_lazy_loading(&self) -> bool {
        true
    }
}

struct AuthorWithDefaults(ModelState);

impl Model for AuthorWithDefaults {
    fn named_relations() -> NamedRelations<Self> {
        NamedRelations::default().nested("posts", || HasMany::<Self, Post>::new("id", "author_id"))
    }

    fn automatically_eager_load_relationships() -> bool {
        true
    }

    fn prevents_lazy_loading(&self) -> bool {
        true
    }

    fn handle_lazy_loading_violation_using(&self, violation: DatabaseError) -> DatabaseResult<()> {
        assert!(
            matches!(violation, DatabaseError::LazyLoadingViolation { relation, .. } if relation == "posts")
        );
        Ok(())
    }

    fn metadata() -> &'static ModelMetadata {
        Author::metadata()
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

    fn with_default_relations(relations: EagerLoads<Self>) -> EagerLoads<Self> {
        relations
            .with("posts", HasMany::<Self, Post>::new("id", "author_id"))
            .with("mentees", HasMany::<Self, Author>::new("id", "mentor_id"))
    }

    fn with_default_counts(
        query: Builder<'static, Self>,
    ) -> openrust::database::DatabaseResult<Builder<'static, Self>> {
        query.with_count("posts", &HasMany::<Self, Post>::new("id", "author_id"))
    }
}

impl Model for Author {
    fn named_relations() -> NamedRelations<Self> {
        NamedRelations::default()
            .nested("posts", || POSTS)
            .nested("details", || POSTS)
            .nested("featured", || FEATURED_POSTS)
            .nested("commentsThrough", || COMMENTS_THROUGH)
    }

    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_authors")
            .fillable(&["name"])
            .timestamps(None, None);
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
}

struct Post(ModelState);

impl Model for Post {
    fn named_relations() -> NamedRelations<Self> {
        NamedRelations::default()
            .nested("comments", || POST_COMMENTS)
            .nested("details", || POST_COMMENTS)
            .nested("author", || AUTHOR)
    }

    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_posts")
            .fillable(&["title", "score"])
            .timestamps(None, None)
            .soft_deletes("deleted_at");
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
}

struct Comment(ModelState);

impl Model for Comment {
    fn named_relations() -> NamedRelations<Self> {
        NamedRelations::default()
            .nested("post", || BelongsTo::<Self, Post>::new("post_id", "id"))
            .morph_to("subject", || {
                MorphTo::<Self>::new("subject_type", "subject_id")
                    .target::<Author>("author", "id")
                    .target::<Post>("post", "id")
            })
    }

    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_comments")
            .fillable(&["body"])
            .timestamps(None, None)
            .soft_deletes("deleted_at");
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
}

const COMMENTS_THROUGH: HasManyThrough<Author, Post, Comment> =
    HasManyThrough::new("author_id", "post_id", "id", "id");
const FIRST_COMMENT_THROUGH: HasOneThrough<Author, Post, Comment> =
    HasOneThrough::new("author_id", "post_id", "id", "id");
const POST_COMMENTS: MorphMany<Post, Comment> =
    MorphMany::new("id", "subject_id", "subject_type", "post");
const AUTHOR_COMMENT: MorphOne<Author, Comment> =
    MorphOne::new("id", "subject_id", "subject_type", "author");

const POSTS: HasMany<Author, Post> = HasMany::new("id", "author_id");
const AUTHOR: BelongsTo<Post, Author> = BelongsTo::new("author_id", "id");
const FIRST_POST: HasOne<Author, Post> = HasOne::new("id", "author_id");
const MENTEES: HasMany<Author, Author> = HasMany::new("id", "mentor_id");
const FEATURED_POSTS: BelongsToMany<Author, Post> = BelongsToMany::new(
    "relation_featured_posts",
    "author_id",
    "post_id",
    "id",
    "id",
);

fn connect(config: Json) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"relations","connections":{"relations":config}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

async fn select_count(db: &Connection) -> u64 {
    let row = db
        .select_one("SHOW SESSION STATUS LIKE 'Com_select'", &[])
        .await
        .unwrap()
        .unwrap();
    let Some(Value::Text(count)) = row.get_at(1) else {
        panic!("invalid MySQL counter");
    };
    count.parse().unwrap()
}

async fn verify_singular_identity(
    db: &Connection,
    other: &Connection,
    mysql: bool,
    author: &Author,
    post: &Post,
    hidden: &Post,
) {
    let baseline = if mysql { select_count(db).await } else { 0 };
    assert!(AUTHOR.is(post, Some(author)));
    assert!(AUTHOR.is_not(post, None));
    assert!(FIRST_POST.is(author, Some(post)).await.unwrap());

    let mut child = Post::new_instance(Attributes::new()).unwrap();
    let mut owner = Author::new_instance(Attributes::new()).unwrap();
    child.set_attribute("owner_code", "+00042");
    owner.set_attribute("code", 42_i64);
    let by_code = BelongsTo::new("owner_code", "code");
    assert!(by_code.is(&child, Some(&owner)));
    child.set_attribute("owner_code", "42other");
    assert!(by_code.is_not(&child, Some(&owner)));
    child.set_attribute("owner_code", "0");
    owner.set_attribute("code", 0_i64);
    assert!(by_code.is_not(&child, Some(&owner)));
    let mut same_name_owner = Author::new_instance_on(other, Attributes::new()).unwrap();
    same_name_owner.set_attribute("id", author.get_key().unwrap().clone());
    assert!(AUTHOR.is_not(post, Some(&same_name_owner)));

    let mut comment = Comment::new_instance(Attributes::new()).unwrap();
    comment.set_attribute("subject_id", author.get_key().unwrap().clone());
    comment.set_attribute("subject_type", "author");
    let subject = MorphTo::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id");
    let mut detached = Author::new_instance(Attributes::new()).unwrap();
    detached.set_attribute("id", author.get_key().unwrap().clone());
    assert!(subject.is(&comment, Some(&detached)));
    assert!(subject.is_not(&comment, Some(post)));
    comment.set_attribute("subject_type", "unknown");
    assert!(subject.is_not(&comment, Some(&detached)));
    if mysql {
        assert_eq!(select_count(db).await - baseline, 0);
    }

    let mut newest = POSTS
        .create(
            author,
            Attributes::from([("title".into(), "Newest identity".into())]),
        )
        .await
        .unwrap();
    let latest = POSTS.one().latest_of_many(None);
    assert!(latest.is(author, Some(&newest)).await.unwrap());
    assert!(latest.is_not(author, Some(post)).await.unwrap());
    assert!(latest.is_not(author, Some(hidden)).await.unwrap());
    newest.force_delete().await.unwrap();
}

async fn verify_through_and_morph(
    db: &Connection,
    other: &Connection,
    mysql: bool,
    author: &Author,
    post: &Post,
    hidden: &Post,
) {
    let mut visible = POST_COMMENTS
        .create(post, Attributes::from([("body".into(), "visible".into())]))
        .await
        .unwrap();
    visible.set_attribute("post_id", post.get_key().unwrap().clone());
    visible.save().await.unwrap();
    let mut hidden_comment = POST_COMMENTS
        .create(
            hidden,
            Attributes::from([("body".into(), "hidden parent".into())]),
        )
        .await
        .unwrap();
    hidden_comment.set_attribute("post_id", hidden.get_key().unwrap().clone());
    hidden_comment.save().await.unwrap();
    let mut deleted = POST_COMMENTS
        .create(post, Attributes::from([("body".into(), "deleted".into())]))
        .await
        .unwrap();
    deleted.set_attribute("post_id", post.get_key().unwrap().clone());
    deleted.save().await.unwrap();
    deleted.delete().await.unwrap();
    let mut profile = AUTHOR_COMMENT
        .create(
            author,
            Attributes::from([("body".into(), "profile".into())]),
        )
        .await
        .unwrap();
    assert_eq!(
        profile.get_attribute("subject_id"),
        visible.get_attribute("subject_id")
    );
    let comments = Author::through(POSTS).has(HasMany::<Post, Comment>::new("id", "post_id"));
    let found = comments.get_results(author).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].get_key(), visible.get_key());
    assert_eq!(
        comments
            .with_trashed_parents()
            .get_results(author)
            .await
            .unwrap()
            .len(),
        2,
    );
    let comment = Author::through(FIRST_POST).has(HasOne::<Post, Comment>::new("id", "post_id"));
    assert_eq!(
        comment
            .get_results(author)
            .await
            .unwrap()
            .unwrap()
            .get_key(),
        visible.get_key()
    );
    let owner =
        Author::through(AUTHOR_COMMENT).has(HasMany::<Comment, Author>::new("subject_id", "id"));
    let found = owner.get_results(author).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].get_key(), author.get_key());
    let found = Author::query_on(db)
        .has(&owner)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(found.model_keys(), vec![author.get_key().unwrap().clone()]);
    assert!(AUTHOR_COMMENT.is(author, Some(&profile)).await.unwrap());
    assert!(
        AUTHOR_COMMENT
            .latest_of_many(None)
            .is_not(author, Some(&visible))
            .await
            .unwrap()
    );
    let title = post.get_attribute("title").unwrap().clone();
    let mut custom_child = POST_COMMENTS
        .create(post, Attributes::from([("body".into(), title.clone())]))
        .await
        .unwrap();
    let mut custom_parent = Author::from_state(author.state().clone());
    custom_parent.set_attribute("name", title);
    let custom_keys =
        || HasManyThrough::<Author, Post, Comment>::new("title", "body", "name", "title");
    let direct = custom_keys()
        .one()
        .get_results(&custom_parent)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(direct.get_key(), custom_child.get_key());
    let matched = custom_keys()
        .match_results(&[&custom_parent, author])
        .await
        .unwrap();
    assert_eq!(matched[0].len(), 1);
    assert_eq!(matched[0][0].get_key(), custom_child.get_key());
    assert!(matched[1].is_empty());
    custom_child.force_delete().await.unwrap();

    let mut newer = POST_COMMENTS
        .create(post, Attributes::from([("body".into(), "visible".into())]))
        .await
        .unwrap();
    let columns = ["body", "id"];
    let oldest = POST_COMMENTS.one().oldest_of_many(columns.as_slice());
    assert_eq!(
        oldest.get_results(post).await.unwrap().unwrap().get_key(),
        visible.get_key()
    );
    let latest = POST_COMMENTS.one().latest_of_many(&columns);
    assert_eq!(
        latest.get_results(post).await.unwrap().unwrap().get_key(),
        newer.get_key()
    );
    newer.force_delete().await.unwrap();
    assert_eq!(POST_COMMENTS.get_results(post).await.unwrap().len(), 1);
    assert_eq!(
        POST_COMMENTS
            .one()
            .latest_of_many(None)
            .get_results(post)
            .await
            .unwrap()
            .unwrap()
            .get_key(),
        visible.get_key(),
    );
    assert_eq!(
        AUTHOR_COMMENT
            .get_results(author)
            .await
            .unwrap()
            .unwrap()
            .get_key(),
        profile.get_key()
    );
    assert_eq!(
        FIRST_COMMENT_THROUGH
            .get_results(author)
            .await
            .unwrap()
            .unwrap()
            .get_key(),
        visible.get_key()
    );

    let baseline = if mysql { select_count(db).await } else { 0 };
    let authors = Author::query_on(db)
        .with(
            "recentComments",
            COMMENTS_THROUGH.constrain(|query| query.with_casts([("id", AttributeCast::String)])),
        )
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }
    assert!(matches!(
        authors.all()[0]
            .get_relation::<RelatedCollection<Comment>>("recentComments")
            .unwrap()[0]
            .get_attribute_value("id")
            .unwrap(),
        Some(Value::Text(_))
    ));
    assert_eq!(
        authors
            .iter()
            .map(|model| model
                .get_relation::<RelatedCollection<Comment>>("recentComments")
                .unwrap()
                .len())
            .collect::<Vec<_>>(),
        vec![1, 0, 0]
    );
    let before_projection = if mysql { select_count(db).await } else { 0 };
    {
        let json: Json = serde_json::from_str(&authors.to_json().unwrap()).unwrap();
        assert_eq!(json[0]["recent_comments"][0]["body"], "visible");
        assert_eq!(json[1]["recent_comments"], json!([]));
        assert!(
            !authors.all()[0]
                .attributes_to_array()
                .unwrap()
                .contains_key("recent_comments")
        );
        let mut first = authors.into_iter().next().unwrap();
        first.make_hidden(&["recentComments"]);
        assert!(!first.to_array().unwrap().contains_key("recent_comments"));
    }
    if mysql {
        assert_eq!(select_count(db).await, before_projection);
    }
    assert_eq!(
        COMMENTS_THROUGH
            .with_trashed_parents()
            .get_results(author)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        Author::query_on(db)
            .has(&COMMENTS_THROUGH)
            .unwrap()
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![1.into()]
    );

    let subjects = || {
        MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Author>("author", "id")
            .target::<Post>("post", "id")
    };
    let baseline = if mysql { select_count(db).await } else { 0 };
    let comments = Comment::query_on(db)
        .with("subject", subjects())
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 3);
    }
    let mut comments = comments.into_iter();
    let first = comments.next().unwrap();
    let target = first
        .get_relation::<Option<MorphTarget>>("subject")
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(target.alias(), "post");
    assert!(first.to_array().unwrap()["subject"].is_object());
    assert_eq!(
        target.downcast_ref::<Post>().unwrap().get_key(),
        post.get_key()
    );
    let second = comments.next().unwrap();
    assert_eq!(second.to_array().unwrap()["subject"], Json::Null);
    assert!(
        second
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap()
            .is_none()
    );
    let third = comments.next().unwrap();
    assert_eq!(
        third
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap()
            .as_ref()
            .unwrap()
            .downcast_ref::<Author>()
            .unwrap()
            .get_key(),
        author.get_key()
    );

    Box::pin(verify_morph_soft_deletes(
        db,
        &visible,
        &hidden_comment,
        &profile,
    ))
    .await;
    Box::pin(verify_morph_defaults(db, &visible, &hidden_comment)).await;
    let subject = subjects();
    let valid = Comment::query_on(db)
        .where_has_morph(&subject, &["post", "author"])
        .unwrap()
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(
        valid.model_keys(),
        vec![
            visible.get_key().unwrap().clone(),
            profile.get_key().unwrap().clone()
        ]
    );
    let missing = Comment::query_on(db)
        .where_doesnt_have_morph(&subject, &["post"])
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(
        missing.model_keys(),
        vec![hidden_comment.get_key().unwrap().clone()]
    );
    assert!(
        Comment::query_on(db)
            .where_has_morph(&subject, &["unknown"])
            .is_err()
    );
    let missing = Comment::query_on(db)
        .has_morph_count(&subject, &["post"], "=", 0)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(
        missing.model_keys(),
        vec![hidden_comment.get_key().unwrap().clone()]
    );
    let counted_subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id")
        .morph_with_paths::<Post>("post", &["author"])
        .unwrap()
        .morph_with_count::<Post>("post", "id", "comments", POST_COMMENTS)
        .morph_with::<Post>("post", |loads| loads.with("comments", POST_COMMENTS))
        .unwrap()
        .constrain::<Post>("post", "id", |mut query| {
            query.r#where("score", ">", 0);
            query
        });
    let counted = Comment::query_on(db)
        .with("subject", counted_subjects)
        .find_or_fail(visible.get_key().unwrap().clone())
        .await
        .unwrap();
    let counted_post = counted
        .get_relation::<Option<MorphTarget>>("subject")
        .unwrap()
        .as_ref()
        .unwrap()
        .downcast_ref::<Post>()
        .unwrap();
    assert_eq!(
        counted_post.get_attribute("comments_count"),
        Some(&1.into())
    );
    assert!(counted_post.relation_loaded("author"));
    assert!(counted_post.relation_loaded("comments"));
    assert!(matches!(
        MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Post>("post", "id")
            .morph_with_paths::<Author>("post", &["posts"]),
        Err(DatabaseError::InvalidQuery(
            "morph type does not match model"
        ))
    ));
    Box::pin(verify_morph_with_constraint(db, visible.get_key().unwrap())).await;

    Box::pin(verify_morph_association(
        db,
        other,
        mysql,
        subject.named("subject"),
        &mut profile,
        author,
        post,
    ))
    .await;
    let subject = subjects();
    profile
        .set_attribute("subject_type", "missing")
        .set_attribute("subject_id", 1);
    assert!(subject.get_results(&profile).await.is_err());
    let post_tags = MorphToMany::<Post, Author>::new(
        "relation_subject_tags",
        "subject_id",
        "tag_id",
        "id",
        "id",
        "subject_type",
        "post",
    );
    let author_tags = MorphToMany::<Author, Author>::new(
        "relation_subject_tags",
        "subject_id",
        "tag_id",
        "id",
        "id",
        "subject_type",
        "author",
    );
    let tagged_posts = MorphToMany::<Author, Post>::new(
        "relation_subject_tags",
        "tag_id",
        "subject_id",
        "id",
        "id",
        "subject_type",
        "post",
    );
    let key = author.get_key().unwrap().clone();
    post_tags
        .attach_with(
            post,
            std::slice::from_ref(&key),
            Attributes::from([("label".into(), "post link".into())]),
        )
        .await
        .unwrap();
    author_tags
        .attach(author, std::slice::from_ref(&key))
        .await
        .unwrap();
    assert_eq!(tagged_posts.get_results(author).await.unwrap().len(), 1);
    assert_eq!(
        Author::query_on(db)
            .has(&author_tags)
            .unwrap()
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![1.into()]
    );
    assert!(
        post_tags
            .update_existing_pivot(
                post,
                &key,
                Attributes::from([("subject_type".into(), "author".into())])
            )
            .await
            .is_err()
    );
    assert_eq!(
        Post::query_on(db)
            .where_attached_to(&post_tags, std::slice::from_ref(author))
            .unwrap()
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![1.into()]
    );
    let mut tagged = Post::query_on(db);
    tagged.r#where("id", "=", 2).or_where("id", "=", 3);
    assert_eq!(
        tagged
            .or_where_attached_to(&post_tags, std::slice::from_ref(author))
            .unwrap()
            .order_by("id", "asc")
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![1.into(), 2.into()]
    );
    let tags = post_tags
        .with_timestamp_columns(None, Some("linked_at"))
        .unwrap()
        .order_by_pivot_desc("linked_at")
        .with_pivot(["label"])
        .get_results(post)
        .await
        .unwrap();
    let pivot = tags[0].get_relation::<MorphPivot>("pivot").unwrap();
    assert_eq!(pivot.get_attribute("subject_type"), Some(&"post".into()));
    assert_eq!(pivot.get_attribute("label"), Some(&"post link".into()));
    assert_eq!(pivot.get_attribute("linked_at"), Some(&Value::Null));
    let post_tags = MorphToMany::<Post, Author>::new(
        "relation_subject_tags",
        "subject_id",
        "tag_id",
        "id",
        "id",
        "subject_type",
        "post",
    );
    let post_tags = post_tags
        .where_pivot_with(|conditions| conditions.r#where("label", "=", "post link"))
        .or_where_pivot_with(|conditions| conditions.where_null("label"));
    assert_eq!(post_tags.sync(post, &[]).await.unwrap().detached, vec![key]);
    assert!(tagged_posts.get_results(author).await.unwrap().is_empty());
    assert_eq!(author_tags.get_results(author).await.unwrap().len(), 1);
}

fn verify_relation_identity(db: &Connection) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let author = Author::query_on(db).find_or_fail(1).await.unwrap();
        let post = Post::query_on(db).find_or_fail(1).await.unwrap();
        let other = Author::query_on(db).find_or_fail(3).await.unwrap();
        let relation = MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Author>("author", "id")
            .target::<Post>("post", "id");
        for (body, kind, key) in [
            ("identity author", "author".into(), 1.into()),
            ("identity post", "post".into(), 1.into()),
            ("identity other", "post".into(), 2.into()),
            ("identity blank", Value::Null, Value::Null),
            ("identity null key", "post".into(), Value::Null),
            ("First", "author".into(), Value::Null),
        ] {
            db.insert(
                "INSERT INTO relation_comments (body, subject_type, subject_id) VALUES (?, ?, ?)",
                &[body.into(), kind, key],
            )
            .await
            .unwrap();
        }
        let scoped = || {
            let mut query = Comment::query_on(db);
            query.r#where("body", "like", "identity %");
            query.order_by("body", "asc")
        };
        let post_id = relation.identity("post", &post).unwrap();
        let author_id = relation.identity("author", &author).unwrap();
        assert!(relation.identity("author", &post).is_err());
        let found = scoped()
            .where_morphed_to(&relation, &[post_id, author_id])
            .unwrap()
            .get()
            .await
            .unwrap();
        let bodies = |models: &Collection<Comment>| {
            models
                .iter()
                .map(|model| model.get_attribute("body").cloned().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            bodies(&found),
            vec!["identity author".into(), "identity post".into()]
        );
        let negated = scoped()
            .where_not_morphed_to(&relation, &[relation.identity("post", &post).unwrap()])
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(
            bodies(&negated),
            vec![
                "identity author".into(),
                "identity blank".into(),
                "identity other".into()
            ]
        );
        let unset = scoped()
            .where_morph_type(&relation, None)
            .get()
            .await
            .unwrap();
        assert_eq!(bodies(&unset), vec!["identity blank".into()]);
        assert!(scoped().where_morphed_to(&relation, &[]).is_err());

        let custom =
            MorphTo::<Comment>::new("subject_type", "body").target::<Author>("author", "name");
        let id = custom.identity("author", &author).unwrap();
        assert!(
            scoped()
                .where_morphed_to(&relation, std::slice::from_ref(&id))
                .is_err()
        );
        let matched = Comment::query_on(db)
            .where_morphed_to(&custom, &[id])
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(bodies(&matched), vec!["First".into()]);

        let mut disjunction = Comment::query_on(db);
        disjunction.r#where("body", "=", "identity blank");
        let found = disjunction
            .or_where_morphed_to(&relation, &[relation.identity("author", &author).unwrap()])
            .unwrap()
            .order_by("body", "asc")
            .get()
            .await
            .unwrap();
        assert_eq!(
            bodies(&found),
            vec![
                "author profile".into(),
                "identity author".into(),
                "identity blank".into()
            ]
        );

        let mut disjunction = Comment::query_on(db);
        disjunction.r#where("body", "=", "identity post");
        let found = disjunction
            .or_where_not_morphed_to(&relation, &[relation.identity("post", &post).unwrap()])
            .unwrap()
            .get()
            .await
            .unwrap();
        assert!(bodies(&found).contains(&"identity post".into()));
        assert!(bodies(&found).contains(&"identity author".into()));
        assert!(!bodies(&found).contains(&"identity null key".into()));

        let by_name = BelongsTo::<Comment, Author>::new("body", "name");
        let matched = Comment::query_on(db)
            .where_belongs_to(&by_name, std::slice::from_ref(&author))
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(bodies(&matched), vec!["First".into()]);
        let mut disjunction = Comment::query_on(db);
        disjunction.r#where("body", "=", "identity blank");
        let found = disjunction
            .or_where_belongs_to(&by_name, &[author])
            .unwrap()
            .order_by("body", "asc")
            .get()
            .await
            .unwrap();
        assert_eq!(
            bodies(&found),
            vec!["First".into(), "identity blank".into()]
        );
        FEATURED_POSTS.attach(&other, &[1.into()]).await.unwrap();
        let matched = Author::query_on(db)
            .where_attached_to(&FEATURED_POSTS, &[post])
            .unwrap()
            .get()
            .await
            .unwrap();
        assert!(matched.model_keys().contains(&3.into()));
        FEATURED_POSTS.detach(&other, None).await.unwrap();
    })
}

async fn verify_belongs_to_cache(
    db: &Connection,
    mysql: bool,
    post: &Post,
    first: &Author,
    second: &Author,
) {
    let relation = AUTHOR.named("author");
    let mut model = Post::from_state(post.state().clone());
    model.load("author", &relation).await.unwrap();
    model.load("comments", &POST_COMMENTS).await.unwrap();
    let mut edited = Author::from_state(second.state().clone());
    edited.set_attribute("name", "Local owner");
    let baseline = if mysql { select_count(db).await } else { 0 };

    let unsaved = Author::new_instance(Attributes::new()).unwrap();
    assert!(matches!(
        relation.associate(&mut model, &unsaved),
        Err(DatabaseError::InvalidQuery("relationship owner key"))
    ));
    assert_eq!(model.get_attribute("author_id"), first.get_key());
    relation.associate(&mut model, &edited).unwrap();
    assert_eq!(model.get_attribute("author_id"), second.get_key());
    assert!(model.relation_loaded("comments"));
    assert_eq!(model.to_array().unwrap()["author"]["name"], "Local owner");
    assert!(
        model
            .get_relation_or_load("author", &relation)
            .await
            .unwrap()
            .is_some()
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 0);
    }

    relation.associate_key(&mut model, first.get_key().unwrap().clone());
    assert!(!model.relation_loaded("author"));
    assert!(model.relation_loaded("comments"));
    assert_eq!(
        model
            .get_relation_or_load("author", &relation)
            .await
            .unwrap()
            .as_ref()
            .unwrap()
            .get_key(),
        first.get_key()
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }

    relation.disassociate(&mut model);
    assert_eq!(model.get_attribute("author_id"), Some(&Value::Null));
    assert!(model.relation_loaded("author") && model.relation_loaded("comments"));
    assert!(
        model
            .get_relation_or_load("author", &relation)
            .await
            .unwrap()
            .is_none()
    );
    assert!(model.to_array().unwrap()["author"].is_null());
    assert_eq!(
        Post::query_on(db)
            .find_or_fail(post.get_key().unwrap().clone())
            .await
            .unwrap()
            .get_attribute("author_id"),
        first.get_key()
    );
}

async fn verify_belongs_to_owner_keys(
    db: &Connection,
    mysql: bool,
    post: &Post,
    first: &Author,
    second: &Author,
) {
    let first_key = first.get_key().unwrap();
    let second_key = second.get_key().unwrap();
    db.statement(
        "UPDATE relation_authors SET mentor_id = 0 WHERE id IN (?, ?)",
        &[first_key.clone(), second_key.clone()],
    )
    .await
    .unwrap();

    let mut zero = Post::from_state(post.state().clone());
    zero.set_attribute("score", 0);
    let owner = BelongsTo::<Post, Author>::new("score", "mentor_id")
        .constrain(|query| query.order_by("id", "asc"));
    let baseline = if mysql { select_count(db).await } else { 0 };
    assert_eq!(
        owner.get_results(&zero).await.unwrap().unwrap().get_key(),
        Some(first_key)
    );
    assert_eq!(
        zero.get_relation_or_load("owner", &owner)
            .await
            .unwrap()
            .as_ref()
            .unwrap()
            .get_key(),
        Some(first_key)
    );
    assert_eq!(
        zero.get_relation_or_load("owner", &owner)
            .await
            .unwrap()
            .as_ref()
            .unwrap()
            .get_key(),
        Some(first_key)
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }

    let baseline = if mysql { select_count(db).await } else { 0 };
    let eager = owner.match_results(&[&zero, &zero]).await.unwrap();
    assert_eq!(eager[0].as_ref().unwrap().get_key(), Some(second_key));
    assert!(Arc::ptr_eq(
        eager[0].as_ref().unwrap(),
        eager[1].as_ref().unwrap()
    ));
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }

    let fallback =
        BelongsTo::<Post, Author>::new("score", "mentor_id").with_default(|author, child| {
            author.set_attribute("name", child.get_attribute("title").unwrap().clone());
        });
    let mut missing = Post::from_state(post.state().clone());
    missing.set_attribute("score", Value::Null);
    let baseline = if mysql { select_count(db).await } else { 0 };
    let default = fallback.get_results(&missing).await.unwrap().unwrap();
    assert!(!default.state().exists());
    assert_eq!(default.get_attribute("name"), post.get_attribute("title"));
    if mysql {
        assert_eq!(select_count(db).await - baseline, 0);
    }
    missing.set_attribute("score", -1);
    assert!(
        !fallback
            .get_results(&missing)
            .await
            .unwrap()
            .unwrap()
            .state()
            .exists()
    );

    let mut typed = Post::query_on(db)
        .with_casts([("score", AttributeCast::Enum(EnumCast::of::<PostLinkKey>()))])
        .find_or_fail(post.get_key().unwrap().clone())
        .await
        .unwrap();
    typed.set_enum("score", PostLinkKey::Editor).unwrap();
    assert!(matches!(
        typed.get_enum("score").unwrap(),
        Some(PostLinkKey::Editor)
    ));
    let enum_owner = BelongsTo::<Post, Author>::new("score", "id");
    assert_eq!(
        enum_owner
            .get_results(&typed)
            .await
            .unwrap()
            .unwrap()
            .get_key(),
        Some(first_key)
    );
    assert_eq!(
        enum_owner.match_results(&[&typed]).await.unwrap()[0]
            .as_ref()
            .unwrap()
            .get_key(),
        Some(first_key)
    );

    let subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .named("subject")
        .constrain::<Author>("0", "mentor_id", |query| query.order_by("id", "asc"))
        .constrain::<Author>("author", "mentor_id", |query| query.order_by("id", "desc"));
    let mut numeric_alias = Comment::query_on(db).make(Attributes::new()).unwrap();
    numeric_alias.set_attribute("subject_type", "0");
    numeric_alias.set_attribute("subject_id", 0);
    let mut named_alias = Comment::from_state(numeric_alias.state().clone());
    named_alias.set_attribute("subject_type", "author");
    let mut empty_alias = Comment::from_state(numeric_alias.state().clone());
    empty_alias.set_attribute("subject_type", "");

    let baseline = if mysql { select_count(db).await } else { 0 };
    let owner = numeric_alias
        .get_relation_or_load("subject", &subjects)
        .await
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(owner.alias(), "0");
    assert_eq!(
        owner.downcast_ref::<Author>().unwrap().get_key(),
        Some(first_key)
    );
    let owner = subjects.get_results(&named_alias).await.unwrap().unwrap();
    assert_eq!(owner.alias(), "author");
    assert_eq!(
        owner.downcast_ref::<Author>().unwrap().get_key(),
        Some(second_key)
    );
    assert!(subjects.get_results(&empty_alias).await.unwrap().is_none());
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }

    let baseline = if mysql { select_count(db).await } else { 0 };
    let batch = subjects
        .match_results(&[&numeric_alias, &named_alias, &empty_alias])
        .await
        .unwrap();
    for ((loaded, alias), key) in batch
        .iter()
        .zip(["0", "author"])
        .zip([second_key, first_key])
    {
        let loaded = loaded.as_ref().unwrap();
        assert_eq!(loaded.alias(), alias);
        assert_eq!(
            loaded.downcast_ref::<Author>().unwrap().get_key(),
            Some(key)
        );
    }
    assert!(batch[2].is_none());
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }

    let mut record = Comment::query_on(db)
        .create(Attributes::from([(
            "body".into(),
            "owner key round trip".into(),
        )]))
        .await
        .unwrap();
    let selected = Author::query_on(db)
        .select(["id", "mentor_id"])
        .find_or_fail(first_key.clone())
        .await
        .unwrap();
    subjects.associate(&mut record, "0", &selected).unwrap();
    assert_eq!(record.get_attribute("subject_id"), Some(&0.into()));
    assert_eq!(record.get_attribute("subject_type"), Some(&"0".into()));
    record.save().await.unwrap();
    let mut stored = Comment::query_on(db)
        .find_or_fail(record.get_key().unwrap().clone())
        .await
        .unwrap();
    let loaded = stored
        .get_relation_or_load("subject", &subjects)
        .await
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(
        loaded.downcast_ref::<Author>().unwrap().get_key(),
        Some(first_key)
    );

    let projected = MorphTo::<Comment>::new("subject_type", "subject_id").constrain::<Author>(
        "0",
        "mentor_id",
        |q| q.select(["id"]).order_by("id", "asc"),
    );
    let read = projected.get_results(&stored).await.unwrap().unwrap();
    let read = read.downcast_ref::<Author>().unwrap();
    assert_eq!(read.get_key(), Some(first_key));
    assert!(read.get_attribute("mentor_id").is_none());
    assert!(projected.match_results(&[&stored]).await.unwrap()[0].is_none());

    let before = stored.to_array().unwrap();
    assert!(matches!(
        subjects.associate(&mut stored, "0", read),
        Err(DatabaseError::InvalidQuery("relationship owner key"))
    ));
    let mut invalid = Author::from_state(selected.state().clone());
    invalid.set_attribute("mentor_id", Value::Json(json!({ "id": 0 })));
    assert!(matches!(
        subjects.associate(&mut stored, "0", &invalid),
        Err(DatabaseError::InvalidQuery("relationship key type"))
    ));
    let untyped = MorphTo::<Comment>::new("subject_type", "subject_id")
        .named("subject")
        .target::<Author>("", "mentor_id");
    assert!(matches!(
        untyped.associate(&mut stored, "", &selected),
        Err(DatabaseError::InvalidQuery("empty morph type"))
    ));
    assert_eq!(stored.to_array().unwrap(), before);
    record.force_delete().await.unwrap();

    for author in [first, second] {
        db.statement(
            "UPDATE relation_authors SET mentor_id = ? WHERE id = ?",
            &[
                author
                    .get_attribute("mentor_id")
                    .unwrap_or(&Value::Null)
                    .clone(),
                author.get_key().unwrap().clone(),
            ],
        )
        .await
        .unwrap();
    }
}

async fn verify_morph_association(
    db: &Connection,
    other: &Connection,
    mysql: bool,
    subject: MorphTo<Comment>,
    profile: &mut Comment,
    author: &Author,
    post: &Post,
) {
    let ordinary = BelongsTo::<Comment, Post>::new("post_id", "id");
    profile.load("post", &ordinary).await.unwrap();
    profile.load("subject", &subject).await.unwrap();

    let mut snapshot = Post::from_state(post.state().clone());
    snapshot.set_attribute("title", "Unpersisted subject title");
    let baseline = if mysql { select_count(db).await } else { 0 };
    assert!(subject.associate(profile, "post", author).is_err());
    assert_eq!(
        profile
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap()
            .as_ref()
            .unwrap()
            .alias(),
        "author",
    );
    let mut foreign = Post::new_instance_on(other, Attributes::new()).unwrap();
    foreign.set_attribute("id", 1);
    assert!(matches!(
        subject.associate(profile, "post", &foreign),
        Err(DatabaseError::InvalidQuery(
            "mixed relationship connections"
        ))
    ));
    subject.associate(profile, "post", &snapshot).unwrap();
    assert!(profile.relation_loaded("post"));
    let target = profile
        .get_relation_or_load("subject", &subject)
        .await
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(target.alias(), "post");
    assert_eq!(
        target
            .downcast_ref::<Post>()
            .unwrap()
            .get_attribute("title"),
        Some(&"Unpersisted subject title".into())
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 0);
    }
    assert_eq!(
        profile.to_array().unwrap()["subject"]["title"],
        "Unpersisted subject title"
    );
    let key = profile.get_key().unwrap().clone();
    let stored = Comment::query_on(db)
        .find_or_fail(key.clone())
        .await
        .unwrap();
    assert_eq!(stored.get_attribute("subject_type"), Some(&"author".into()));

    profile.save().await.unwrap();
    assert_eq!(POST_COMMENTS.get_results(post).await.unwrap().len(), 2);
    subject.dissociate(profile);
    assert!(profile.relation_loaded("post"));
    assert!(profile.relation_loaded("subject"));
    assert!(
        profile
            .get_relation_or_load("subject", &subject)
            .await
            .unwrap()
            .is_none()
    );
    assert!(profile.to_array().unwrap()["subject"].is_null());
    profile.save().await.unwrap();
    let stored = Comment::query_on(db).find_or_fail(key).await.unwrap();
    assert_eq!(stored.get_attribute("subject_type"), Some(&Value::Null));
    assert_eq!(stored.get_attribute("subject_id"), Some(&Value::Null));
}

async fn verify_morph_defaults(db: &Connection, visible: &Comment, hidden: &Comment) {
    let defaults = || {
        MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Post>("post", "id")
            .target::<Author>("author", "id")
            .with_default::<Post>("post", |post, parent| {
                post.set_attribute("title", parent.get_attribute("body").unwrap().clone());
            })
            .unwrap()
            .with_default::<Author>("author", |author, _| {
                author.set_attribute("name", "Guest");
            })
            .unwrap()
    };

    let mut query = Comment::query_on(db).with("subject", defaults());
    query.where_in(
        "id",
        vec![
            visible.get_key().unwrap().clone(),
            hidden.get_key().unwrap().clone(),
        ],
    );
    let parents = Box::pin(query.get()).await.unwrap();
    for (parent, exists) in [(visible, true), (hidden, false)] {
        let row = parents.find(parent.get_key().unwrap()).unwrap().unwrap();
        let target = row
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap()
            .as_ref()
            .unwrap();
        let post = target.downcast_ref::<Post>().unwrap();
        assert_eq!(post.state().exists(), exists);
        if !exists {
            assert!(post.get_key().is_none());
            assert_eq!(post.get_connection_name(), parent.get_connection_name());
            assert_eq!(row.to_array().unwrap()["subject"]["title"], "hidden parent");
        }
    }
    let mut missing = Comment::from_state(hidden.state().clone());
    missing.set_attribute("subject_id", 999999);
    let result = defaults().get_results(&missing).await.unwrap().unwrap();
    assert!(!result.downcast_ref::<Post>().unwrap().state().exists());
    missing.set_attribute("subject_type", Value::Null);
    assert!(defaults().get_results(&missing).await.unwrap().is_none());
    let result = defaults()
        .default_for_missing_type("author")
        .unwrap()
        .get_results(&missing)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.alias(), "author");
    assert_eq!(
        result
            .downcast_ref::<Author>()
            .unwrap()
            .get_attribute("name"),
        Some(&"Guest".into())
    );
    missing.set_attribute("subject_type", "unknown");
    assert!(matches!(
        defaults().get_results(&missing).await,
        Err(DatabaseError::InvalidQuery("unknown morph type"))
    ));
}

async fn verify_morph_soft_deletes(
    db: &Connection,
    visible: &Comment,
    hidden: &Comment,
    profile: &Comment,
) {
    let morph = || {
        MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Post>("post", "id")
            .target::<Author>("author", "id")
    };
    for (relation, expected) in [
        (morph().with_trashed(), [true, true, true]),
        (morph().without_trashed(), [true, false, true]),
        (morph().only_trashed(), [false, true, true]),
        (morph().only_trashed().with_trashed(), [false, true, true]),
        (
            morph().without_trashed().only_trashed(),
            [false, false, true],
        ),
        (
            morph()
                .with_trashed()
                .constrain::<Post>("post", "id", Builder::without_trashed),
            [true, false, true],
        ),
        (
            morph()
                .only_trashed()
                .constrain::<Post>("post", "id", Builder::with_trashed),
            [false, true, true],
        ),
    ] {
        let mut query = Comment::query_on(db).with("subject", relation);
        let parents = [visible, hidden, profile];
        query.where_in(
            "id",
            parents
                .iter()
                .map(|model| model.get_key().unwrap().clone())
                .collect(),
        );
        let rows = Box::pin(query.get()).await.unwrap();
        for (parent, loaded) in parents.into_iter().zip(expected) {
            let row = rows.find(parent.get_key().unwrap()).unwrap().unwrap();
            let target = row.get_relation::<Option<MorphTarget>>("subject").unwrap();
            assert_eq!(target.is_some(), loaded);
        }
    }

    let target = morph()
        .with_trashed()
        .get_results(hidden)
        .await
        .unwrap()
        .unwrap();
    assert!(target.downcast_ref::<Post>().unwrap().trashed());
    assert!(
        morph()
            .without_trashed()
            .get_results(hidden)
            .await
            .unwrap()
            .is_none()
    );
}

async fn verify_morph_with_constraint(db: &Connection, key: &Value) {
    let morph = MorphTo::<Comment>::new("subject_type", "subject_id")
        .constrain::<Post>("post", "id", |mut query| {
            query.r#where("score", "<", 0);
            query
        })
        .morph_with_paths::<Post>("post", &["author"])
        .unwrap()
        .morph_with_count::<Post>("post", "id", "comments", POST_COMMENTS);
    let comment = Comment::query_on(db)
        .with("subject", morph)
        .find_or_fail(key.clone())
        .await
        .unwrap();
    assert!(
        comment
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap()
            .is_none()
    );
}

async fn verify_morph_relation_writeback(
    db: &Connection,
    mysql: bool,
    comments: &mut Collection<Comment>,
    subjects: &MorphTo<Comment>,
) {
    let subject = |comment: &Comment| {
        comment
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap()
            .clone()
    };
    let index = comments
        .iter()
        .position(|comment| {
            subject(comment).is_some_and(|target| {
                target.alias() == "post"
                    && target.downcast_ref::<Post>().unwrap().get_key() == Some(&1.into())
            })
        })
        .unwrap();
    let before = subject(&comments[index]).unwrap();
    assert!(
        !before
            .downcast_ref::<Post>()
            .unwrap()
            .get_relation::<RelatedCollection<Comment>>("comments")
            .unwrap()
            .is_empty()
    );
    assert!(comments[index].to_array().unwrap()["subject"]["comments"].is_array());
    let author = comments
        .iter()
        .find_map(|comment| subject(comment).filter(|target| target.alias() == "author"))
        .unwrap();
    assert!(
        !author
            .downcast_ref::<Author>()
            .unwrap()
            .relation_loaded("comments")
    );

    let empty = POST_COMMENTS.constrain(|mut query| {
        query.r#where("id", "<", 0);
        query
    });
    let baseline = if mysql { select_count(db).await } else { 0 };
    comments
        .load_morph("subject", subjects, "post", "comments", &empty)
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    assert_eq!(
        comments[index].to_array().unwrap()["subject"]["comments"],
        json!([])
    );
    assert!(
        !before
            .downcast_ref::<Post>()
            .unwrap()
            .get_relation::<RelatedCollection<Comment>>("comments")
            .unwrap()
            .is_empty()
    );

    let invalid = POST_COMMENTS.constrain(|mut query| {
        query.r#where("relation_comments.missing_comment_column", "=", 1);
        query
    });
    assert!(
        comments
            .load_morph("subject", subjects, "post", "comments", &invalid)
            .await
            .is_err()
    );
    assert_eq!(
        comments[index].to_array().unwrap()["subject"]["comments"],
        json!([])
    );

    comments
        .load_morph_paths::<Post>("subject", subjects, "post", &["comments.post"])
        .await
        .unwrap();
    let nested = subject(&comments[index]).unwrap();
    let children = nested
        .downcast_ref::<Post>()
        .unwrap()
        .get_relation::<RelatedCollection<Comment>>("comments")
        .unwrap();
    assert!(!children.is_empty());
    assert!(
        children
            .iter()
            .all(|comment| comment.relation_loaded("post"))
    );
    let projected = comments[index].to_array().unwrap();
    assert!(projected["subject"]["comments"].is_array());

    let mut single = Comment::from_state(comments[index].state().clone());
    single
        .load_morph("subject", subjects, "post", "comments", &empty)
        .await
        .unwrap();
    assert_eq!(single.to_array().unwrap()["subject"]["comments"], json!([]));
    assert_eq!(comments[index].to_array().unwrap(), projected);
}

async fn verify_morph_aggregates(
    db: &Connection,
    mysql: bool,
    comments: &mut Collection<Comment>,
    subjects: &MorphTo<Comment>,
) {
    let relation = POST_COMMENTS;
    let baseline = if mysql { select_count(db).await } else { 0 };
    let aggregates = [
        RelationAggregateQuery::new(
            "comments as has_feedback",
            &relation,
            RelationAggregate::Exists,
        ),
        RelationAggregateQuery::new(
            "comments as last_feedback",
            &relation,
            RelationAggregate::Max("id"),
        ),
    ];
    comments
        .load_morph_aggregates("subject", subjects, "post", &aggregates)
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    let mut post_index = None;
    for (index, comment) in comments.iter().enumerate() {
        let subject = comment
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap();
        match subject.as_ref().map(MorphTarget::alias) {
            Some("post") => {
                let post = subject.as_ref().unwrap().downcast_ref::<Post>().unwrap();
                let has_feedback = post.get_attribute("last_feedback").unwrap() != &Value::Null;
                assert_eq!(
                    post.get_attribute("has_feedback"),
                    Some(&has_feedback.into())
                );
                assert_eq!(
                    post.get_raw_original("last_feedback"),
                    post.get_attribute("last_feedback")
                );
                post_index = Some(index);
            }
            Some("author") => {
                let author = subject.as_ref().unwrap().downcast_ref::<Author>().unwrap();
                assert!(author.get_attribute("posts_count").is_some());
                assert!(author.get_attribute("last_feedback").is_none());
            }
            None => {}
            Some(alias) => panic!("unexpected alias {alias}"),
        }
    }
    let row = &comments[post_index.unwrap()];
    let mut model = Comment::from_state(row.state().clone());
    model
        .load_morph_count(
            "subject",
            subjects,
            "post",
            "comments as feedback_count",
            &relation,
        )
        .await
        .unwrap();
    assert!(model.to_array().unwrap()["subject"]["feedback_count"].is_u64());
    assert!(
        row.to_array().unwrap()["subject"]
            .get("feedback_count")
            .is_none()
    );

    let invalid = [
        RelationAggregateQuery::new("comments as duplicate", &relation, RelationAggregate::Count),
        RelationAggregateQuery::new(
            "comments as duplicate",
            &relation,
            RelationAggregate::Exists,
        ),
    ];
    assert!(matches!(
        comments
            .load_morph_aggregates("subject", subjects, "post", &invalid)
            .await,
        Err(DatabaseError::InvalidQuery("duplicate aggregate alias"))
    ));
    assert!(matches!(
        comments
            .load_morph_aggregates("subject", subjects, "author", &aggregates)
            .await,
        Err(DatabaseError::InvalidQuery(
            "morph type does not match model"
        ))
    ));
}

fn verify_morph_batches(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        db.insert(
            "INSERT INTO relation_comments (body, subject_type, subject_id) VALUES (?, ?, ?)",
            &["author profile".into(), "author".into(), 1.into()],
        )
        .await
        .unwrap();
        let subjects = || {
            MorphTo::<Comment>::new("subject_type", "subject_id")
                .target::<Author>("author", "id")
                .target::<Post>("post", "id")
        };
        let mut comments = Comment::query_on(db)
            .with("subject", subjects())
            .get()
            .await
            .unwrap();
        assert!(
            comments
                .morph_models::<Author>("subject", &subjects(), "post")
                .await
                .is_err()
        );
        let baseline = if mysql { select_count(db).await } else { 0 };
        {
            let mut posts = comments
                .load_morph("subject", &subjects(), "post", "comments", &POST_COMMENTS)
                .await
                .unwrap();
            assert_eq!(posts.all().len(), 2);
            let first = posts
                .all()
                .iter()
                .position(|model| model.get_key() == Some(&1.into()))
                .unwrap();
            let comments = posts
                .get_relation_or_load(first, "comments", &POST_COMMENTS)
                .await
                .unwrap();
            assert!(!comments.is_empty());
        }
        {
            let authors = comments
                .load_morph_count("subject", &subjects(), "author", "posts", &POSTS)
                .await
                .unwrap();
            assert_eq!(authors.all().len(), 1);
            assert_eq!(authors.all()[0].get_key(), Some(&1.into()));
            assert!(authors.get_attribute(0, "posts_count").is_some());
            assert!(authors.get_attribute(0, "comments_count").is_none());
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }

        Box::pin(verify_morph_relation_writeback(
            db,
            mysql,
            &mut comments,
            &subjects(),
        ))
        .await;
        Box::pin(verify_morph_aggregates(
            db,
            mysql,
            &mut comments,
            &subjects(),
        ))
        .await;

        let morph = subjects();
        let mut roots = Comment::query_on(db)
            .with("subject", subjects())
            .get()
            .await
            .unwrap()
            .with_relationship_autoloading();
        let post_indexes: Vec<_> = roots
            .iter()
            .enumerate()
            .filter_map(|(index, root)| {
                let target = root
                    .get_relation::<Option<MorphTarget>>("subject")?
                    .as_ref()?;
                (target.alias() == "post").then_some(index)
            })
            .collect();
        assert!(post_indexes.len() >= 2);
        let baseline = if mysql { select_count(db).await } else { 0 };
        for &index in &post_indexes {
            let mut root = roots.model_at(index).unwrap();
            assert!(
                root.morph_model::<Author>("subject", &morph, "author")
                    .await
                    .unwrap()
                    .is_none()
            );
            let mut post = root
                .morph_model::<Post>("subject", &morph, "post")
                .await
                .unwrap()
                .unwrap();
            post.get_relation_or_load("comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        for root in roots.iter() {
            let Some(target) = root.get_relation::<Option<MorphTarget>>("subject").unwrap() else {
                continue;
            };
            match target.alias() {
                "post" => assert!(
                    target
                        .downcast_ref::<Post>()
                        .unwrap()
                        .relation_loaded("comments")
                ),
                "author" => assert!(
                    !target
                        .downcast_ref::<Author>()
                        .unwrap()
                        .relation_loaded("comments")
                ),
                _ => unreachable!(),
            }
        }

        let author_relation = AUTHOR;
        let posts_relation = POSTS;
        let baseline = if mysql { select_count(db).await } else { 0 };
        for &index in &post_indexes {
            let mut root = roots.model_at(index).unwrap();
            let mut post = root
                .morph_model::<Post>("subject", &morph, "post")
                .await
                .unwrap()
                .unwrap();
            let mut author = post
                .related_model_at("author", &author_relation, 0)
                .await
                .unwrap()
                .unwrap();
            assert!(
                !author
                    .get_relation_or_load("posts", &posts_relation)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        for index in post_indexes {
            let subject = &roots.to_array().unwrap()[index]["subject"];
            assert!(subject["author"]["posts"].is_array());
        }

        let mut moved_posts = Vec::new();
        for mut root in Comment::query_on(db)
            .with("subject", subjects())
            .get()
            .await
            .unwrap()
            .with_relationship_autoloading()
        {
            if let Some(post) = root
                .get_relation_or_load("subject", &morph)
                .await
                .unwrap()
                .as_ref()
                .and_then(MorphTarget::downcast_ref::<Post>)
            {
                moved_posts.push(Post::from_state(post.state().clone()));
            }
        }
        assert!(moved_posts.len() >= 2);
        let baseline = if mysql { select_count(db).await } else { 0 };
        for post in &mut moved_posts {
            post.get_relation_or_load("comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }

        let mut authors = Author::query_on(db).get().await.unwrap();
        let mut comments = authors
            .related_models("comments", &COMMENTS_THROUGH)
            .await
            .unwrap();
        let posts = comments
            .morph_models::<Post>("subject", &subjects(), "post")
            .await
            .unwrap();
        assert!(!posts.all().is_empty());
    })
}

async fn verify_path_constraints(db: &Connection) {
    let path = "posts.comments.post";
    let baseline = Author::query_on(db)
        .where_has_path(path)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(!baseline.is_empty());
    let column = Author::query_on(db)
        .where_relation_path(path, "id", ">", 0)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(column.model_keys(), baseline.model_keys());
    let closure = Author::query_on(db)
        .where_relation_path_with(path, |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "<", 0).or_where(
                &format!("{table}.id"),
                ">",
                0,
            )
        })
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(closure.model_keys(), baseline.model_keys());
    let mut either = Author::query_on(db);
    either.where_key(0);
    assert_eq!(
        either
            .or_where_relation_path(path, "id", ">", 0)
            .unwrap()
            .get()
            .await
            .unwrap()
            .model_keys(),
        baseline.model_keys()
    );
    let all = Author::query_on(db).get().await.unwrap();
    let missing = Author::query_on(db)
        .where_doesnt_have_relation_path(path, "id", "<", 0)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(missing.model_keys(), all.model_keys());
    let mut either_missing = Author::query_on(db);
    either_missing.where_key(0);
    assert_eq!(
        either_missing
            .or_where_doesnt_have_relation_path(path, "id", "<", 0)
            .unwrap()
            .get()
            .await
            .unwrap()
            .model_keys(),
        all.model_keys()
    );

    let selected_comment = Comment::query_on(db)
        .where_has_path("post")
        .unwrap()
        .order_by("id", "asc")
        .first_or_fail()
        .await
        .unwrap();
    let id = selected_comment.get_key().unwrap().clone();
    let loaded = Author::query_on(db)
        .with_where_relation_path_with("posts.comments", move |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "=", id.clone())
        })
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(loaded.len(), 1);
    let posts = loaded[0]
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    let comments: Vec<_> = posts
        .iter()
        .flat_map(|post| {
            post.get_relation::<RelatedCollection<Comment>>("comments")
                .unwrap()
                .iter()
        })
        .collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].get_key(), selected_comment.get_key());
    assert!(
        Author::query_on(db)
            .where_relation_path("posts.bad", "id", "=", 1)
            .is_err()
    );
}

async fn verify_model_reload_relations(db: &Connection, model: &Author) {
    let fresh = model
        .fresh_with_paths(&["posts.comments"])
        .await
        .unwrap()
        .unwrap();
    assert!(fresh.relation_loaded("posts"));
    assert_eq!(fresh.get_key(), model.get_key());
    let mut author = Author::query_on(db)
        .with_paths(&["posts"])
        .unwrap()
        .find_or_fail(model.get_key().unwrap().clone())
        .await
        .unwrap();
    let before = author
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    let cached = before[0].clone();
    author.refresh().await.unwrap();
    let after = author
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert!(!Arc::ptr_eq(&cached, &after[0]));

    let mut tx = db.begin_transaction().await.unwrap();
    let post_key = after[0].get_key().unwrap().clone();
    tx.table("relation_posts")
        .r#where("id", "=", post_key.clone())
        .update(Attributes::from([(
            "title".into(),
            "Uncommitted title".into(),
        )]))
        .await
        .unwrap();
    author.refresh_for_update(&mut tx).await.unwrap();
    let posts = author
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert_eq!(
        posts
            .iter()
            .find(|post| post.get_key() == Some(&post_key))
            .unwrap()
            .get_attribute("title"),
        Some(&"Uncommitted title".into()),
    );
    tx.roll_back(None).await.unwrap();

    author.load("unregistered", &POSTS).await.unwrap();
    let prior_name = author.get_raw_original("name").unwrap().clone();
    let key = author.get_key().unwrap().clone();
    db.table("relation_authors")
        .r#where("id", "=", key.clone())
        .update(Attributes::from([(
            "name".into(),
            "Database reload".into(),
        )]))
        .await
        .unwrap();
    assert!(matches!(
        author.refresh().await,
        Err(DatabaseError::InvalidQuery("unknown relationship path"))
    ));
    assert_eq!(
        author.get_attribute("name"),
        Some(&"Database reload".into())
    );
    assert_eq!(author.get_raw_original("name"), Some(&prior_name));
    db.table("relation_authors")
        .r#where("id", "=", key)
        .update(Attributes::from([("name".into(), prior_name)]))
        .await
        .unwrap();
}

async fn verify_eager_columns(db: &Connection, mysql: bool) {
    assert!(matches!(
        Author::query_on(db).with_paths(&["posts:id,"]),
        Err(DatabaseError::InvalidQuery("invalid relationship columns"))
    ));
    let baseline = if mysql { select_count(db).await } else { 0 };
    let author = Author::query_on(db)
        .with_paths(&[
            "posts:id,author_id,title",
            "posts.comments:id,subject_id,body",
        ])
        .unwrap()
        .find_or_fail(1)
        .await
        .unwrap();
    let posts = author
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert!(!posts.is_empty());
    let mut comments_count = 0;
    for post in posts {
        assert!(post.get_attribute("title").is_some());
        assert!(post.get_attribute("score").is_none());
        assert_eq!(post.get_attribute("author_id"), author.get_key());
        for comment in post
            .get_relation::<RelatedCollection<Comment>>("comments")
            .unwrap()
        {
            assert_eq!(comment.get_attribute("subject_id"), post.get_key());
            assert!(comment.get_attribute("body").is_some());
            assert!(comment.get_attribute("subject_type").is_none());
            comments_count += 1;
        }
    }
    assert!(comments_count > 0);
    if mysql {
        assert_eq!(select_count(db).await - baseline, 3);
    }

    let mut author = Author::query_on(db).find_or_fail(1).await.unwrap();
    author
        .load_missing_paths(&["posts:id,author_id"])
        .await
        .unwrap();
    author
        .load_missing_paths(&["posts:id,author_id,title"])
        .await
        .unwrap();
    let posts = author
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert!(
        posts
            .iter()
            .all(|post| post.get_attribute("title").is_none())
    );

    for paths in [
        ["posts:id,author_id", "posts.comments:id,subject_id,body"],
        ["posts.comments:id,subject_id,body", "posts:id,author_id"],
    ] {
        let mut query = Author::query_on(db);
        query.where_key(1);
        let mut authors = query.get().await.unwrap();
        let baseline = if mysql { select_count(db).await } else { 0 };
        authors.load_missing_paths(&paths).await.unwrap();
        authors
            .load_missing_paths(&["posts.comments:id,subject_id"])
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        let posts = authors[0]
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap();
        assert!(!posts.is_empty());
        let mut loaded_comments = 0;
        for post in posts {
            assert!(post.get_attribute("title").is_none());
            for comment in post
                .get_relation::<RelatedCollection<Comment>>("comments")
                .unwrap()
            {
                assert!(comment.get_attribute("body").is_some());
                loaded_comments += 1;
            }
        }
        assert!(loaded_comments > 0);
    }
    let mut query = Author::query_on(db).with_paths(&["posts"]).unwrap();
    query.where_key(1);
    let mut authors = query.get().await.unwrap();
    let baseline = if mysql { select_count(db).await } else { 0 };
    authors
        .load_missing_paths(&[
            "posts.comments:id,subject_id",
            "posts.comments:id,subject_id,body",
        ])
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    let loaded_posts = authors[0]
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert!(
        loaded_posts
            .iter()
            .flat_map(|post| {
                post.get_relation::<RelatedCollection<Comment>>("comments")
                    .unwrap()
                    .iter()
            })
            .all(|comment| comment.get_attribute("body").is_none())
    );

    let mut query = Author::query_on(db)
        .with_paths(&["posts:id,author_id", "posts:id,author_id,title"])
        .unwrap();
    query.where_key(1);
    let eager = query.first_or_fail().await.unwrap();
    assert!(
        eager
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap()
            .iter()
            .all(|post| post.get_attribute("title").is_none())
    );

    let post_id = posts[0].get_key().unwrap().clone();
    FEATURED_POSTS
        .sync_without_detaching(&author, std::slice::from_ref(&post_id))
        .await
        .unwrap();
    let joined = Author::query_on(db)
        .with_paths(&["featured:id,title", "commentsThrough:id,body"])
        .unwrap()
        .find_or_fail(1)
        .await
        .unwrap();
    let featured = joined
        .get_relation::<RelatedCollection<Post>>("featured")
        .unwrap();
    let post = featured
        .iter()
        .find(|post| post.get_key() == Some(&post_id))
        .unwrap();
    assert!(post.get_attribute("author_id").is_none());
    assert!(post.get_relation::<Pivot>("pivot").is_some());
    let through = joined
        .get_relation::<RelatedCollection<Comment>>("commentsThrough")
        .unwrap();
    assert!(!through.is_empty());
    assert!(
        through
            .iter()
            .all(|comment| comment.get_attribute("post_id").is_none())
    );

    let comment = Comment::query_on(db)
        .with_paths(&["subject:id"])
        .unwrap()
        .where_morph_type(
            &MorphTo::<Comment>::new("subject_type", "subject_id").target::<Post>("post", "id"),
            Some("post"),
        )
        .first_or_fail()
        .await
        .unwrap();
    let owner = comment
        .get_relation::<Option<openrust::database::eloquent::relations::MorphTarget>>("subject")
        .unwrap()
        .as_ref()
        .unwrap()
        .downcast_ref::<Post>()
        .unwrap();
    assert_eq!(owner.get_key(), comment.get_attribute("subject_id"));
    assert!(owner.get_attribute("title").is_none());
}

async fn verify_named_paths(db: &Connection, mysql: bool) {
    assert!(matches!(
        Author::query_on(db).with_paths(&["posts..comments"]),
        Err(DatabaseError::InvalidQuery("invalid relationship path"))
    ));
    assert!(matches!(
        Author::query_on(db).with_paths(&["posts.unknown"]),
        Err(DatabaseError::InvalidQuery("unknown relationship path"))
    ));
    let matching = Author::query_on(db)
        .where_has_path("posts.comments.post")
        .unwrap()
        .get()
        .await
        .unwrap();
    let missing = Author::query_on(db)
        .where_doesnt_have_path("posts.comments.post")
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(!matching.is_empty());
    assert!(!missing.is_empty());
    assert!(matching.iter().all(|author| {
        missing
            .iter()
            .all(|other| other.get_key() != author.get_key())
    }));
    assert_eq!(
        matching.len() + missing.len(),
        Author::query_on(db).get().await.unwrap().len()
    );
    let one_comment_post = Author::query_on(db)
        .has_path_count("posts.comments.post", "=", 1)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(one_comment_post.len(), matching.len());
    let two_comment_posts = Author::query_on(db)
        .has_path_count("posts.comments.post", ">=", 2)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(two_comment_posts.is_empty());
    assert!(matches!(
        Author::query_on(db).where_has_path("posts."),
        Err(DatabaseError::InvalidQuery("invalid relationship path"))
    ));
    assert!(matches!(
        Author::query_on(db).where_doesnt_have_path("posts.unknown"),
        Err(DatabaseError::InvalidQuery("unknown relationship path"))
    ));
    let baseline = if mysql { select_count(db).await } else { 0 };
    let mut authors = Author::query_on(db)
        .with_paths(&["posts", "posts.comments", "posts.comments", "posts.author"])
        .unwrap()
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 4);
    }
    let with_posts = authors
        .iter()
        .find(|author| {
            !author
                .get_relation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .is_empty()
        })
        .unwrap();
    Box::pin(verify_model_reload_relations(db, with_posts)).await;
    let metadata_baseline = if mysql { select_count(db).await } else { 0 };
    assert_eq!(
        authors.get_queueable_class(),
        Some(std::any::type_name::<Author>())
    );
    assert_eq!(authors.get_queueable_ids(), authors.model_keys());
    assert_eq!(
        authors.get_queueable_connection().unwrap(),
        Some("relations")
    );
    let shared = authors.get_queueable_relations();
    assert!(shared.contains(&"posts".into()));
    for author in authors.iter() {
        assert!(
            shared
                .iter()
                .all(|path| author.get_queueable_relations().contains(path))
        );
    }
    if mysql {
        assert_eq!(select_count(db).await - metadata_baseline, 0);
    }
    let mut found_comments = false;
    for author in authors.iter() {
        for post in author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap()
        {
            assert!(post.relation_loaded("author"));
            found_comments |= !post
                .get_relation::<RelatedCollection<Comment>>("comments")
                .unwrap()
                .is_empty();
        }
    }
    assert!(found_comments);
    let baseline = if mysql { select_count(db).await } else { 0 };
    authors
        .load_missing_paths(&["posts.comments.post", "posts.author"])
        .await
        .unwrap();
    authors
        .load_missing_paths(&["posts.comments.post"])
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    let mut found = false;
    for author in authors.iter() {
        for post in author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap()
        {
            for comment in post
                .get_relation::<RelatedCollection<Comment>>("comments")
                .unwrap()
            {
                assert!(comment.relation_loaded("post"));
                found = true;
            }
        }
    }
    assert!(found);
    let mut constrained = Author::query_on(db)
        .with_paths(&["posts.author"])
        .unwrap()
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    let mut query = Author::query_on(db)
        .with_paths(&["posts.comments"])
        .unwrap();
    query.where_key(1);
    let already_loaded = query.first_or_fail().await.unwrap();
    let count_loaded_comments = |author: &Author| {
        author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap()
            .iter()
            .map(|post| {
                post.get_relation::<RelatedCollection<Comment>>("comments")
                    .unwrap()
                    .len()
            })
            .sum::<usize>()
    };
    let expected = count_loaded_comments(&already_loaded);
    assert!(expected > 0);
    let mut mixed = Collection::new(
        std::iter::once(already_loaded)
            .chain(constrained.fresh_with_paths(&["posts"]).await.unwrap())
            .collect(),
    );
    let baseline = if mysql { select_count(db).await } else { 0 };
    mixed
        .load_missing_path_with("posts.comments:id,subject_id,body", |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "<", 0)
        })
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    assert_eq!(count_loaded_comments(&mixed[0]), expected);
    assert!(
        mixed
            .iter()
            .skip(1)
            .all(|author| count_loaded_comments(author) == 0)
    );

    let baseline = if mysql { select_count(db).await } else { 0 };
    constrained
        .load_missing_path_with("posts.author.posts", |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "<", 0)
        })
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    for author in constrained.iter() {
        for post in author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap()
        {
            let nested = post
                .get_relation::<Option<Arc<Author>>>("author")
                .unwrap()
                .as_ref()
                .unwrap();
            assert!(
                nested
                    .get_relation::<RelatedCollection<Post>>("posts")
                    .unwrap()
                    .is_empty()
            );
        }
    }

    let mut query = Author::query_on(db);
    query.where_key(1);
    let mut batch = query.get().await.unwrap();
    let selected_post = POSTS.get_results(&batch[0]).await.unwrap()[0]
        .get_key()
        .unwrap()
        .clone();
    let scoped_post = selected_post.clone();
    let applied = Arc::new(std::sync::Mutex::new(Vec::new()));
    let first = applied.clone();
    let second = applied.clone();
    let baseline = if mysql { select_count(db).await } else { 0 };
    batch
        .load_missing_paths_with([
            MissingRelation::new("posts.comments").constrain(move |conditions, table| {
                first.lock().unwrap().push("first");
                conditions.r#where(&format!("{table}.id"), "<", 0)
            }),
            MissingRelation::new("posts.author")
                .constrain(|conditions, table| conditions.r#where(&format!("{table}.id"), ">", 0)),
            MissingRelation::new("posts:id,author_id").constrain(move |conditions, table| {
                conditions.r#where(&format!("{table}.id"), "=", scoped_post.clone())
            }),
            MissingRelation::new("posts.comments").constrain(move |conditions, table| {
                second.lock().unwrap().push("second");
                conditions.r#where(&format!("{table}.id"), ">", 0)
            }),
        ])
        .await
        .unwrap();
    assert_eq!(count_loaded_comments(&batch[0]), 0);
    let posts = batch[0]
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].get_key(), Some(&selected_post));
    assert_eq!(*applied.lock().unwrap(), ["second", "first"]);
    assert!(posts.iter().all(|post| {
        post.get_attribute("title").is_none()
            && post
                .get_relation::<Option<Arc<Author>>>("author")
                .unwrap()
                .is_some()
    }));
    if mysql {
        assert_eq!(select_count(db).await - baseline, 3);
    }
    let baseline = if mysql { select_count(db).await } else { 0 };
    batch
        .load_missing_paths_with([
            MissingRelation::new("posts.author"),
            MissingRelation::new("posts.comments"),
        ])
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 0);
    }
    assert_eq!(count_loaded_comments(&batch[0]), 0);

    let mut model = Author::query_on(db).first_or_fail().await.unwrap();
    model.load_paths(&["posts.comments"]).await.unwrap();
    model
        .load_missing_path_with("posts.comments.post", |conditions, table| {
            conditions.r#where(&format!("{table}.id"), ">", 0)
        })
        .await
        .unwrap();
    assert!(model.relation_loaded("posts"));
    let mut loaded = Author::query_on(db)
        .where_has_path("posts")
        .unwrap()
        .first_or_fail()
        .await
        .unwrap();
    loaded.load_paths(&["posts.author"]).await.unwrap();
    loaded.make_hidden(&["posts"]);
    assert_eq!(loaded.get_queueable_relations(), ["posts", "posts.author"]);
    let mut other = Author::from_state(loaded.state().clone());
    other.unset_relation("posts");
    other.load_paths(&["posts"]).await.unwrap();
    let key = loaded.get_queueable_id().unwrap().clone();
    let models = Collection::new(vec![loaded, other]);
    assert_eq!(models.get_queueable_ids(), [key.clone(), key]);
    assert_eq!(models.get_queueable_relations(), ["posts"]);
    let mixed = Collection::new(vec![
        Author::from_state(models[0].state().clone()),
        Author::new_instance(Attributes::new()).unwrap(),
    ]);
    assert!(matches!(
        mixed.get_queueable_connection(),
        Err(DatabaseError::InvalidQuery(
            "queueable collection has multiple connections"
        ))
    ));
    assert_eq!(mixed.get_queueable_ids()[1], Value::Null);
    let empty = Collection::<Author>::new(vec![]);
    assert_eq!(empty.get_queueable_class(), None);
    assert_eq!(empty.get_queueable_connection().unwrap(), None);
    assert!(empty.get_queueable_ids().is_empty());
    assert!(empty.get_queueable_relations().is_empty());
}

async fn verify_morph_paths(db: &Connection, mysql: bool) {
    let author = Author::query_on(db)
        .where_has_path("posts")
        .unwrap()
        .first_or_fail()
        .await
        .unwrap();
    let table = || db.table("relation_comments");
    let profile = table()
        .insert_get_id(Attributes::from([
            ("body".into(), "named morph profile".into()),
            ("subject_type".into(), "author".into()),
            ("subject_id".into(), author.get_key().unwrap().clone()),
        ]))
        .await
        .unwrap();
    let no_target = table()
        .insert_get_id(Attributes::from([(
            "body".into(),
            "named morph null".into(),
        )]))
        .await
        .unwrap();
    let post = Post::query_on(db)
        .where_has_path("comments")
        .unwrap()
        .first_or_fail()
        .await
        .unwrap();
    let additional = table()
        .insert_get_id(Attributes::from([
            ("body".into(), "named morph counted".into()),
            ("subject_type".into(), "post".into()),
            ("subject_id".into(), post.get_key().unwrap().clone()),
        ]))
        .await
        .unwrap();
    let selected = Comment::query_on(db)
        .where_relation_path("subject", "id", "=", author.get_key().unwrap().clone())
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(selected.find(&profile).unwrap().is_some());
    assert!(selected.find(&no_target).unwrap().is_none());
    let baseline = if mysql { select_count(db).await } else { 0 };
    let comments = Comment::query_on(db)
        .order_by("id", "asc")
        .with_paths(&["subject", "subject.details", "subject.details"])
        .unwrap()
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 5);
    }
    let mut aliases = std::collections::HashSet::new();
    let mut expected = Vec::new();
    let mut counted = Vec::new();
    for comment in comments.iter() {
        let Some(target) = comment
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap()
        else {
            assert_eq!(comment.get_queueable_relations(), ["subject"]);
            continue;
        };
        assert!(
            comment
                .get_queueable_relations()
                .contains(&"subject.details".into())
        );
        aliases.insert(target.alias());
        let len = if let Some(author) = target.downcast_ref::<Author>() {
            author
                .get_relation::<RelatedCollection<Post>>("details")
                .unwrap()
                .len()
        } else {
            target
                .downcast_ref::<Post>()
                .unwrap()
                .get_relation::<RelatedCollection<Comment>>("details")
                .unwrap()
                .len()
        };
        if len > 0 {
            expected.push(comment.get_key().unwrap().clone());
        }
        if len >= 2 {
            counted.push(comment.get_key().unwrap().clone());
        }
    }
    let mut deferred = Comment::query_on(db)
        .with_paths(&["subject"])
        .unwrap()
        .get()
        .await
        .unwrap();
    let baseline = if mysql { select_count(db).await } else { 0 };
    deferred
        .load_missing_paths(&["subject.details"])
        .await
        .unwrap();
    deferred
        .load_missing_paths(&["subject.details"])
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }
    let mut filtered = Comment::query_on(db)
        .with_paths(&["subject"])
        .unwrap()
        .get()
        .await
        .unwrap();
    let baseline = if mysql { select_count(db).await } else { 0 };
    filtered
        .load_missing_path_with("subject.details", |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "<", 0)
        })
        .await
        .unwrap();
    filtered
        .load_missing_paths(&["subject.details"])
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }
    let filtered = filtered.to_array().unwrap();
    assert!(filtered.iter().any(|comment| comment["subject"].is_null()));
    assert!(
        filtered
            .iter()
            .filter(|comment| !comment["subject"].is_null())
            .all(|comment| comment["subject"]["details"] == serde_json::json!([]))
    );

    let mut combined = Comment::query_on(db).get().await.unwrap();
    let baseline = if mysql { select_count(db).await } else { 0 };
    combined
        .load_missing_paths_with([
            MissingRelation::new("subject.details")
                .constrain(|conditions, table| conditions.r#where(&format!("{table}.id"), "<", 0)),
            MissingRelation::new("subject")
                .constrain(|conditions, table| conditions.r#where(&format!("{table}.id"), ">", 0)),
        ])
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 4);
    }
    let combined = combined.to_array().unwrap();
    let with_subjects = combined
        .iter()
        .filter(|comment| !comment["subject"].is_null())
        .collect::<Vec<_>>();
    assert!(!with_subjects.is_empty());
    assert!(
        with_subjects
            .iter()
            .all(|comment| comment["subject"]["details"] == json!([]))
    );

    let mut subjects = Comment::query_on(db).get().await.unwrap();
    let baseline = if mysql { select_count(db).await } else { 0 };
    subjects
        .load_missing_path_with("subject", |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "<", 0)
        })
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }
    assert!(subjects.iter().all(|comment| {
        comment
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap()
            .is_none()
    }));

    let projected = deferred.to_array().unwrap();
    assert!(
        projected
            .iter()
            .any(|comment| comment["subject"].get("details").is_some())
    );
    assert_eq!(aliases, ["post", "author"].into_iter().collect());
    assert!(!expected.is_empty());
    assert!(!counted.is_empty());
    let matched = Comment::query_on(db)
        .where_has_path("subject.details")
        .unwrap()
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    let absent = Comment::query_on(db)
        .where_doesnt_have_path("subject.details")
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(!absent.is_empty());
    assert_eq!(matched.len() + absent.len(), comments.len());
    assert_eq!(matched.model_keys(), expected);
    assert_eq!(
        Comment::query_on(db)
            .has_path_count("subject.details", ">=", 2)
            .unwrap()
            .order_by("id", "asc")
            .get()
            .await
            .unwrap()
            .model_keys(),
        counted
    );
    assert!(matches!(
        Comment::query_on(db).with_paths(&["subject.comments"]),
        Err(DatabaseError::InvalidQuery("unknown relationship path"))
    ));
    let baseline = if mysql { select_count(db).await } else { 0 };
    let discovered = Comment::query_on(db)
        .where_has_path_all("subject.details")
        .await
        .unwrap()
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(discovered.model_keys(), expected);
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }
    assert_eq!(
        Comment::query_on(db)
            .has_path_count_all("subject.details", ">=", 2)
            .await
            .unwrap()
            .order_by("id", "asc")
            .get()
            .await
            .unwrap()
            .model_keys(),
        counted
    );
    let missing = Comment::query_on(db)
        .where_doesnt_have_path_all("subject.details")
        .await
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(missing.len(), absent.len());
    assert!(missing.model_keys().contains(&no_target));
    Box::pin(verify_discovered_path_targets(db, &expected)).await;
    let unknown = table()
        .insert_get_id(Attributes::from([
            ("body".into(), "unknown named morph".into()),
            ("subject_type".into(), "unknown".into()),
            ("deleted_at".into(), "2026-01-01 00:00:00".into()),
        ]))
        .await
        .unwrap();
    assert!(matches!(
        Comment::query_on(db)
            .where_has_path_all("subject.details")
            .await,
        Err(DatabaseError::InvalidQuery("unknown morph type"))
    ));
    table()
        .where_in("id", vec![unknown, profile.clone()])
        .delete()
        .await
        .unwrap();
    let only_posts = Comment::query_on(db)
        .where_has_path_all("subject.comments")
        .await
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(!only_posts.is_empty());
    Box::pin(verify_discovered_path_filter(db, &additional, &no_target)).await;
    assert!(absent.model_keys().contains(&no_target));
    db.table("relation_comments")
        .where_in("id", vec![no_target, additional])
        .delete()
        .await
        .unwrap();
}

async fn verify_discovered_path_targets(db: &Connection, expected: &[Value]) {
    let query = Comment::query_on(db)
        .with_where_relation_path_all("subject.details", "id", ">", 0)
        .await
        .unwrap()
        .order_by("id", "asc");
    let mixed = Box::pin(query.get()).await.unwrap();
    assert_eq!(mixed.model_keys(), expected);
    let mut aliases = std::collections::HashSet::new();
    for row in mixed.to_array().unwrap() {
        assert!(!row["subject"]["details"].as_array().unwrap().is_empty());
        aliases.insert(row["subject_type"].as_str().unwrap().to_owned());
    }
    assert_eq!(
        aliases,
        ["author".into(), "post".into()].into_iter().collect()
    );
}

async fn verify_discovered_path_filter(db: &Connection, additional: &Value, no_target: &Value) {
    let path = "subject.comments";
    let body = "named morph counted";
    let query = Comment::query_on(db)
        .where_relation_path_all(path, "body", "=", body)
        .await
        .unwrap()
        .order_by("id", "asc");
    let filtered = Box::pin(query.get()).await.unwrap();
    assert!(filtered.model_keys().contains(additional));

    let query = Comment::query_on(db)
        .where_doesnt_have_relation_path_all(path, "body", "=", body)
        .await
        .unwrap();
    let absent = Box::pin(query.get()).await.unwrap();
    assert!(absent.model_keys().contains(no_target));
    assert!(!absent.model_keys().contains(additional));

    let query = Comment::query_on(db)
        .with_where_relation_path_all_with(path, move |conditions, qualifier| {
            conditions.r#where(&format!("{qualifier}.body"), "=", body)
        })
        .await
        .unwrap()
        .order_by("id", "asc");
    let loaded = Box::pin(query.get()).await.unwrap();
    assert_eq!(loaded.model_keys(), filtered.model_keys());
    for row in loaded.to_array().unwrap() {
        let comments = row["subject"]["comments"].as_array().unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0]["body"], body);
    }
}

fn verify_relation_chain(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let mut authors = Author::query_on(db)
            .with("posts", POSTS)
            .order_by("id", "asc")
            .get()
            .await
            .unwrap();
        let mut posts = authors.related_models("posts", &POSTS).await.unwrap();
        assert!(posts.all().len() > 1);
        let broken = POST_COMMENTS.constrain(|mut query| {
            query.r#where("relation_comments.unknown_relation_column", "=", "error");
            query
        });
        assert!(
            posts
                .get_relation_or_load(0, "comments", &broken)
                .await
                .is_err()
        );
        let baseline = if mysql { select_count(db).await } else { 0 };
        for index in 0..posts.all().len() {
            posts
                .get_relation_or_load(index, "comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        let mut comments = posts
            .related_models("comments", &POST_COMMENTS)
            .await
            .unwrap();
        assert!(comments.all().len() >= 2);
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        let post = BelongsTo::<Comment, Post>::new("post_id", "id");
        let linked = comments.related_models("post", &post).await.unwrap();
        assert!(linked.all().len() >= 2);
        assert!(authors.iter().all(|author| author.relation_loaded("posts")));

        let mut posts = Post::query_on(db)
            .with("author", AUTHOR)
            .get()
            .await
            .unwrap();
        let owners = posts.related_models("author", &AUTHOR).await.unwrap();
        let unique = owners
            .all()
            .iter()
            .map(|owner| owner.get_key().unwrap().clone())
            .collect::<Vec<_>>();
        assert!(unique.len() < posts.len());
    })
}

fn verify_lazy_loading_policy(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let posts = HasMany::<StrictAuthor, Post>::new("id", "author_id");
        let ordered = || StrictAuthor::query_on(db).order_by("id", "asc");
        let mut strict = ordered().find_or_fail(1).await.unwrap();
        let mut group = ordered().get().await.unwrap();
        let baseline = if mysql { select_count(db).await } else { 0 };
        assert!(matches!(
            strict.get_relation_or_load("posts", &posts).await,
            Err(DatabaseError::LazyLoadingViolation { model, relation })
                if model == std::any::type_name::<StrictAuthor>() && relation == "posts"
        ));
        assert!(matches!(
            group.get_relation_or_load(0, "posts", &posts).await,
            Err(DatabaseError::LazyLoadingViolation { .. })
        ));
        assert!(!strict.relation_loaded("posts"));
        assert!(!group[0].relation_loaded("posts"));
        if mysql {
            assert_eq!(select_count(db).await - baseline, 0);
        }

        strict.load("posts", &posts).await.unwrap();
        let baseline = if mysql { select_count(db).await } else { 0 };
        assert!(
            !strict
                .get_relation_or_load("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        let mut group = group.with_relationship_autoloading();
        assert!(
            group
                .get_relation_or_load(2, "posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(group.iter().all(|model| model.relation_loaded("posts")));
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }

        let mut fresh = StrictAuthor::new_instance(Attributes::new()).unwrap();
        assert!(
            fresh
                .get_relation_or_load("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        let mut created = ordered()
            .create(Attributes::from([("name".into(), "New author".into())]))
            .await
            .unwrap();
        assert!(created.state().was_recently_created());
        assert!(
            created
                .get_relation_or_load("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        created.delete().await.unwrap();

        let mut allowed = AuthorWithDefaults::query_on(db)
            .with_only(|loads| loads)
            .find_or_fail(1)
            .await
            .unwrap();
        let allowed_posts = HasMany::<AuthorWithDefaults, Post>::new("id", "author_id");
        assert!(
            !allowed
                .get_relation_or_load("posts", &allowed_posts)
                .await
                .unwrap()
                .is_empty()
        );
    })
}

fn verify_relation_autoload(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let ordered = || Author::query_on(db).order_by("id", "asc");
        let mut plain = ordered().get().await.unwrap();
        plain
            .get_relation_or_load(0, "posts", &POSTS)
            .await
            .unwrap();
        assert!(plain[0].relation_loaded("posts"));
        assert!(!plain[1].relation_loaded("posts"));
        let mut group = plain.with_relationship_autoloading().except(&[]).unwrap();
        let broken = POSTS.constrain(|mut query| {
            query.r#where("relation_posts.unknown_relation_column", "=", "error");
            query
        });
        assert!(group.model_at(20).is_none());
        assert!(
            group
                .model_at(1)
                .unwrap()
                .get_relation_or_load("posts", &broken)
                .await
                .is_err()
        );
        assert!(group[0].relation_loaded("posts"));
        assert!(!group[1].relation_loaded("posts"));
        assert!(!group[2].relation_loaded("posts"));

        let baseline = if mysql { select_count(db).await } else { 0 };
        {
            let mut author = group.model_at(1).unwrap();
            assert_eq!(author.get_key(), Some(&2.into()));
            author.get_relation_or_load("posts", &POSTS).await.unwrap();
        }
        assert!(
            group
                .get_relation_or_load(2, "posts", &POSTS)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(group.iter().all(|author| author.relation_loaded("posts")));
        assert!(
            group
                .get_relation_or_load(20, "posts", &POSTS)
                .await
                .is_err()
        );
        let wrong_output = HasOne::<Author, Post>::new("id", "author_id");
        assert!(
            group
                .get_relation_or_load(0, "posts", &wrong_output)
                .await
                .is_err()
        );
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }

        let mut detached = ordered()
            .get()
            .await
            .unwrap()
            .with_relationship_autoloading()
            .into_iter();
        let mut first = detached.next().unwrap();
        let mut second = detached.next().unwrap();
        let mut empty = detached.next().unwrap();
        drop(detached);
        assert!(first.get_relation_or_load("posts", &broken).await.is_err());
        assert!(!first.relation_loaded("posts"));
        let baseline = if mysql { select_count(db).await } else { 0 };
        let posts = POSTS;
        let first_posts = first.get_relation_or_load("posts", &posts).await.unwrap();
        let mut first_post = Post::from_state(first_posts[0].state().clone());
        assert!(second.relation_loaded("posts"));
        assert!(second.get_queueable_relations().contains(&"posts".into()));
        assert!(second.to_array().unwrap()["posts"].is_array());
        let second_posts = second
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap();
        let mut second_post = Post::from_state(second_posts[0].state().clone());
        assert!(empty.relation_loaded("posts"));
        assert!(
            empty
                .get_relation_or_load("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        let bad_comments = POST_COMMENTS.constrain(|mut query| {
            query.r#where("relation_comments.unknown_relation_column", "=", "error");
            query
        });
        assert!(
            first_post
                .get_relation_or_load("comments", &bad_comments)
                .await
                .is_err()
        );
        assert!(!second_post.relation_loaded("comments"));
        let baseline = if mysql { select_count(db).await } else { 0 };
        let comments = POST_COMMENTS;
        let (left, right) = tokio::join!(
            first_post.get_relation_or_load("comments", &comments),
            second_post.get_relation_or_load("comments", &comments)
        );
        left.unwrap();
        right.unwrap();
        assert!(second.to_array().unwrap()["posts"][0]["comments"].is_array());
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        first.unset_relation("posts");
        assert!(!first.relation_loaded("posts"));
        assert!(!first.to_array().unwrap().contains_key("posts"));
        assert!(second.relation_loaded("posts"));
        assert!(second.get_queueable_relations().contains(&"posts".into()));
        assert!(first.get_relation_or_load("posts", &broken).await.is_err());
        second.unset_relation("posts");
        second.set_attribute("id", 3);
        assert!(
            second
                .get_relation_or_load("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );

        let mut other = ordered().get().await.unwrap();
        assert!(other.iter().all(|author| !author.relation_loaded("posts")));
        let posts = POSTS;
        let mut descendant = ordered().get().await.unwrap();
        descendant
            .get_relation_or_load(0, "posts", &POSTS)
            .await
            .unwrap();
        let mut descendant = descendant.with_relationship_autoloading();
        let broken_comments = POST_COMMENTS.constrain(|mut query| {
            query.r#where("relation_comments.unknown_relation_column", "=", "error");
            query
        });
        {
            let mut author = descendant.model_at(0).unwrap();
            let mut post = author
                .related_model_at("posts", &posts, 0)
                .await
                .unwrap()
                .unwrap();
            assert!(
                post.get_relation_or_load("comments", &broken_comments)
                    .await
                    .is_err()
            );
            assert!(!post.relation_loaded("comments"));
        }
        let baseline = if mysql { select_count(db).await } else { 0 };
        for index in [0, 1] {
            let mut author = descendant.model_at(index).unwrap();
            let mut post = author
                .related_model_at("posts", &posts, 0)
                .await
                .unwrap()
                .unwrap();
            post.get_relation_or_load("comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        let json = descendant.to_array().unwrap();
        assert!(json[0]["posts"][0].get("comments").is_some());
        assert!(json[1]["posts"][0].get("comments").is_some());
        assert!(
            descendant
                .model_at(2)
                .unwrap()
                .related_model_at("posts", &posts, 0)
                .await
                .unwrap()
                .is_none()
        );
        {
            let mut author = other.model_at(0).unwrap();
            let mut post = author
                .related_model_at("posts", &posts, 0)
                .await
                .unwrap()
                .unwrap();
            post.get_relation_or_load("comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        assert!(!other[1].relation_loaded("posts"));
        assert!(
            other.to_array().unwrap()[0]["posts"][0]
                .get("comments")
                .is_some()
        );
        other
            .model_at(2)
            .unwrap()
            .get_relation_or_load("posts", &POSTS)
            .await
            .unwrap();
        assert!(!other[1].relation_loaded("posts"));
        let mut single = ordered().find_or_fail(3).await.unwrap();
        assert!(
            single
                .get_relation_or_load("posts", &POSTS)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            single
                .get_relation_or_load("posts", &broken)
                .await
                .unwrap()
                .is_empty()
        );

        let mut opted_in = AuthorWithDefaults::query_on(db)
            .with_only(|loads| loads)
            .order_by("id", "asc")
            .get()
            .await
            .unwrap();
        let posts = HasMany::<AuthorWithDefaults, Post>::new("id", "author_id");
        assert!(!opted_in[0].relation_loaded("posts"));
        opted_in
            .get_relation_or_load(0, "posts", &posts)
            .await
            .unwrap();
        assert!(
            opted_in
                .iter()
                .all(|author| author.relation_loaded("posts"))
        );
    })
}

fn verify_default_models(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let empty = Author::query_on(db).find_or_fail(3).await.unwrap();
        let blank_post = Post::new_instance(Attributes::new()).unwrap();
        let default_owner = AUTHOR.with_default(|owner, post| {
            assert!(post.get_attribute("author_id").is_none());
            owner.set_attribute("name", "Guest author");
        });
        let fallback = default_owner
            .get_results(&blank_post)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fallback.get_attribute("name"), Some(&"Guest author".into()));
        assert!(fallback.get_key().is_none());
        assert!(!fallback.state().exists());

        let newest = POSTS
            .one()
            .latest_of_many(None)
            .with_default(|post, parent| {
                assert_eq!(post.get_attribute("author_id"), parent.get_key());
                post.set_attribute("title", "No posts yet");
            });
        let baseline = if mysql { select_count(db).await } else { 0 };
        let authors = Author::query_on(db)
            .with("newest", newest)
            .order_by("id", "asc")
            .get()
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        let newest = |index: usize| {
            authors[index]
                .get_relation::<Option<Arc<Post>>>("newest")
                .unwrap()
                .as_ref()
                .unwrap()
        };
        assert!(newest(0).state().exists());
        assert_eq!(
            newest(2).get_attribute("title"),
            Some(&"No posts yet".into())
        );
        assert_eq!(newest(2).get_connection_name(), Some(db.get_name()));
        assert!(!newest(2).state().exists());

        let through = FIRST_COMMENT_THROUGH.with_default(|_, _| {});
        let through_default = through.get_results(&empty).await.unwrap().unwrap();
        assert!(!through_default.state().exists());
        assert!(through_default.get_attribute("post_id").is_none());
        assert!(
            Author::query_on(db)
                .where_doesnt_have(&through)
                .unwrap()
                .get()
                .await
                .unwrap()
                .model_keys()
                .contains(&3.into())
        );

        let morph = AUTHOR_COMMENT.with_default(|comment, _| {
            assert_eq!(
                comment.get_attribute("subject_type"),
                Some(&"author".into())
            );
        });
        let morph_default = morph.get_results(&empty).await.unwrap().unwrap();
        assert_eq!(morph_default.get_attribute("subject_id"), empty.get_key());
        assert!(!morph_default.state().exists());

        let mut no_owner = Post::query_on(db).make(Attributes::new()).unwrap();
        no_owner.set_attribute("author_id", 999_999);
        let filtered = AUTHOR.with_default(|model, _| {
            model.set_attribute("name", "Missing owner");
        });
        let missing = filtered.get_results(&no_owner).await.unwrap().unwrap();
        assert_eq!(missing.get_connection_name(), Some(db.get_name()));
        assert_eq!(missing.get_attribute("name"), Some(&"Missing owner".into()));
        assert!(missing.get_key().is_none());
    })
}

fn verify_default_eager_loads(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let ordered = || AuthorWithDefaults::query_on(db).order_by("id", "asc");
        let posts = || HasMany::<AuthorWithDefaults, Post>::new("id", "author_id");
        let baseline = if mysql { select_count(db).await } else { 0 };
        let models = ordered().get().await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 3);
        }
        assert!(
            models
                .iter()
                .all(|model| model.relation_loaded("posts") && model.relation_loaded("mentees"))
        );
        assert!(
            !models[0]
                .get_relation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .is_empty()
        );
        assert!(
            models[2]
                .get_relation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .is_empty()
        );

        let baseline = if mysql { select_count(db).await } else { 0 };
        let without_posts = ordered().without(&["posts"]).find_or_fail(1).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        assert!(!without_posts.relation_loaded("posts"));
        assert!(without_posts.relation_loaded("mentees"));
        assert!(without_posts.get_attribute("posts_count").is_some());
        let only = ordered()
            .with_only(|relations| relations.with("other_posts", posts()))
            .find_or_fail(1)
            .await
            .unwrap();
        assert!(only.relation_loaded("other_posts"));
        assert!(!only.relation_loaded("posts"));
        assert!(!only.relation_loaded("mentees"));
        assert!(only.get_attribute("posts_count").is_some());
        let projected = ordered()
            .with_only(|loads| loads)
            .select(["id"])
            .find_or_fail(1)
            .await
            .unwrap();
        assert!(projected.get_attribute("posts_count").is_none());

        let narrow = || {
            posts().constrain(|mut query| {
                query.r#where("title", "=", "no matching posts");
                query
            })
        };
        let explicit = ordered()
            .with("posts", narrow())
            .find_or_fail(1)
            .await
            .unwrap();
        assert!(
            explicit
                .get_relation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .is_empty()
        );
        assert!(explicit.relation_loaded("mentees"));
        let source = Collection::new(vec![
            ordered()
                .with_only(|loads| loads)
                .find_or_fail(1)
                .await
                .unwrap(),
            ordered()
                .with_only(|loads| loads)
                .find_or_fail(1)
                .await
                .unwrap(),
        ]);
        let baseline = if mysql { select_count(db).await } else { 0 };
        let fresh = source.fresh_with("posts", &narrow()).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 3);
        }
        assert_eq!(fresh.len(), 2);
        assert!(
            fresh
                .iter()
                .all(|model| model.get_attribute("posts_count").is_some())
        );
        assert!(fresh.iter().all(|model| {
            model.relation_loaded("mentees")
                && model
                    .get_relation::<RelatedCollection<Post>>("posts")
                    .unwrap()
                    .is_empty()
        }));

        let baseline = if mysql { select_count(db).await } else { 0 };
        let named = Box::pin(source.fresh_with_paths(&[
            "posts.comments",
            "posts.author",
            "posts.comments",
        ]))
        .await
        .unwrap();
        assert_eq!(named.model_keys(), source.model_keys());
        if mysql {
            assert_eq!(select_count(db).await - baseline, 5);
        }
        for model in named.iter() {
            assert!(model.relation_loaded("mentees"));
            let posts = model
                .get_relation::<RelatedCollection<Post>>("posts")
                .unwrap();
            assert!(!posts.is_empty());
            assert!(
                posts
                    .iter()
                    .all(|post| post.relation_loaded("comments") && post.relation_loaded("author"))
            );
        }
        assert!(source.iter().all(|model| !model.relation_loaded("posts")));
        assert!(matches!(
            Box::pin(source.fresh_with_paths(&["posts.invalid"])).await,
            Err(DatabaseError::InvalidQuery("unknown relationship path"))
        ));
        assert!(
            Box::pin(Collection::<AuthorWithDefaults>::new(vec![]).fresh_with_paths(&["posts"]))
                .await
                .unwrap()
                .is_empty()
        );

        let baseline = if mysql { select_count(db).await } else { 0 };
        let mut keyed = source.to_query().unwrap().get().await.unwrap();
        assert!(keyed[0].get_attribute("posts_count").is_none());
        keyed.load_count("posts", &posts()).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        assert!(!keyed[0].relation_loaded("posts"));
        assert!(keyed[0].get_attribute("posts_count").is_some());

        let via_posts = HasManyThrough::<Author, Post, AuthorWithDefaults>::new(
            "author_id",
            "id",
            "id",
            "author_id",
        )
        .constrain(|query| query.with_only(|loads| loads));
        let plain_author = Author::query_on(db).find_or_fail(1).await.unwrap();
        let matched = via_posts.get_results(&plain_author).await.unwrap();
        assert!(!matched.is_empty());
        assert!(
            matched
                .iter()
                .all(|model| model.get_attribute("posts_count").is_some())
        );
    })
}

fn verify_related_column_queries(db: &Connection) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let filtered = Author::query_on(db)
            .with_where_relation(
                "posts",
                POSTS.constrain(|mut query| {
                    query
                        .r#where("title", "=", "First post")
                        .or_where("title", "=", "Second post");
                    query
                }),
                "score",
                ">",
                15,
            )
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(filtered.model_keys(), [2.into()]);
        let loaded = filtered[0]
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded[0].get_attribute("title"),
            Some(&"Second post".into())
        );
        let mut either = Author::query_on(db);
        either.where_key(3);
        assert_eq!(
            either
                .or_where_relation(POSTS, "score", ">", 15)
                .unwrap()
                .order_by("id", "asc")
                .get()
                .await
                .unwrap()
                .model_keys(),
            [2.into(), 3.into()],
        );
        let mut either_missing = Author::query_on(db);
        either_missing.where_key(2);
        assert_eq!(
            either_missing
                .or_where_doesnt_have_relation(POSTS, "score", ">", 15)
                .unwrap()
                .order_by("id", "asc")
                .get()
                .await
                .unwrap()
                .model_keys(),
            [1.into(), 2.into(), 3.into()],
        );
        assert_eq!(
            Author::query_on(db)
                .where_doesnt_have_relation(POSTS, "score", ">", 15)
                .unwrap()
                .order_by("id", "asc")
                .get()
                .await
                .unwrap()
                .model_keys(),
            [1.into(), 3.into()],
        );
        assert_eq!(
            Author::query_on(db)
                .where_relation(MENTEES, "id", "=", 2)
                .unwrap()
                .get()
                .await
                .unwrap()
                .model_keys(),
            [1.into()],
        );
    })
}

fn verify_morph_column_queries<'a>(
    db: &'a Connection,
    subjects: &'a MorphTo<Comment>,
) -> openrust::database::DatabaseFuture<'a, ()> {
    Box::pin(async move {
        let selected = || {
            let mut query = Comment::query_on(db);
            query.r#where("body", "like", "wild %");
            query
        };
        let column_match = selected()
            .where_morph_relation(subjects, &["author", "post"], "id", "=", 1)
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(column_match.len(), 1);
        assert_eq!(
            column_match[0].get_attribute("body"),
            Some(&"wild live".into())
        );
        let column_absent = selected()
            .where_morph_doesnt_have_relation(subjects, &["post"], "id", "=", 1)
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(column_absent.len(), 1);
        assert_eq!(
            column_absent[0].get_attribute("body"),
            Some(&"wild missing".into())
        );
        let mut either = Comment::query_on(db);
        either.r#where("body", "=", "wild null");
        let either = either
            .or_where_morph_relation(subjects, &["author"], "id", "=", 1)
            .unwrap()
            .get()
            .await
            .unwrap();
        assert!(
            either
                .iter()
                .any(|model| model.get_attribute("body") == Some(&"wild live".into()))
        );
        assert!(
            either
                .iter()
                .any(|model| model.get_attribute("body") == Some(&"wild null".into()))
        );
        assert!(
            !either
                .iter()
                .any(|model| model.get_attribute("body") == Some(&"wild missing".into()))
        );
        let mut either_missing = Comment::query_on(db);
        either_missing.r#where("body", "=", "wild null");
        let either_missing = either_missing
            .or_where_morph_doesnt_have_relation(subjects, &["post"], "id", "=", 1)
            .unwrap()
            .get()
            .await
            .unwrap();
        assert!(
            either_missing
                .iter()
                .any(|model| model.get_attribute("body") == Some(&"wild missing".into()))
        );
        assert!(
            either_missing
                .iter()
                .any(|model| model.get_attribute("body") == Some(&"wild null".into()))
        );
        assert!(
            !either_missing
                .iter()
                .any(|model| model.get_attribute("body") == Some(&"wild live".into()))
        );
        assert!(
            selected()
                .where_morph_relation(subjects, &["unknown"], "id", "=", 1)
                .is_err()
        );
    })
}

fn verify_morph_wildcard(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Author>("author", "id")
            .target::<Post>("post", "id")
            .try_constrain::<Author>("unused", "id", |_| {
                Err(openrust::database::DatabaseError::InvalidQuery(
                    "unused morph query",
                ))
            });
        db.insert("INSERT INTO relation_comments (body, subject_type, subject_id) VALUES (?, ?, ?), (?, ?, ?), (?, ?, ?)",
            &["wild live".into(), "author".into(), 1.into(),
              "wild missing".into(), "post".into(), 999_999.into(),
              "wild null".into(), Value::Null, Value::Null]).await.unwrap();
        let selected = || {
            let mut query = Comment::query_on(db);
            query.r#where("body", "like", "wild %");
            query.order_by("body", "asc")
        };
        verify_morph_column_queries(db, &subjects).await;
        let baseline = if mysql { select_count(db).await } else { 0 };
        let present = selected()
            .where_has_morph_all(&subjects)
            .await
            .unwrap()
            .get()
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        assert_eq!(present.len(), 1);
        assert_eq!(present[0].get_attribute("body"), Some(&"wild live".into()));
        let absent = selected()
            .where_doesnt_have_morph_all(&subjects)
            .await
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(
            absent
                .iter()
                .map(|comment| comment.get_attribute("body").cloned().unwrap())
                .collect::<Vec<_>>(),
            vec!["wild missing".into(), "wild null".into()]
        );
        let zero = selected()
            .has_morph_count_all(&subjects, "=", 0)
            .await
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(zero.model_keys(), absent.model_keys());

        db.insert("INSERT INTO relation_comments (body, subject_type, subject_id, deleted_at) VALUES (?, ?, ?, ?)",
            &["hidden unknown".into(), "unregistered".into(), 1.into(), "2026-01-01 00:00:00".into()]).await.unwrap();
        assert!(matches!(
            selected().where_has_morph_all(&subjects).await,
            Err(openrust::database::DatabaseError::InvalidQuery(
                "unknown morph type"
            ))
        ));

        db.statement("UPDATE relation_comments SET subject_type = NULL", &[])
            .await
            .unwrap();
        assert!(
            selected()
                .where_has_morph_all(&subjects)
                .await
                .unwrap()
                .get()
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            selected()
                .where_doesnt_have_morph_all(&subjects)
                .await
                .unwrap()
                .get()
                .await
                .unwrap()
                .len(),
            3
        );
    })
}

fn verify_one_through<'a>(
    db: &'a Connection,
    mysql: bool,
    author: &'a Author,
) -> openrust::database::DatabaseFuture<'a, ()> {
    Box::pin(async move {
        let first = FIRST_COMMENT_THROUGH
            .get_results(author)
            .await
            .unwrap()
            .unwrap();
        let second_post = POSTS
            .create(
                author,
                Attributes::from([("title".into(), "Second path".into())]),
            )
            .await
            .unwrap();
        let mut tied = POST_COMMENTS
            .create(
                &second_post,
                Attributes::from([("body".into(), "visible".into())]),
            )
            .await
            .unwrap();
        tied.set_attribute("post_id", second_post.get_key().unwrap().clone());
        tied.save().await.unwrap();

        assert!(
            FIRST_COMMENT_THROUGH
                .is(author, Some(&first))
                .await
                .unwrap()
        );
        assert!(FIRST_COMMENT_THROUGH.is(author, Some(&tied)).await.unwrap());
        assert!(FIRST_COMMENT_THROUGH.is_not(author, None).await.unwrap());
        assert!(
            COMMENTS_THROUGH
                .one()
                .latest_of_many(None)
                .is(author, Some(&tied))
                .await
                .unwrap()
        );
        assert!(
            COMMENTS_THROUGH
                .one()
                .latest_of_many(None)
                .is_not(author, Some(&first))
                .await
                .unwrap()
        );
        let latest = || COMMENTS_THROUGH.one().latest_of_many(&["body", "id"]);
        let baseline = if mysql { select_count(db).await } else { 0 };
        let authors = Author::query_on(db)
            .with("latest_comment", latest())
            .order_by("id", "asc")
            .get()
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        let loaded = |index: usize| {
            authors[index]
                .get_relation::<Option<Arc<Comment>>>("latest_comment")
                .unwrap()
        };
        assert_eq!(loaded(0).as_ref().unwrap().get_key(), tied.get_key());
        assert!(loaded(1).is_none());
        assert!(loaded(2).is_none());

        let oldest = COMMENTS_THROUGH
            .one()
            .oldest_of_many(None)
            .get_results(author)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(oldest.get_key(), first.get_key());
        assert_eq!(
            COMMENTS_THROUGH
                .one()
                .oldest_of_many(&["body", "id"])
                .get_results(author)
                .await
                .unwrap()
                .unwrap()
                .get_key(),
            first.get_key()
        );
        let first_key = first.get_key().unwrap().clone();
        let constrained = latest()
            .where_candidates(move |query| query.r#where("id", "=", first_key.clone()))
            .unwrap();
        assert_eq!(
            constrained
                .get_results(author)
                .await
                .unwrap()
                .unwrap()
                .get_key(),
            first.get_key()
        );
        assert_eq!(
            Author::query_on(db)
                .has_count(&latest(), "=", 1)
                .unwrap()
                .get()
                .await
                .unwrap()
                .model_keys(),
            vec![1.into()]
        );
        let has_latest = Author::query_on(db)
            .with_exists("latest_comment", &latest())
            .unwrap()
            .order_by("id", "asc")
            .get()
            .await
            .unwrap();
        assert_eq!(
            has_latest[0].get_attribute("latest_comment_exists"),
            Some(&true.into())
        );
        assert_eq!(
            has_latest[1].get_attribute("latest_comment_exists"),
            Some(&false.into())
        );
        let shared_path =
            HasManyThrough::<Post, Post, Comment>::new("author_id", "post_id", "author_id", "id")
                .one()
                .of_many(&[
                    ("body", OneOfManyAggregate::Max),
                    ("id", OneOfManyAggregate::Min),
                ]);
        let projected = Post::query_on(db)
            .with_max("latest_comment as chosen_comment_id", &shared_path, "id")
            .unwrap()
            .find_or_fail(second_post.get_key().unwrap().clone())
            .await
            .unwrap();
        assert_eq!(
            projected.get_attribute("chosen_comment_id"),
            first.get_key()
        );

        let eligible = |query: openrust::database::query::Conditions| {
            query.r#where("body", "=", "hidden parent")
        };
        let excluding = latest().where_candidates(eligible).unwrap();
        assert!(excluding.get_results(author).await.unwrap().is_none());
        let including = latest()
            .with_trashed_parents()
            .where_candidates(eligible)
            .unwrap();
        assert_eq!(
            including
                .get_results(author)
                .await
                .unwrap()
                .unwrap()
                .get_attribute("body"),
            Some(&"hidden parent".into())
        );
        let deleted = COMMENTS_THROUGH
            .one()
            .latest_of_many(None)
            .constrain(|query| query.with_trashed())
            .where_candidates(|query| query.r#where("body", "=", "deleted"))
            .unwrap();
        assert_eq!(
            deleted
                .get_results(author)
                .await
                .unwrap()
                .unwrap()
                .get_attribute("body"),
            Some(&"deleted".into())
        );
    })
}

async fn verify_collection_collapse(db: &Connection, mysql: bool) {
    use openrust::support::Collection as BaseCollection;

    let mut authors = Author::query_on(db)
        .order_by("id", "asc")
        .get()
        .await
        .unwrap()
        .with_relationship_autoloading();
    let copy = |index: usize| Author::from_state(authors[index].state().clone());
    let models = BaseCollection::make([
        Collection::from_entries([("shared", copy(0)), ("7", copy(1))]),
        Collection::from_entries([("shared", copy(2)), ("12", copy(0))]),
    ])
    .collapse();
    assert_eq!(models.count(), 3);
    assert_eq!(models.get("shared").unwrap().get_key(), Some(&3.into()));
    assert_eq!(models.get(0).unwrap().get_key(), Some(&2.into()));
    assert_eq!(models.get(1).unwrap().get_key(), Some(&1.into()));
    assert!(models.get(7).is_none() && models.get(12).is_none());
    let vectors = BaseCollection::make([vec![1, 2], vec![], vec![3]]).collapse();
    assert_eq!(
        vectors
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let nested = BaseCollection::make([BaseCollection::make([vec![1, 2]])]).collapse();
    assert_eq!(nested.get(0), Some(&vec![1, 2]));

    let baseline = if mysql { select_count(db).await } else { 0 };
    let mut first = authors.model_at(0).unwrap();
    first.get_relation_or_load("posts", &POSTS).await.unwrap();
    assert!(authors.iter().all(|author| author.relation_loaded("posts")));
    let groups = authors.map(|author, _| {
        author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap()
            .clone()
    });
    let expected = groups.iter().flat_map(|(_, group)| group.iter());
    let flattened = groups.clone().collapse();
    assert_eq!(flattened.count(), expected.clone().count());
    assert!(!flattened.is_empty());
    for ((_, snapshot), loaded) in flattened.iter().zip(expected) {
        assert!(Arc::ptr_eq(snapshot, loaded));
    }
    let shared = flattened.get(0).unwrap();
    let mut independent = Post::from_state(shared.state().clone());
    independent.set_attribute("title", "edited outside the relationship");
    assert_ne!(
        independent.get_attribute("title"),
        shared.get_attribute("title")
    );
    assert!(authors[0].to_array().unwrap()["posts"].is_array());
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
}

fn verify_collection_keys<'a>(
    db: &'a Connection,
    other: &'a Connection,
) -> openrust::database::DatabaseFuture<'a, ()> {
    Box::pin(async move {
        let ordered = || Author::query_on(db).order_by("id", "asc");
        let authors = ordered().get().await.unwrap();
        let labels = authors.map(|model, index| (index, model.get_key().cloned()));
        assert_eq!(labels.get(1), Some(&(1, Some(2.into()))));
        let indexed = authors
            .map_with_keys(|model, index| [(format!("author-{index}"), model.get_key().cloned())]);
        assert_eq!(indexed.get("author-2"), Some(&Some(3.into())));
        let ids = authors.pluck(|model| model.get_key().cloned());
        assert_eq!(ids.get(0), Some(&Some(1.into())));
        let repeated = authors.pluck_with_keys(|_| "id", |model| model.get_key().cloned());
        assert_eq!(repeated.count(), 1);
        assert_eq!(repeated.get("id"), Some(&Some(3.into())));
        let retained = ordered().get().await.unwrap().map_models(|model, _| model);
        assert_eq!(retained.model_keys(), authors.model_keys());
        assert_eq!(
            retained.to_base().get(1).unwrap().get_key(),
            Some(&2.into())
        );
        let empty = Collection::<Author>::new(vec![]);
        assert!(empty.map::<i32>(|_, _| unreachable!()).is_empty());
        assert_eq!(authors.keys().get(2), Some(&2.into()));
        let keyed = ordered()
            .get()
            .await
            .unwrap()
            .map_models_with_keys(|model, index| {
                [(if index == 1.into() { "7" } else { "last" }, model)]
            });
        assert_eq!(keyed.model_keys(), [3.into(), 2.into()]);
        assert_eq!(keyed.get_by_key(7).unwrap().get_key(), Some(&2.into()));
        assert_eq!(keyed.first().unwrap().get_key(), Some(&3.into()));
        assert_eq!(keyed.keys().get(0), Some(&"last".into()));
        assert_eq!(
            keyed.map(|model, _| model.get_key().cloned()).get("last"),
            Some(&Some(3.into()))
        );
        assert_eq!(
            serde_json::from_str::<Json>(&keyed.to_json().unwrap()).unwrap()["7"]["id"],
            json!(2)
        );
        let fresh = keyed.fresh().await.unwrap();
        assert_eq!(fresh.get_by_key("last").unwrap().get_key(), Some(&3.into()));
        let zipped = keyed.fresh().await.unwrap().zip(["a", "b", "c"]);
        assert_eq!(zipped.count(), 3);
        assert_eq!(
            zipped.get(0).unwrap().0.as_ref().unwrap().get_key(),
            Some(&3.into())
        );
        assert_eq!(
            zipped
                .get(2)
                .map(|(model, label)| (model.is_none(), *label)),
            Some((true, Some("c")))
        );
        let shorter = keyed.fresh().await.unwrap().zip(["a"]);
        assert!(shorter.get(1).unwrap().1.is_none());
        let columns = keyed
            .fresh()
            .await
            .unwrap()
            .zip_many([&["a"][..], &["x", "y", "z"], &[]]);
        assert_eq!(columns.count(), 3);
        assert_eq!(columns.get(0).unwrap().1, [Some(&"a"), Some(&"x"), None]);
        assert_eq!(columns.get(1).unwrap().1, [None, Some(&"y"), None]);
        let (model, values) = columns.get(2).unwrap();
        assert!(model.is_none());
        assert_eq!(values, &[None, Some(&"z"), None]);
        let empty_columns = keyed.fresh().await.unwrap().zip_many::<i32, [i32; 0]>([]);
        assert_eq!(empty_columns.count(), 2);
        assert!(empty_columns.get(0).unwrap().1.is_empty());

        let flipped = keyed.flip_by(|author| (author.get_key() == Some(&2.into())).then_some("42"));
        assert_eq!(flipped.count(), 1);
        assert_eq!(flipped.get(42), Some(&7.into()));
        let collision = keyed.flip_by(|_| Some("shared"));
        assert_eq!(collision.count(), 1);
        assert_eq!(collision.get("shared"), Some(&7.into()));
        let padding = &keyed[0];
        let no_padding = keyed.fresh().await.unwrap().pad(2, padding).unwrap();
        assert!(no_padding.get(7).is_some());
        let padded = keyed.fresh().await.unwrap().pad(-4, padding).unwrap();
        assert_eq!(padded.count(), 4);
        assert_eq!(padded.get(2).unwrap().get_key(), Some(&2.into()));
        assert!(padded.get("last").is_some());
        let right = keyed.fresh().await.unwrap().pad(3, padding).unwrap();
        assert_eq!(right.get(1).unwrap().get_key(), Some(&3.into()));
        let flat = keyed.fresh().await.unwrap().flatten();
        assert_eq!(flat.get(1).unwrap().get_key(), Some(&2.into()));
        assert!(flat.get("last").is_none());
        assert_eq!(
            fresh
                .map_models(|model, _| model)
                .to_base()
                .get(7)
                .unwrap()
                .get_key(),
            Some(&2.into())
        );
        assert!(empty.map_models(|model, _| model).is_empty());
        assert!(std::ptr::eq(
            authors.find_or_fail(&"2".into()).unwrap(),
            &authors[1]
        ));
        assert!(!authors.doesnt_contain(&2.into()).unwrap());
        assert!(authors.doesnt_contain(&20.into()).unwrap());
        for error in [
            authors.find_or_fail(&20.into()).err(),
            authors.find_many_or_fail(&[1.into(), 20.into()]).err(),
            authors.find_many_or_fail(&[Value::Null]).err(),
            Collection::<Author>::new(vec![])
                .find_or_fail(&1.into())
                .err(),
        ] {
            assert!(matches!(
                error,
                Some(DatabaseError::ModelNotFound("relation_authors"))
            ));
        }
        let found_keys = |models: Vec<&Author>| {
            models
                .into_iter()
                .map(|author| author.get_key().unwrap().clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            found_keys(
                authors
                    .find_many(&[3.into(), 20.into(), "1".into()])
                    .unwrap()
            ),
            vec![1.into(), 3.into()]
        );
        assert_eq!(
            found_keys(
                authors
                    .find_many_or_fail(&[3.into(), "1".into(), 1.into()])
                    .unwrap()
            ),
            vec![1.into(), 3.into()]
        );
        assert!(authors.find_many_or_fail(&[]).unwrap().is_empty());
        let reloaded = ordered().find_or_fail(2).await.unwrap();
        assert!(authors[1].is(&reloaded));
        assert!(authors.contains_model(&reloaded));
        let mut same_table = StrictAuthor::query_on(db).make(Attributes::new()).unwrap();
        same_table.set_attribute("id", reloaded.get_key().unwrap().clone());
        assert!(authors.contains_model(&same_table));
        same_table.set_attribute("id", "2");
        assert!(authors.doesnt_contain_model(&same_table));
        assert!(std::ptr::eq(
            authors.find_model_or_fail(&reloaded).unwrap(),
            &authors[1]
        ));
        let mut post = Post::query_on(db).make(Attributes::new()).unwrap();
        post.set_attribute("id", reloaded.get_key().unwrap().clone());
        assert!(authors[1].is_not(&post));
        assert!(authors.doesnt_contain_model(&post));
        assert!(std::ptr::eq(
            authors.find_model(&post).unwrap().unwrap(),
            &authors[1]
        ));
        post.set_attribute("id", 20);
        assert!(matches!(
            authors.find_model_or_fail(&post),
            Err(DatabaseError::ModelNotFound("relation_authors"))
        ));
        let mut unbound = Author::new_instance(Attributes::new()).unwrap();
        unbound.set_attribute("id", reloaded.get_key().unwrap().clone());
        assert_ne!(
            unbound.get_connection_name(),
            reloaded.get_connection_name()
        );
        assert!(authors.doesnt_contain_model(&unbound));
        unbound.set_attribute("id", "2");
        assert!(authors.find_model(&unbound).unwrap().is_some());
        let mut visited = Vec::new();
        assert!(authors.contains_where(|author, index| {
            visited.push(index);
            author.get_attribute("name") == Some(&"Second".into())
        }));
        assert_eq!(visited, [0, 1]);
        assert!(authors.doesnt_contain_where(|_, index| index == 20));
        let replacement = || async {
            let mut updated = ordered().find_or_fail(1).await.unwrap();
            updated.set_attribute("name", "Replacement");
            updated
        };
        let merged = ordered()
            .get()
            .await
            .unwrap()
            .merge([replacement().await])
            .unwrap();
        assert_eq!(merged.model_keys(), vec![1.into(), 2.into(), 3.into()]);
        assert_eq!(merged[0].get_attribute("name"), Some(&"Replacement".into()));
        let mut duplicate = ordered()
            .get()
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();
        duplicate.push(replacement().await);
        let duplicate = Collection::new(duplicate);
        assert_eq!(
            found_keys(duplicate.find_many(&[1.into(), 1.into()]).unwrap()),
            vec![1.into(), 1.into()]
        );
        assert!(matches!(
            duplicate.find_many_or_fail(&[1.into(), 20.into()]),
            Err(DatabaseError::ModelNotFound("relation_authors"))
        ));
        let mut dictionary_models: Vec<_> = duplicate
            .iter()
            .map(|model| Author::from_state(model.state().clone()))
            .collect();
        dictionary_models[3].set_attribute("id", "1");
        let mut binary = Author::new_instance(Attributes::new()).unwrap();
        binary.set_attribute("id", Value::Bytes(vec![0, 255]));
        dictionary_models.push(binary);
        dictionary_models.push(Author::new_instance(Attributes::new()).unwrap());
        let dictionary_models = Collection::new(dictionary_models);
        let dictionary = dictionary_models.get_dictionary().unwrap();
        let keys: Vec<_> = dictionary.keys().map(Vec::as_slice).collect();
        assert_eq!(keys, [&b"1"[..], &b"2"[..], &b"3"[..], &[0, 255]]);
        assert!(std::ptr::eq(
            dictionary[b"1".as_slice()],
            &dictionary_models[3]
        ));
        assert_eq!(
            dictionary[b"1".as_slice()].get_attribute("name"),
            Some(&"Replacement".into())
        );
        assert!(
            Collection::<Author>::new(vec![])
                .get_dictionary()
                .unwrap()
                .is_empty()
        );

        let projected = duplicate.duplicates_by(|author| author.get_key().cloned());
        assert_eq!(projected.get(3), Some(&Some(1.into())));
        assert_eq!(projected.count(), 1);
        let fresh = duplicate.fresh().await.unwrap();
        let strict = fresh.duplicates_strict();
        assert_eq!(strict.keys().get(0), Some(&3.into()));
        let unique_by = duplicate.fresh().await.unwrap().unique_by(|author, _| {
            author.get_key().and_then(|key| match key {
                Value::Integer(key) => Some(*key),
                _ => None,
            })
        });
        assert_eq!(unique_by.model_keys(), [1.into(), 2.into(), 3.into()]);
        let counts = duplicate.count_by(|author, _| author.get_key() == Some(&1.into()));
        assert_eq!(counts.get(true), Some(&2));
        assert_eq!(counts.get(false), Some(&2));
        let (duplicates, rest) =
            duplicate.partition(|author, _| author.get_key() == Some(&1.into()));
        assert_eq!(duplicates.keys().get(1), Some(&3.into()));
        assert_eq!(rest.keys().get(0), Some(&1.into()));
        let distinct = duplicates.merge(rest).unwrap().unique().unwrap();
        assert_eq!(distinct.model_keys(), vec![1.into(), 2.into(), 3.into()]);
        assert_eq!(
            distinct[0].get_attribute("name"),
            Some(&"Replacement".into())
        );
        let keys = ordered()
            .get()
            .await
            .unwrap()
            .only(&[2.into(), 3.into()])
            .unwrap();
        assert_eq!(
            ordered()
                .get()
                .await
                .unwrap()
                .diff(&keys)
                .unwrap()
                .model_keys(),
            vec![1.into()]
        );
        assert_eq!(
            ordered()
                .get()
                .await
                .unwrap()
                .intersect(&keys)
                .unwrap()
                .model_keys(),
            vec![2.into(), 3.into()]
        );
        assert_eq!(
            keys.to_query()
                .unwrap()
                .order_by("id", "asc")
                .get()
                .await
                .unwrap()
                .model_keys(),
            vec![2.into(), 3.into()]
        );
        assert!(Collection::<Author>::new(Vec::new()).to_query().is_err());
        let mut outside = Author::query_on(other).make(Attributes::new()).unwrap();
        outside.set_attribute("id", 2);
        assert!(
            Collection::new(vec![ordered().find_or_fail(1).await.unwrap(), outside])
                .to_query()
                .is_err()
        );
    })
}

fn verify_collection_reload(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let ordered = || Author::query_on(db).order_by("id", "desc");
        let mut missing = Author::query_on(db)
            .create(Attributes::from([("name".into(), "Temporary".into())]))
            .await
            .unwrap();
        let missing_key = missing.get_key().unwrap().clone();
        let mut models = ordered()
            .get()
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();
        let mut dirty = ordered().find_or_fail(1).await.unwrap();
        dirty.set_attribute("name", "Edited locally");
        models.push(dirty);
        missing.delete().await.unwrap();

        let mut stale = Collection::new(models);
        assert!(matches!(
            stale.load_count("posts", &POSTS).await,
            Err(openrust::database::DatabaseError::ModelNotFound(
                "relation_authors"
            ))
        ));
        assert!(
            stale
                .iter()
                .all(|model| model.get_attribute("posts_count").is_none())
        );
        let baseline = if mysql { select_count(db).await } else { 0 };
        let mut refreshed = stale.fresh_with("posts", &POSTS).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        assert_eq!(stale.model_keys()[0], missing_key);
        assert_eq!(
            stale.last().unwrap().get_attribute("name"),
            Some(&"Edited locally".into())
        );
        assert_eq!(
            refreshed.model_keys(),
            vec![3.into(), 2.into(), 1.into(), 1.into()]
        );
        assert_eq!(
            refreshed[2].get_attribute("name"),
            refreshed[3].get_attribute("name")
        );
        assert!(refreshed.iter().all(|model| model.relation_loaded("posts")));

        let baseline = if mysql { select_count(db).await } else { 0 };
        refreshed.load_count("posts", &POSTS).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        let baseline = if mysql { select_count(db).await } else { 0 };
        refreshed
            .load_aggregates(&[
                RelationAggregateQuery::new("posts", &POSTS, RelationAggregate::Count),
                RelationAggregateQuery::new("posts", &POSTS, RelationAggregate::Exists),
                RelationAggregateQuery::new("posts", &POSTS, RelationAggregate::Sum("score")),
                RelationAggregateQuery::new("mentees", &MENTEES, RelationAggregate::Count),
            ])
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        assert!(
            refreshed
                .iter()
                .all(|model| model.get_attribute("mentees_count").is_some())
        );
        assert!(
            refreshed
                .load_aggregates(&[
                    RelationAggregateQuery::new(
                        "posts as collision",
                        &POSTS,
                        RelationAggregate::Count
                    ),
                    RelationAggregateQuery::new(
                        "mentees as COLLISION",
                        &MENTEES,
                        RelationAggregate::Exists
                    ),
                ])
                .await
                .is_err()
        );
        assert!(
            refreshed
                .iter()
                .all(|model| model.get_attribute("collision").is_none())
        );
        refreshed.load_avg("posts", &POSTS, "score").await.unwrap();
        refreshed.load_min("posts", &POSTS, "score").await.unwrap();
        refreshed
            .load_max("posts as top_score", &POSTS, "score")
            .await
            .unwrap();
        assert_eq!(refreshed[0].get_attribute("posts_count"), Some(&0.into()));
        assert_eq!(
            refreshed[0].get_attribute("posts_exists"),
            Some(&false.into())
        );
        for attribute in [
            "posts_sum_score",
            "posts_avg_score",
            "posts_min_score",
            "top_score",
        ] {
            assert_eq!(refreshed[0].get_attribute(attribute), Some(&Value::Null));
        }
        assert_eq!(
            refreshed[2].get_attribute("posts_exists"),
            Some(&true.into())
        );
        assert_eq!(
            refreshed[2].state().get_attributes(),
            refreshed[3].state().get_attributes()
        );
        assert!(refreshed.iter().all(|model| !model.state().is_dirty()));
        assert!(refreshed.load_count("posts as id", &POSTS).await.is_err());

        let mut modified = ordered().find_or_fail(1).await.unwrap();
        modified.set_attribute("name", "Pending");
        let mut modified = Collection::new(vec![modified]);
        modified.load_count("posts", &POSTS).await.unwrap();
        assert_eq!(
            modified[0].state().get_dirty(),
            Attributes::from([("name".into(), "Pending".into())])
        );

        let mut single = ordered().find_or_fail(1).await.unwrap();
        single.set_attribute("name", "Pending model edit");
        let baseline = if mysql { select_count(db).await } else { 0 };
        single
            .load_aggregates(&[
                RelationAggregateQuery::new("posts", &POSTS, RelationAggregate::Count),
                RelationAggregateQuery::new("posts", &POSTS, RelationAggregate::Exists),
            ])
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        assert_eq!(single.get_attribute("posts_exists"), Some(&true.into()));
        assert_eq!(
            single.get_raw_original("posts_count"),
            refreshed[2].get_attribute("posts_count")
        );
        assert_eq!(
            single.state().get_dirty(),
            Attributes::from([("name".into(), "Pending model edit".into())])
        );
        single.load_count("posts", &POSTS).await.unwrap();
        single.load_exists("posts", &POSTS).await.unwrap();
        single.load_sum("posts", &POSTS, "score").await.unwrap();
        single.load_avg("posts", &POSTS, "score").await.unwrap();
        single.load_min("posts", &POSTS, "score").await.unwrap();
        single
            .load_max("posts as top_score", &POSTS, "score")
            .await
            .unwrap();
        assert_eq!(
            single.get_attribute("posts_sum_score"),
            refreshed[2].get_attribute("posts_sum_score")
        );
        assert_eq!(
            single.get_attribute("posts_avg_score"),
            refreshed[2].get_attribute("posts_avg_score")
        );
        assert_eq!(
            single.get_attribute("posts_min_score"),
            refreshed[2].get_attribute("posts_min_score")
        );
        assert_eq!(
            single.get_attribute("top_score"),
            refreshed[2].get_attribute("top_score")
        );
        let mut stale_model = Author::from_state(stale[0].state().clone());
        assert!(matches!(
            stale_model.load_count("posts", &POSTS).await,
            Err(DatabaseError::ModelNotFound("relation_authors"))
        ));
        assert!(stale_model.get_attribute("posts_count").is_none());

        let mut empty = Collection::<Author>::new(Vec::new());
        assert!(empty.fresh().await.unwrap().is_empty());
        empty.load_count("posts", &POSTS).await.unwrap();
        let mut unsaved = ordered().make(Attributes::new()).unwrap();
        unsaved.set_attribute("id", 1);
        assert!(
            Collection::new(vec![unsaved])
                .fresh()
                .await
                .unwrap()
                .is_empty()
        );

        let mut trashed = Post::query_on(db).only_trashed().get().await.unwrap();
        assert!(!trashed.is_empty());
        assert_eq!(
            trashed.fresh().await.unwrap().model_keys(),
            trashed.model_keys()
        );
        trashed
            .load_count("comments", &POST_COMMENTS)
            .await
            .unwrap();
        assert!(
            trashed
                .iter()
                .all(|post| post.get_attribute("comments_count").is_some())
        );
    })
}

async fn verify_pivot_constraints(
    db: &Connection,
    first: &Author,
    first_key: &Value,
    second_key: &Value,
) {
    let ids = |posts: RelatedCollection<Post>| {
        posts
            .iter()
            .map(|post| post.get_key().unwrap().clone())
            .collect::<Vec<_>>()
    };
    let selected = FEATURED_POSTS
        .with_pivot_value("label", "selected")
        .unwrap();
    assert!(
        FEATURED_POSTS
            .with_pivot_value("label", Value::Null)
            .is_err()
    );
    assert!(FEATURED_POSTS.with_pivot_value("AUTHOR_ID", 5).is_err());
    selected
        .attach(first, std::slice::from_ref(first_key))
        .await
        .unwrap();
    let changed = selected
        .sync_without_detaching(first, &[first_key.clone(), second_key.clone()])
        .await
        .unwrap();
    assert_eq!(changed.attached, vec![second_key.clone()]);
    assert_eq!(selected.get_results(first).await.unwrap().len(), 2);
    let updated = selected
        .update_existing_pivot(
            first,
            second_key,
            Attributes::from([("label".into(), "hidden".into())]),
        )
        .await
        .unwrap();
    assert_eq!(updated, 1);
    assert_eq!(
        ids(selected.get_results(first).await.unwrap()),
        vec![first_key.clone()]
    );
    assert_eq!(
        selected
            .update_existing_pivot(
                first,
                second_key,
                Attributes::from([("label".into(), "ignored".into())]),
            )
            .await
            .unwrap(),
        0
    );
    let alternatives = FEATURED_POSTS
        .where_pivot_in("label", vec!["selected".into()])
        .or_where_pivot_null("label");
    assert_eq!(
        ids(alternatives.get_results(first).await.unwrap()),
        vec![first_key.clone()]
    );
    assert_eq!(
        Author::query_on(db)
            .has(&alternatives)
            .unwrap()
            .get()
            .await
            .unwrap()
            .len(),
        2
    );
    let hidden = FEATURED_POSTS
        .where_pivot_not_in("label", vec!["selected".into()])
        .where_pivot_between("label", ["a".into(), "m".into()])
        .where_pivot_not_between("label", ["a".into(), "f".into()])
        .where_pivot_not_null("label");
    let owners = Author::query_on(db)
        .with("filtered", hidden)
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(
        ids(owners[0]
            .get_relation::<RelatedCollection<Post>>("filtered")
            .unwrap()
            .clone()),
        vec![second_key.clone()]
    );
    assert!(
        owners[1]
            .get_relation::<RelatedCollection<Post>>("filtered")
            .unwrap()
            .is_empty()
    );
    let selected_labels = |conditions: openrust::database::query::Conditions| {
        conditions.where_nested(|nested| {
            nested
                .r#where("label", "=", "selected")
                .or_where("label", "=", "grouped")
        })
    };
    let grouped = FEATURED_POSTS.where_pivot_with(|conditions| {
        selected_labels(conditions).where_column("author_id", "=", "post_id")
    });
    let alternatives = FEATURED_POSTS
        .where_pivot("label", "=", "never")
        .or_where_pivot_with(selected_labels);
    for relation in [&grouped, &alternatives] {
        assert_eq!(
            ids(relation.get_results(first).await.unwrap()).as_slice(),
            std::slice::from_ref(first_key)
        );
    }
    assert_eq!(
        grouped
            .update_existing_pivot(
                first,
                second_key,
                Attributes::from([("label".into(), "out of scope".into())]),
            )
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        grouped
            .update_existing_pivot(
                first,
                first_key,
                Attributes::from([("label".into(), "grouped".into())]),
            )
            .await
            .unwrap(),
        1
    );
    let changed = alternatives.sync(first, &[]).await.unwrap();
    assert_eq!(changed.detached, vec![first_key.clone()]);
    assert_eq!(grouped.detach(first, None).await.unwrap(), 0);
    assert_eq!(
        FEATURED_POSTS
            .where_pivot_with(|group| group.r#where("label", "=", "hidden"))
            .detach(first, None)
            .await
            .unwrap(),
        1
    );
    selected
        .attach_with(
            first,
            std::slice::from_ref(second_key),
            Attributes::from([("label".into(), "hidden".into())]),
        )
        .await
        .unwrap();
    selected
        .attach_with(
            first,
            std::slice::from_ref(first_key),
            Attributes::from([("label".into(), "explicit".into())]),
        )
        .await
        .unwrap();
    assert!(selected.get_results(first).await.unwrap().is_empty());
    let constrained = FEATURED_POSTS.where_pivot("label", "=", "selected");
    assert_eq!(constrained.detach(first, None).await.unwrap(), 0);
    assert_eq!(FEATURED_POSTS.detach(first, None).await.unwrap(), 2);
    constrained
        .attach(first, std::slice::from_ref(first_key))
        .await
        .unwrap();
    assert!(constrained.get_results(first).await.unwrap().is_empty());
    assert_eq!(FEATURED_POSTS.detach(first, None).await.unwrap(), 1);
}

async fn verify_pivot_timestamps(db: &Connection, owner: &Author, first: &Value, second: &Value) {
    const EARLIER: &str = "2000-01-01 00:00:00";
    const LATER: &str = "2001-01-01 00:00:00";
    let relation = FEATURED_POSTS
        .with_timestamps()
        .order_by_pivot_desc("created_at");
    for (key, value) in [(first, EARLIER), (second, LATER)] {
        relation
            .attach_with(
                owner,
                std::slice::from_ref(key),
                Attributes::from([
                    ("created_at".into(), value.into()),
                    ("updated_at".into(), EARLIER.into()),
                ]),
            )
            .await
            .unwrap();
    }
    let posts = relation.get_results(owner).await.unwrap();
    assert_eq!(posts[0].get_key(), Some(second));
    let eager = Author::query_on(db)
        .with(
            "ordered",
            FEATURED_POSTS
                .with_timestamps()
                .order_by_pivot("relation_featured_posts.created_at", "asc"),
        )
        .find_or_fail(owner.get_key().unwrap().clone())
        .await
        .unwrap();
    let posts = eager
        .get_relation::<RelatedCollection<Post>>("ordered")
        .unwrap();
    assert_eq!(posts[0].get_key(), Some(first));
    assert_eq!(
        relation
            .update_existing_pivot(owner, first, Attributes::new())
            .await
            .unwrap(),
        1
    );
    let report = relation
        .sync_with_pivot_attributes(
            owner,
            &[(
                second.clone(),
                Attributes::from([
                    ("label".into(), "changed".into()),
                    ("updated_at".into(), EARLIER.into()),
                ]),
            )],
            false,
        )
        .await
        .unwrap();
    assert_eq!(report.updated, vec![second.clone()]);
    let posts = relation.get_results(owner).await.unwrap();
    for (post, expected) in posts.iter().zip([LATER, EARLIER]) {
        let pivot = post.get_relation::<Pivot>("pivot").unwrap();
        assert_eq!(pivot.get_attribute("created_at"), Some(&expected.into()));
        assert!(
            matches!(pivot.get_attribute("updated_at"), Some(Value::Text(value)) if value.as_str() > LATER)
        );
    }
    assert_eq!(relation.detach(owner, None).await.unwrap(), 2);
    relation
        .attach(owner, std::slice::from_ref(first))
        .await
        .unwrap();
    let posts = relation.get_results(owner).await.unwrap();
    let pivot = posts[0].get_relation::<Pivot>("pivot").unwrap();
    let created = pivot.get_attribute("created_at");
    assert!(matches!(created, Some(Value::Text(value)) if value.as_str() > LATER));
    assert_eq!(created, pivot.get_attribute("updated_at"));
    assert_eq!(relation.detach(owner, None).await.unwrap(), 1);

    let custom = FEATURED_POSTS
        .with_timestamp_columns(None, Some("linked_at"))
        .unwrap();
    assert!(
        FEATURED_POSTS
            .with_timestamp_columns(Some("author_id"), None)
            .is_err()
    );
    let report = custom
        .sync_without_detaching(owner, std::slice::from_ref(first))
        .await
        .unwrap();
    assert_eq!(report.attached, vec![first.clone()]);
    let posts = custom.get_results(owner).await.unwrap();
    let pivot = posts[0].get_relation::<Pivot>("pivot").unwrap();
    assert!(
        matches!(pivot.get_attribute("linked_at"), Some(Value::Text(value)) if value.as_str() > LATER)
    );
    assert_eq!(pivot.get_attribute("created_at"), None);
    assert_eq!(custom.detach(owner, None).await.unwrap(), 1);
}

enum PostLinkKey {
    Editor,
}

impl CastableEnum for PostLinkKey {
    fn from_value(value: &Value) -> DatabaseResult<Self> {
        match value {
            Value::Integer(1) => Ok(Self::Editor),
            _ => Err(DatabaseError::InvalidQuery("post link key")),
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Editor => 1.into(),
        }
    }
}

struct TextPivotArticle(ModelState);

impl Model for TextPivotArticle {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("relation_text_articles")
            .incrementing(false)
            .timestamps(None, None);
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

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("id", AttributeCast::String)]
    }
}

struct CreatedOnlyPostLink(ModelState);

impl Model for CreatedOnlyPostLink {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("relation_created_links")
            .incrementing(false)
            .timestamps(Some("created_at"), None)
            .fillable(&["label"]);
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

async fn verify_pivot_key_contracts(db: &Connection, owner: &Author) -> DatabaseResult<()> {
    const UUID: &str = "7b7306ae-5a02-46fa-a84c-9538f45c7dd4";
    const OUTSIDE_I64: &str = "9223372036854775808";
    const ORIGINAL_TIME: &str = "2000-01-01 00:00:00";
    for sql in [
        "DROP TABLE IF EXISTS relation_created_links",
        "DROP TABLE IF EXISTS relation_text_links",
        "DROP TABLE IF EXISTS relation_text_articles",
        "CREATE TABLE relation_created_links (author_id BIGINT NOT NULL, post_id BIGINT NOT NULL, label VARCHAR(80), created_at DATETIME, PRIMARY KEY (author_id, post_id))",
        "CREATE TABLE relation_text_articles (id VARCHAR(64) PRIMARY KEY)",
        "CREATE TABLE relation_text_links (author_id BIGINT NOT NULL, article_id VARCHAR(64) NOT NULL, visible BIGINT DEFAULT 0, PRIMARY KEY (author_id, article_id), FOREIGN KEY (article_id) REFERENCES relation_text_articles(id))",
    ] {
        db.statement(sql, &[]).await?;
    }

    let numbered = BelongsToMany::<Author, Post>::new(
        "relation_created_links",
        "author_id",
        "post_id",
        "id",
        "id",
    )
    .using::<CreatedOnlyPostLink>()
    .with_pivot(["label"])
    .with_timestamp_columns(Some("created_at"), None)?;
    let editor = PostLinkKey::Editor.into_value();
    let sync = numbered.sync(owner, &[editor.clone(), 2.into()]).await?;
    assert_eq!(sync.attached, [Value::Integer(1), Value::Integer(2)]);
    assert!(sync.updated.is_empty() && sync.detached.is_empty());
    assert_eq!(
        numbered
            .toggle(owner, std::slice::from_ref(&editor))
            .await?
            .detached
            .as_slice(),
        std::slice::from_ref(&editor)
    );
    assert_eq!(
        numbered
            .toggle(owner, std::slice::from_ref(&editor))
            .await?
            .attached
            .as_slice(),
        std::slice::from_ref(&editor)
    );
    numbered.detach(owner, None).await?;

    numbered
        .attach_with(
            owner,
            std::slice::from_ref(&editor),
            Attributes::from([("label".into(), "created only".into())]),
        )
        .await?;
    let row = db.table("relation_created_links").first().await?.unwrap();
    assert_eq!(row.get("label"), Some(&"created only".into()));
    assert!(
        matches!(row.get("created_at"), Some(Value::Text(value)) if value.as_str() > ORIGINAL_TIME)
    );
    assert!(row.get("updated_at").is_none());
    db.table("relation_created_links")
        .update(Attributes::from([(
            "created_at".into(),
            ORIGINAL_TIME.into(),
        )]))
        .await?;
    numbered
        .update_existing_pivot(
            owner,
            &editor,
            Attributes::from([("label".into(), "edited".into())]),
        )
        .await?;
    let models = numbered.get_results(owner).await?;
    let pivot = models[0]
        .get_relation::<CreatedOnlyPostLink>("pivot")
        .unwrap();
    assert_eq!(pivot.get_attribute("label"), Some(&"edited".into()));
    assert_eq!(
        pivot.get_attribute("created_at"),
        Some(&ORIGINAL_TIME.into())
    );
    assert!(pivot.get_attribute("updated_at").is_none());

    db.table("relation_text_articles")
        .insert(
            &[UUID, OUTSIDE_I64, "1", "01"]
                .map(|key| Attributes::from([("id".into(), key.into())])),
        )
        .await?;
    let text = BelongsToMany::<Author, TextPivotArticle>::new(
        "relation_text_links",
        "author_id",
        "article_id",
        "id",
        "id",
    )
    .with_pivot(["visible"]);
    let keys = [Value::from(UUID), OUTSIDE_I64.into(), 1.into(), "01".into()];
    let changes = text.sync(owner, &keys).await?;
    assert_eq!(
        changes.attached,
        [UUID, OUTSIDE_I64, "1", "01"].map(Value::from)
    );
    for model in text.get_results(owner).await? {
        let pivot = model.get_relation::<Pivot>("pivot").unwrap();
        assert_eq!(pivot.get_attribute("visible"), Some(&0.into()));
    }
    let changes = text
        .sync_with_pivot_values(
            owner,
            &keys,
            Attributes::from([("visible".into(), 1.into())]),
            true,
        )
        .await?;
    assert_eq!(
        changes.updated,
        [UUID, OUTSIDE_I64, "1", "01"].map(Value::from)
    );
    let changes = text.toggle(owner, &[1.into()]).await?;
    assert_eq!(changes.detached, [Value::from("1")]);
    let remaining = text.get_results(owner).await?;
    assert_eq!(remaining.len(), 3);
    assert!(
        remaining
            .iter()
            .any(|article| article.get_key() == Some(&"01".into()))
    );
    for model in remaining {
        assert_eq!(
            model
                .get_relation::<Pivot>("pivot")
                .unwrap()
                .get_attribute("visible"),
            Some(&1.into())
        );
    }
    Ok(())
}

struct Membership(ModelState);

impl Model for Membership {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("unused_memberships")
            .incrementing(false)
            .timestamps(None, None)
            .fillable(&["label"]);
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
    fn pivot_inverse_relations() -> &'static [&'static str] {
        &["author", "post", "owner", "item"]
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("label", AttributeCast::Integer)]
    }
}

struct MembershipEvents;

impl openrust::database::eloquent::concerns::ModelEventListener<Membership> for MembershipEvents {
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        _: &'a mut Membership,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        Box::pin(async { panic!("relationship writes must dispatch inside their transaction") })
    }

    fn handle_in_transaction<'a>(
        &'a self,
        event: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut Membership,
        transaction: &'a mut openrust::database::Transaction,
    ) -> openrust::database::DatabaseFuture<
        'a,
        DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        use openrust::database::eloquent::concerns::{ModelEvent::*, ModelEventResponse::*};
        Box::pin(async move {
            let label = model.get_attribute_value("label")?;
            if matches!(event, Saving | Deleting) && label == Some(99.into()) {
                return Ok(Cancel);
            }
            if matches!(event, Created | Updated | Deleted) {
                transaction
                    .statement(
                        "INSERT INTO relation_membership_events (event) VALUES (?)",
                        &[format!("{event:?}").into()],
                    )
                    .await?;
                if (event == Created && label == Some(77.into()))
                    || (matches!(event, Updated | Deleted) && label == Some(88.into()))
                {
                    return Err(DatabaseError::InvalidQuery("reject pivot after writing"));
                }
            }
            Ok(Continue)
        })
    }
}

struct TouchAuthor(ModelState);

impl Model for TouchAuthor {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata =
            ModelMetadata::new("relation_authors").timestamps(None, Some("updated_at"));
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
}

async fn verify_toggle_and_touch(db: &Connection) {
    const OLD: &str = "2000-01-01 00:00:00";
    let mut parent = TouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
    parent.set_attribute("name", "unsaved");
    let relation = BelongsToMany::<TouchAuthor, Post>::new(
        "relation_featured_posts",
        "author_id",
        "post_id",
        "id",
        "id",
    )
    .with_touching_parent();
    assert!(
        relation
            .toggle(&parent, &[])
            .await
            .unwrap()
            .attached
            .is_empty()
    );
    let old = TouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
    assert_eq!(old.get_attribute("updated_at"), Some(&OLD.into()));
    assert!(
        relation
            .toggle_or_fail(&parent, &[2.into(), Value::Null])
            .await
            .is_err()
    );
    let result = relation
        .toggle_with_pivot_attributes(
            &parent,
            &[
                (
                    1.into(),
                    Attributes::from([("label".into(), "toggle".into())]),
                ),
                (2.into(), Attributes::new()),
            ],
        )
        .await
        .unwrap();
    assert_eq!(result.attached, vec![1.into()]);
    assert_eq!(result.detached, vec![2.into()]);
    let posts = relation
        .with_pivot(["label"])
        .get_results(&parent)
        .await
        .unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].get_key(), Some(&1.into()));
    assert_eq!(
        posts[0]
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"toggle".into())
    );
    let saved = TouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
    assert_eq!(saved.get_attribute("name"), Some(&"first".into()));
    assert!(
        matches!(saved.get_attribute("updated_at"), Some(Value::Text(time)) if time.as_str() > OLD)
    );
    let relation = BelongsToMany::<TouchAuthor, Post>::new(
        "relation_featured_posts",
        "author_id",
        "post_id",
        "id",
        "id",
    )
    .with_touching_parent();
    for action in ["attach", "update", "detach", "sync"] {
        db.table("relation_authors")
            .r#where("id", "=", 1)
            .update(Attributes::from([("updated_at".into(), OLD.into())]))
            .await
            .unwrap();
        let parent = TouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
        match action {
            "attach" => relation.attach(&parent, &[2.into()]).await.unwrap(),
            "update" => assert_eq!(
                relation
                    .update_existing_pivot(
                        &parent,
                        &2.into(),
                        Attributes::from([("label".into(), "updated".into())])
                    )
                    .await
                    .unwrap(),
                1
            ),
            "detach" => assert_eq!(
                relation.detach(&parent, Some(&[2.into()])).await.unwrap(),
                1
            ),
            "sync" => assert_eq!(
                relation.sync(&parent, &[2.into()]).await.unwrap().attached,
                vec![2.into()]
            ),
            _ => unreachable!(),
        }
        let fresh = TouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
        assert!(
            matches!(fresh.get_attribute("updated_at"), Some(Value::Text(time)) if time.as_str() > OLD),
            "{action}"
        );
    }
    let other = TouchAuthor::query_on(db).find_or_fail(2).await.unwrap();
    assert_eq!(other.get_attribute("updated_at"), Some(&OLD.into()));
}

struct SaveTouchAuthor(ModelState);

impl Model for SaveTouchAuthor {
    fn metadata() -> &'static ModelMetadata {
        TouchAuthor::metadata()
    }

    fn touches() -> Touches<Self> {
        Touches::default()
            .belongs_to_many(
                BelongsToMany::<Self, TouchPost>::new(
                    "relation_featured_posts",
                    "author_id",
                    "post_id",
                    "id",
                    "id",
                )
                .where_pivot("label", "=", "eligible"),
            )
            .morph_to_many(MorphToMany::<Self, TouchPost>::new(
                "relation_morph_memberships",
                "subject_id",
                "tag_id",
                "id",
                "id",
                "subject_type",
                "featured",
            ))
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

struct TouchPost(ModelState);

impl Model for TouchPost {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_posts")
            .timestamps(None, Some("updated_at"))
            .soft_deletes("deleted_at");
        &METADATA
    }

    fn touches() -> Touches<Self> {
        Touches::default().belongs_to(BelongsTo::<Self, SaveTouchAuthor>::new("author_id", "id"))
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

async fn verify_related_touch(db: &Connection) {
    const OLD: &str = "2000-01-01 00:00:00";
    let parent = TouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
    let relation = || {
        BelongsToMany::<TouchAuthor, TouchPost>::new(
            "relation_featured_posts",
            "author_id",
            "post_id",
            "id",
            "id",
        )
    };
    db.statement(
        "DELETE FROM relation_featured_posts WHERE author_id = 1",
        &[],
    )
    .await
    .unwrap();
    relation().attach(&parent, &[1.into()]).await.unwrap();
    let timestamp = "SELECT updated_at FROM relation_posts WHERE id = ?";
    assert_eq!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());

    let touching = relation().with_touching_related();
    for action in ["attach", "update", "detach", "sync", "toggle"] {
        db.statement("UPDATE relation_posts SET updated_at = ?", &[OLD.into()])
            .await
            .unwrap();
        match action {
            "attach" => touching.attach(&parent, &[2.into()]).await.unwrap(),
            "update" => {
                touching
                    .update_existing_pivot(
                        &parent,
                        &2.into(),
                        Attributes::from([("label".into(), "updated".into())]),
                    )
                    .await
                    .unwrap();
            }
            "detach" => {
                touching.detach(&parent, Some(&[2.into()])).await.unwrap();
            }
            "sync" => {
                touching.sync(&parent, &[1.into(), 2.into()]).await.unwrap();
            }
            "toggle" => {
                touching.toggle(&parent, &[2.into()]).await.unwrap();
            }
            _ => unreachable!(),
        }
        assert_ne!(
            db.scalar(timestamp, &[1.into()]).await.unwrap(),
            OLD.into(),
            "{action}"
        );
        if matches!(action, "detach" | "toggle") {
            assert_eq!(
                db.scalar(timestamp, &[2.into()]).await.unwrap(),
                OLD.into(),
                "{action}"
            );
        } else {
            assert_ne!(
                db.scalar(timestamp, &[2.into()]).await.unwrap(),
                OLD.into(),
                "{action}"
            );
        }
    }

    db.statement("UPDATE relation_posts SET updated_at = ?", &[OLD.into()])
        .await
        .unwrap();
    touching.sync(&parent, &[1.into()]).await.unwrap();
    assert_eq!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());

    relation()
        .attach(&parent, &[2.into(), 3.into()])
        .await
        .unwrap();
    db.statement("UPDATE relation_featured_posts SET label = 'eligible' WHERE author_id = 1 AND post_id IN (1, 3)", &[]).await.unwrap();
    relation()
        .where_pivot("label", "=", "eligible")
        .touch(&parent)
        .await
        .unwrap();
    assert_ne!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_eq!(db.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());
    assert_eq!(db.scalar(timestamp, &[3.into()]).await.unwrap(), OLD.into());

    db.statement(
        "INSERT INTO relation_morph_memberships VALUES ('author',1,1,'other')",
        &[],
    )
    .await
    .unwrap();
    db.statement("UPDATE relation_posts SET updated_at = ?", &[OLD.into()])
        .await
        .unwrap();
    let morph = MorphToMany::<TouchAuthor, TouchPost>::new(
        "relation_morph_memberships",
        "subject_id",
        "tag_id",
        "id",
        "id",
        "subject_type",
        "featured",
    )
    .with_touching_related();
    morph.attach(&parent, &[2.into()]).await.unwrap();
    assert_eq!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_ne!(db.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());

    assert!(touching.attach(&parent, &[4.into()]).await.is_err());
    assert_eq!(
        db.table("relation_featured_posts")
            .r#where("author_id", "=", 1)
            .r#where("post_id", "=", 4)
            .count("*")
            .await
            .unwrap(),
        0
    );
}

async fn verify_save_touches_related(db: &Connection) {
    const OLD: &str = "2000-01-01 00:00:00";
    let timestamp = "SELECT updated_at FROM relation_posts WHERE id = ?";
    let author_timestamp = "SELECT updated_at FROM relation_authors WHERE id = 2";
    db.statement("UPDATE relation_posts SET updated_at = ?", &[OLD.into()])
        .await
        .unwrap();
    db.statement("UPDATE relation_posts SET author_id = 2 WHERE id = 2", &[])
        .await
        .unwrap();
    let mut parent = SaveTouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
    parent.save().await.unwrap();
    assert_eq!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_eq!(db.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());

    let mut tx = db.begin_transaction().await.unwrap();
    parent.set_attribute("name", "touch rolled back");
    parent.save_in_transaction(&mut tx).await.unwrap();
    assert_ne!(tx.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_ne!(tx.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());
    assert_eq!(tx.scalar(timestamp, &[3.into()]).await.unwrap(), OLD.into());
    assert_ne!(tx.scalar(author_timestamp, &[]).await.unwrap(), OLD.into());
    tx.roll_back(None).await.unwrap();
    for post in [1, 2, 3] {
        assert_eq!(
            db.scalar(timestamp, &[post.into()]).await.unwrap(),
            OLD.into()
        );
    }
    assert_eq!(db.scalar(author_timestamp, &[]).await.unwrap(), OLD.into());

    let mut parent = SaveTouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
    parent.set_attribute("name", "touch committed");
    parent.save().await.unwrap();
    assert_ne!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_ne!(db.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());
    assert_ne!(db.scalar(author_timestamp, &[]).await.unwrap(), OLD.into());
}

async fn verify_inferred_pivot_touch(db: &Connection) {
    const OLD: &str = "2000-01-01 00:00:00";
    let parent = SaveTouchAuthor::query_on(db).find_or_fail(1).await.unwrap();
    let timestamp = "SELECT updated_at FROM relation_posts WHERE id = ?";
    let reset = "UPDATE relation_posts SET updated_at = ?";
    db.statement(reset, &[OLD.into()]).await.unwrap();

    let relation = || {
        BelongsToMany::<SaveTouchAuthor, TouchPost>::new(
            "relation_featured_posts",
            "author_id",
            "post_id",
            "id",
            "id",
        )
    };
    relation()
        .where_pivot("label", "=", "unmatched")
        .attach_with(
            &parent,
            &[4.into()],
            Attributes::from([("label".into(), "unmatched".into())]),
        )
        .await
        .unwrap();
    assert_eq!(db.scalar(timestamp, &[4.into()]).await.unwrap(), OLD.into());
    relation().detach(&parent, Some(&[4.into()])).await.unwrap();

    let filtered = relation().where_pivot("label", "=", "eligible");
    assert!(
        filtered
            .attach_with(
                &parent,
                &[4.into()],
                Attributes::from([("label".into(), "eligible".into())]),
            )
            .await
            .is_err()
    );
    assert_eq!(
        db.scalar(
            "SELECT COUNT(*) FROM relation_featured_posts WHERE author_id = 1 AND post_id = 4",
            &[]
        )
        .await
        .unwrap(),
        0.into()
    );
    filtered.detach(&parent, Some(&[1.into()])).await.unwrap();
    filtered
        .attach_with(
            &parent,
            &[1.into()],
            Attributes::from([("label".into(), "eligible".into())]),
        )
        .await
        .unwrap();
    assert_ne!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_eq!(db.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());

    db.statement(reset, &[OLD.into()]).await.unwrap();
    let post = TouchPost::query_on(db).find_or_fail(1).await.unwrap();
    let inverse = BelongsToMany::<TouchPost, SaveTouchAuthor>::new(
        "relation_featured_posts",
        "post_id",
        "author_id",
        "id",
        "id",
    )
    .where_pivot("label", "=", "eligible");
    assert_eq!(inverse.detach(&post, Some(&[1.into()])).await.unwrap(), 1);
    assert_ne!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());

    db.statement(reset, &[OLD.into()]).await.unwrap();
    let morph = |alias| {
        MorphToMany::<SaveTouchAuthor, TouchPost>::new(
            "relation_morph_memberships",
            "subject_id",
            "tag_id",
            "id",
            "id",
            "subject_type",
            alias,
        )
    };
    morph("different")
        .attach(&parent, &[1.into()])
        .await
        .unwrap();
    assert_eq!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    morph("featured")
        .attach(&parent, &[1.into()])
        .await
        .unwrap();
    assert_ne!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
}

async fn verify_pivot_reload(pivot: &Membership) {
    let mut refreshed = Membership::from_state(pivot.state().clone());
    let fresh = refreshed.fresh().await.unwrap().unwrap();
    assert_eq!(fresh.get_table(), "relation_featured_posts");
    assert_eq!(fresh.get_attribute_value("label").unwrap(), Some(20.into()));
    refreshed.refresh().await.unwrap();
    assert_eq!(
        refreshed.get_attribute_value("label").unwrap(),
        Some(20.into())
    );
    assert!(!refreshed.state().is_dirty());
}

async fn verify_first_and_update_or_create(db: &Connection) {
    let parent = Author::query_on(db).find_or_fail(1).await.unwrap();
    let relation = FEATURED_POSTS.with_pivot(["label"]);
    let title = |name: &str| Attributes::from([("title".into(), name.into())]);
    let score = |value: i64| Attributes::from([("score".into(), value.into())]);
    let label = |name: &str| Attributes::from([("label".into(), name.into())]);

    let scoped = FEATURED_POSTS.constrain(move |query| query.with_attributes(score(83), true));
    let scoped_post = scoped
        .create(&parent, title("scoped-pivot"), label("scoped"))
        .await
        .unwrap();
    assert_eq!(scoped_post.get_attribute("score"), Some(&83.into()));
    let scoped_force = FEATURED_POSTS
        .constrain(move |query| query.with_attributes(score(84), false))
        .force_create(&parent, title("scoped-pivot-force"), label("scoped"))
        .await
        .unwrap();
    assert_eq!(scoped_force.get_attribute("score"), Some(&84.into()));
    assert_eq!(scoped.get_results(&parent).await.unwrap().len(), 1);
    scoped
        .detach(
            &parent,
            Some(&[
                scoped_post.get_key().unwrap().clone(),
                scoped_force.get_key().unwrap().clone(),
            ]),
        )
        .await
        .unwrap();

    let existing = relation
        .first_or_create(&parent, title("unlinked"), score(70), label("attached"))
        .await
        .unwrap();
    assert_eq!(existing.get_key(), Some(&100.into()));
    assert_eq!(existing.get_attribute("score"), Some(&90.into()));
    assert!(!existing.state().was_recently_created());

    let first = relation
        .first_or_create(&parent, title("unlinked"), score(71), label("ignored"))
        .await
        .unwrap();
    assert_eq!(first.get_attribute("score"), Some(&90.into()));
    assert_eq!(
        first
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"attached".into())
    );

    let updated = relation
        .update_or_create(&parent, title("unlinked"), score(72), label("ignored"))
        .await
        .unwrap();
    assert_eq!(updated.get_key(), first.get_key());
    assert_eq!(updated.get_attribute("score"), Some(&72.into()));
    assert_eq!(
        updated
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"attached".into())
    );

    let created = relation
        .update_or_create(
            &parent,
            title("upsert-created"),
            score(73),
            label("created"),
        )
        .await
        .unwrap();
    assert!(created.state().was_recently_created());
    assert_eq!(created.get_attribute("score"), Some(&73.into()));
    assert_eq!(
        Post::query_on(db)
            .find_or_fail(100)
            .await
            .unwrap()
            .get_attribute("score"),
        Some(&72.into())
    );

    let rejected = relation
        .update_or_create(&parent, title("unlinked"), score(73), label("ignored"))
        .await;
    assert!(matches!(rejected, Err(DatabaseError::Driver { .. })));
    assert_eq!(
        Post::query_on(db)
            .find_or_fail(100)
            .await
            .unwrap()
            .get_attribute("score"),
        Some(&72.into())
    );
}

async fn verify_restore_and_deferred_values(db: &Connection) {
    let parent = Author::query_on(db).find_or_fail(1).await.unwrap();
    let relation = FEATURED_POSTS.with_pivot(["label"]);
    let title = |name: &str| Attributes::from([("title".into(), name.into())]);
    let label = |name: &str| Attributes::from([("label".into(), name.into())]);
    let never = || RelatedValues::deferred(|| panic!("values must remain deferred"));

    db.statement("INSERT INTO relation_posts (title, score, deleted_at) VALUES ('trash-attached', 200, '2025-01-01 00:00:00'), ('trash-unlinked', 201, '2025-01-01 00:00:00')", &[])
        .await.unwrap();
    let mut deleted = Post::query_on(db).only_trashed();
    deleted.r#where("title", "=", "trash-attached");
    let key = deleted
        .first_or_fail()
        .await
        .unwrap()
        .get_key()
        .unwrap()
        .clone();
    relation
        .attach_with(&parent, std::slice::from_ref(&key), label("original"))
        .await
        .unwrap();

    let restored = relation
        .restore_or_create(&parent, title("trash-attached"), never(), label("ignored"))
        .await
        .unwrap();
    assert_eq!(restored.get_key(), Some(&key));
    assert_eq!(
        restored
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"original".into())
    );
    assert!(Post::query_on(db).find(key).await.unwrap().is_some());

    let mut calls = 0;
    let restored = relation
        .create_or_restore(
            &parent,
            title("trash-unlinked"),
            RelatedValues::deferred(|| {
                calls += 1;
                Attributes::new()
            }),
            label("new-link"),
        )
        .await
        .unwrap();
    assert_eq!(calls, 1);
    assert!(!restored.state().was_recently_created());
    assert!(matches!(
        restored.get_attribute("deleted_at"),
        Some(Value::Null)
    ));

    let linked = relation
        .first_or_create(&parent, title("trash-unlinked"), never(), label("ignored"))
        .await
        .unwrap();
    assert_eq!(linked.get_key(), restored.get_key());
    assert_eq!(
        linked
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"new-link".into())
    );

    let mut calls = 0;
    let created = relation
        .restore_or_create(
            &parent,
            title("restore-created"),
            RelatedValues::deferred(|| {
                calls += 1;
                Attributes::from([("score".into(), 202.into())])
            }),
            label("created"),
        )
        .await
        .unwrap();
    assert_eq!(calls, 1);
    assert!(created.state().was_recently_created());
    let mut calls = 0;
    let updated = relation
        .update_or_create(
            &parent,
            title("restore-created"),
            RelatedValues::deferred(|| {
                calls += 1;
                Attributes::from([("score".into(), 203.into())])
            }),
            label("ignored"),
        )
        .await
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(updated.get_key(), created.get_key());
    assert_eq!(updated.get_attribute("score"), Some(&203.into()));

    let unsupported = BelongsToMany::<Author, Author>::new(
        "relation_featured_posts",
        "author_id",
        "post_id",
        "id",
        "id",
    );
    assert!(matches!(
        unsupported
            .restore_or_create(&parent, Attributes::new(), never(), Attributes::new())
            .await,
        Err(DatabaseError::InvalidQuery(_))
    ));
}

async fn verify_increment_or_create(db: &Connection, concurrent: bool) {
    let parent = Author::query_on(db).find_or_fail(1).await.unwrap();
    let relation = FEATURED_POSTS
        .with_pivot(["label"])
        .with_pivot_value("label", "counter")
        .unwrap();
    let attributes = || Attributes::from([("title".into(), "counter-start".into())]);
    let increment =
        |extra| relation.increment_or_create(&parent, attributes(), "score", 300_i64, 2_i64, extra);
    let created = increment(Attributes::new()).await.unwrap();
    assert!(created.state().was_recently_created());
    assert_eq!(created.get_attribute("score"), Some(&300.into()));

    let updated = increment(Attributes::new()).await.unwrap();
    assert!(!updated.state().was_recently_created());
    assert_eq!(updated.get_key(), created.get_key());
    assert_eq!(updated.get_attribute("score"), Some(&302.into()));
    assert_eq!(
        updated
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"counter".into())
    );

    let extra = Attributes::from([("title".into(), "counter-renamed".into())]);
    let updated = increment(extra).await.unwrap();
    assert_eq!(
        updated.get_attribute("title"),
        Some(&"counter-renamed".into())
    );
    assert_eq!(updated.get_attribute("score"), Some(&304.into()));
    if concurrent {
        let attributes = || Attributes::from([("title".into(), "counter-renamed".into())]);
        let increment = || {
            relation.increment_or_create(
                &parent,
                attributes(),
                "score",
                300_i64,
                2_i64,
                Attributes::new(),
            )
        };
        let (a, b) = tokio::join!(increment(), increment());
        let mut scores = [a.unwrap(), b.unwrap()].map(|model| match model.get_attribute("score") {
            Some(Value::Integer(score)) => *score,
            other => panic!("expected integer counter, got {other:?}"),
        });
        scores.sort_unstable();
        assert_eq!(scores, [306, 308]);
    }
    let persisted = Post::query_on(db)
        .find_or_fail(created.get_key().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(
        persisted.get_attribute("score"),
        Some(&if concurrent { 308.into() } else { 304.into() })
    );
    assert!(matches!(
        relation
            .increment_or_create(
                &parent,
                attributes(),
                "score",
                0_i64,
                "invalid",
                Attributes::new()
            )
            .await,
        Err(DatabaseError::InvalidQuery(_))
    ));
}

struct RelationCounter(ModelState);

impl Model for RelationCounter {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_counters")
            .fillable(&["name", "count", "post_id", "owner_type"])
            .timestamps(None, None)
            .soft_deletes("deleted_at");
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
}

struct ScaledCounter(ModelState);

struct CounterScale(i64);

impl openrust::database::eloquent::casts::Castable for CounterScale {
    type Arguments = i64;
    type Caster = Self;

    fn cast_using(scale: &i64) -> DatabaseResult<Self> {
        Ok(Self(*scale))
    }
}

impl openrust::database::eloquent::casts::CastsInboundAttributes for CounterScale {
    fn set(
        &self,
        _: &str,
        value: Value,
        _: &Attributes,
    ) -> DatabaseResult<openrust::database::eloquent::casts::CastWrite> {
        let Value::Integer(value) = value else {
            return Err(DatabaseError::InvalidQuery("scaled counter"));
        };
        let stored = value
            .checked_mul(self.0)
            .ok_or(DatabaseError::InvalidQuery("scaled counter"))?;
        Ok(openrust::database::eloquent::casts::CastWrite::Value(
            stored.into(),
        ))
    }

    fn increment(
        &self,
        name: &str,
        amount: &Value,
        attributes: &Attributes,
    ) -> DatabaseResult<Option<Value>> {
        let (Some(Value::Integer(current)), Value::Integer(amount)) =
            (attributes.get(name), amount)
        else {
            return Err(DatabaseError::InvalidQuery("scaled counter"));
        };
        if *amount == 9 {
            return Err(DatabaseError::InvalidQuery("rejected logical increment"));
        }
        Ok(Some((current / self.0 + amount).into()))
    }

    fn decrement(
        &self,
        name: &str,
        amount: &Value,
        attributes: &Attributes,
    ) -> DatabaseResult<Option<Value>> {
        let (Some(Value::Integer(current)), Value::Integer(amount)) =
            (attributes.get(name), amount)
        else {
            return Err(DatabaseError::InvalidQuery("scaled counter"));
        };
        Ok(Some((current / self.0 - amount).into()))
    }
}

impl openrust::database::eloquent::casts::CastsAttributes for CounterScale {
    fn get(&self, _: &str, value: Option<&Value>, _: &Attributes) -> DatabaseResult<Value> {
        let Some(Value::Integer(value)) = value else {
            return Err(DatabaseError::InvalidQuery("scaled counter"));
        };
        Ok((value / self.0).into())
    }
}

impl Model for ScaledCounter {
    fn metadata() -> &'static ModelMetadata {
        RelationCounter::metadata()
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        use openrust::database::eloquent::casts::ClassCast;
        const CASTS: &[(&str, AttributeCast)] = &[("count", ClassCast::of::<CounterScale>(&100))];
        CASTS
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

async fn verify_cast_counter(db: &Connection, post: &Post, mysql: bool) {
    let relation = HasMany::<Post, ScaledCounter>::new("id", "post_id");
    let increment = |step: i64| {
        relation.increment_or_create(
            post,
            Attributes::from([("name".into(), "scaled".into())]),
            "count",
            3_i64,
            step,
            Attributes::new(),
        )
    };
    let created = increment(2).await.unwrap();
    assert_eq!(created.get_attribute("count"), Some(&300.into()));
    let updated = increment(2).await.unwrap();
    assert_eq!(updated.get_attribute("count"), Some(&500.into()));
    assert_eq!(
        updated.get_attribute_value("count").unwrap(),
        Some(5.into())
    );
    assert!(matches!(
        increment(9).await,
        Err(DatabaseError::InvalidQuery("rejected logical increment"))
    ));
    let current = ScaledCounter::query_on(db)
        .find_or_fail(created.get_key().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(current.get_attribute("count"), Some(&500.into()));
    let mut current = current;
    assert_eq!(
        current
            .decrement("count", 1_i64, Attributes::new())
            .await
            .unwrap(),
        1
    );
    assert_eq!(current.get_attribute("count"), Some(&400.into()));
    assert_eq!(
        current.get_attribute_value("count").unwrap(),
        Some(4.into())
    );
    current
        .increment("count", 1_i64, Attributes::new())
        .await
        .unwrap();

    if mysql {
        let (a, b) = tokio::join!(increment(2), increment(2));
        let values =
            [a.unwrap(), b.unwrap()].map(|model| model.get_attribute_value("count").unwrap());
        assert!(values.contains(&Some(7.into())) && values.contains(&Some(9.into())));
    }
}

async fn verify_non_pivot_lookup(author: &Author, post: &Post) {
    let attrs = |name: &str| Attributes::from([("name".into(), name.into())]);
    let count = |value: i64| Attributes::from([("count".into(), value.into())]);
    let direct = || HasMany::<Post, RelationCounter>::new("id", "post_id");
    let pending = direct()
        .first_or_new(post, attrs("lookup"), count(1))
        .await
        .unwrap();
    assert!(!pending.state().exists());
    assert_eq!(pending.get_attribute("post_id"), post.get_key());
    let first = direct()
        .first_or_create(post, attrs("lookup"), count(2))
        .await
        .unwrap();
    let matched = direct()
        .one()
        .first_or_create(
            post,
            attrs("lookup"),
            RelatedValues::deferred(|| panic!("lookup hit")),
        )
        .await
        .unwrap();
    assert_eq!(first.get_key(), matched.get_key());
    let updated = direct()
        .update_or_create(post, attrs("lookup"), RelatedValues::deferred(|| count(3)))
        .await
        .unwrap();
    assert_eq!(updated.get_attribute("count"), Some(&3.into()));
    let mut conflicting = count(8);
    conflicting.insert("owner_type".into(), "post".into());
    let existing = direct()
        .create_or_first(post, attrs("lookup"), conflicting)
        .await
        .unwrap();
    assert_eq!(existing.get_key(), first.get_key());
    assert_eq!(existing.get_attribute("count"), Some(&3.into()));

    let morph =
        || MorphMany::<Post, RelationCounter>::new("id", "post_id", "owner_type", "lookup-morph");
    let mut created = morph()
        .one()
        .first_or_create(post, attrs("lookup"), count(4))
        .await
        .unwrap();
    assert_ne!(created.get_key(), first.get_key());
    assert_eq!(
        created.get_attribute("owner_type"),
        Some(&"lookup-morph".into())
    );
    created.delete().await.unwrap();
    let restored = morph()
        .restore_or_create(
            post,
            attrs("lookup"),
            RelatedValues::deferred(|| panic!("trashed hit")),
        )
        .await
        .unwrap();
    assert_eq!(restored.get_key(), created.get_key());
    assert_eq!(restored.get_attribute("deleted_at"), Some(&Value::Null));

    let through = || {
        HasManyThrough::<Author, Post, RelationCounter>::new("author_id", "post_id", "id", "id")
            .constrain(|mut query| {
                query.r#where("owner_type", "=", "post");
                query
            })
    };
    let pending = through()
        .first_or_new(author, attrs("through-lookup"), count(5))
        .await
        .unwrap();
    assert!(!pending.state().exists());
    assert!(pending.get_attribute("post_id").is_none());
    let mut create = count(6);
    create.insert("post_id".into(), post.get_key().unwrap().clone());
    create.insert("owner_type".into(), "post".into());
    let mut model = through()
        .first_or_create(author, attrs("through-lookup"), create.clone())
        .await
        .unwrap();
    let matched = through()
        .one()
        .create_or_first(author, attrs("through-lookup"), create.clone())
        .await
        .unwrap();
    assert_eq!(matched.get_key(), model.get_key());
    model.delete().await.unwrap();
    let restored = through()
        .one()
        .create_or_restore(author, attrs("through-lookup"), create)
        .await
        .unwrap();
    assert_eq!(restored.get_key(), model.get_key());
    let updated = through()
        .update_or_create(author, attrs("through-lookup"), count(7))
        .await
        .unwrap();
    assert_eq!(updated.get_attribute("count"), Some(&7.into()));
}

async fn verify_scoped_attributes(db: &Connection, author: &Author, post: &Post) {
    let name = |name: &str| Attributes::from([("name".into(), name.into())]);
    let defaults = |count: i64| Attributes::from([("count".into(), count.into())]);
    let direct = HasMany::<Post, RelationCounter>::new("id", "post_id")
        .constrain(move |query| query.with_attributes(defaults(77), true));
    let created = direct.create(post, name("scoped-create")).await.unwrap();
    assert_eq!(created.get_attribute("count"), Some(&77.into()));
    let mut attributes = name("scoped-override");
    attributes.insert("count".into(), 78.into());
    let overridden = direct.create(post, attributes).await.unwrap();
    assert_eq!(overridden.get_attribute("count"), Some(&78.into()));

    let mut existing = RelationCounter::new_instance_on(db, name("scoped-save")).unwrap();
    assert!(direct.save(post, &mut existing).await.unwrap());
    assert_eq!(existing.get_attribute("count"), Some(&77.into()));
    let made = direct
        .first_or_new(post, name("scoped-new"), Attributes::new())
        .await
        .unwrap();
    assert!(!made.state().exists());
    assert_eq!(made.get_attribute("count"), Some(&77.into()));
    let scoped = direct.get_results(post).await.unwrap();
    assert!(
        scoped
            .iter()
            .any(|model| model.get_key() == created.get_key())
    );
    assert!(
        !scoped
            .iter()
            .any(|model| model.get_key() == overridden.get_key())
    );

    let morph = MorphMany::<Post, RelationCounter>::new("id", "post_id", "owner_type", "scoped")
        .constrain(move |query| {
            query.with_attributes(
                Attributes::from([
                    ("count".into(), 79.into()),
                    ("owner_type".into(), "ignored".into()),
                    ("post_id".into(), 999.into()),
                ]),
                false,
            )
        });
    let forced = morph
        .force_create(post, name("scoped-force"))
        .await
        .unwrap();
    assert_eq!(forced.get_attribute("count"), Some(&79.into()));
    assert_eq!(forced.get_attribute("owner_type"), Some(&"scoped".into()));
    assert_eq!(forced.get_attribute("post_id"), post.get_key());

    let through =
        HasManyThrough::<Author, Post, RelationCounter>::new("author_id", "post_id", "id", "id")
            .constrain(move |query| query.with_attributes(defaults(81), false));
    let mut input = name("scoped-through");
    input.insert("post_id".into(), post.get_key().unwrap().clone());
    let related = through
        .first_or_create(author, input, Attributes::new())
        .await
        .unwrap();
    assert_eq!(related.get_attribute("count"), Some(&81.into()));

    let query = RelationCounter::query_on(db).with_attributes(defaults(82), false);
    let instance = query.make(name("scoped-builder")).unwrap();
    assert_eq!(instance.get_attribute("count"), Some(&82.into()));
    assert!(query.get().await.unwrap().len() > scoped.len());
}

async fn verify_non_pivot_counters(db: &Connection, mysql: bool) {
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY AUTOINCREMENT"
    };
    db.statement(&format!("CREATE TABLE relation_counters (id {id}, name VARCHAR(80), count BIGINT, visits BIGINT DEFAULT 10, post_id BIGINT, owner_type VARCHAR(80) DEFAULT 'post', deleted_at DATETIME, UNIQUE (name, post_id, owner_type))"), &[]).await.unwrap();
    let author = Author::query_on(db).find_or_fail(1).await.unwrap();
    let post = Post::query_on(db).find_or_fail(2).await.unwrap();
    let name = |name: &str| Attributes::from([("name".into(), name.into())]);
    let assert_count = |model: &RelationCounter, count: i64| {
        assert_eq!(model.get_attribute("count"), Some(&count.into()))
    };

    let direct = HasMany::<Post, RelationCounter>::new("id", "post_id");
    let model = direct
        .increment_or_create(
            &post,
            name("direct"),
            "count",
            10_i64,
            2_i64,
            Attributes::from([("post_id".into(), 999.into())]),
        )
        .await
        .unwrap();
    assert_eq!(model.get_attribute("post_id"), post.get_key());
    assert_count(&model, 10);
    let model = direct
        .one()
        .increment_or_create(
            &post,
            name("direct"),
            "count",
            10_i64,
            2_i64,
            Attributes::new(),
        )
        .await
        .unwrap();
    assert_count(&model, 12);

    let morph = || MorphMany::<Post, RelationCounter>::new("id", "post_id", "owner_type", "video");
    let model = morph()
        .increment_or_create(
            &post,
            name("direct"),
            "count",
            20_i64,
            2_i64,
            Attributes::from([("owner_type".into(), "wrong".into())]),
        )
        .await
        .unwrap();
    assert_eq!(model.get_attribute("owner_type"), Some(&"video".into()));
    assert_count(&model, 20);
    let model = morph()
        .one()
        .increment_or_create(
            &post,
            name("direct"),
            "count",
            20_i64,
            2_i64,
            Attributes::new(),
        )
        .await
        .unwrap();
    assert_count(&model, 22);

    db.table("relation_posts")
        .update(Attributes::from([("author_id".into(), 1.into())]))
        .await
        .unwrap();
    let through = || {
        HasManyThrough::<Author, Post, RelationCounter>::new("author_id", "post_id", "id", "id")
            .constrain(|mut query| {
                query.r#where("owner_type", "=", "post");
                query
            })
    };
    let model = through()
        .increment_or_create(
            &author,
            name("through"),
            "count",
            30_i64,
            2_i64,
            Attributes::from([("post_id".into(), 2.into())]),
        )
        .await
        .unwrap();
    assert_eq!(model.get_attribute("post_id"), post.get_key());
    assert_count(&model, 30);
    let model = through()
        .one()
        .increment_or_create(
            &author,
            name("through"),
            "count",
            30_i64,
            2_i64,
            Attributes::new(),
        )
        .await
        .unwrap();
    assert_count(&model, 32);
    let mut model = model;
    assert_eq!(
        model
            .increment_each(
                Attributes::from([("count".into(), 3.into()), ("visits".into(), 4.into())]),
                Attributes::from([("name".into(), "renamed".into())]),
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(model.get_attribute("count"), Some(&35.into()));
    assert_eq!(model.get_attribute("visits"), Some(&14.into()));
    assert_eq!(model.get_attribute("name"), Some(&"renamed".into()));
    assert_eq!(
        model
            .decrement_each(
                Attributes::from([("count".into(), 2.into()), ("visits".into(), 5.into())]),
                Attributes::new(),
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(model.get_attribute("count"), Some(&33.into()));
    assert_eq!(model.get_attribute("visits"), Some(&9.into()));
    assert!(model.get_dirty().unwrap().is_empty());
    assert!(
        model
            .increment_each(
                Attributes::from([
                    ("count".into(), 1.into()),
                    ("visits".into(), "invalid".into())
                ]),
                Attributes::new(),
            )
            .await
            .is_err()
    );
    let persisted = RelationCounter::query_on(db)
        .find_or_fail(model.get_key().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(persisted.get_attribute("count"), Some(&33.into()));
    assert_eq!(persisted.get_attribute("visits"), Some(&9.into()));
    Box::pin(verify_cast_counter(db, &post, mysql)).await;
    Box::pin(verify_non_pivot_lookup(&author, &post)).await;
    Box::pin(verify_scoped_attributes(db, &author, &post)).await;
    db.statement("DROP TABLE relation_counters", &[])
        .await
        .unwrap();
}

async fn verify_many_to_many_writes(db: &Connection, parent: &Author) {
    let relation = FEATURED_POSTS;
    let model = |title: &str, score: i64| {
        Attributes::from([
            ("title".into(), title.into()),
            ("score".into(), score.into()),
        ])
    };
    let joining = |label: &str| Attributes::from([("label".into(), label.into())]);
    let created = relation
        .create(parent, model("saved with pivot", 700), joining("first"))
        .await
        .unwrap();

    let mut existing = Post::query_on(db)
        .find_or_fail(created.get_key().unwrap().clone())
        .await
        .unwrap();
    existing.set_attribute("score", 701);
    assert!(
        relation
            .save(parent, &mut existing, joining("duplicate"))
            .await
            .is_err()
    );
    assert_eq!(
        Post::query_on(db)
            .find_or_fail(created.get_key().unwrap().clone())
            .await
            .unwrap()
            .get_attribute("score"),
        Some(&700.into())
    );
    assert!(existing.get_dirty().unwrap().contains_key("score"));

    let saved = relation
        .create_many_quietly(
            parent,
            [model("batch one", 702), model("batch two", 703)],
            &[joining("one"), joining("two")],
        )
        .await
        .unwrap();
    assert_eq!(
        saved
            .iter()
            .map(|row| row.get_attribute("score").cloned().unwrap())
            .collect::<Vec<_>>(),
        vec![702.into(), 703.into()]
    );
    let mut batch = [
        Post::new_instance(model("saved batch one", 704)).unwrap(),
        Post::new_instance(model("saved batch rejected", 705)).unwrap(),
    ];
    assert!(
        relation
            .save_many(
                parent,
                &mut batch,
                &[joining("committed"), Attributes::new()]
            )
            .await
            .is_err()
    );
    assert!(batch[0].state().exists());
    assert!(!batch[1].state().exists());
    let mut query = Post::query_on(db);
    query.r#where("title", "=", "saved batch rejected");
    assert!(query.first().await.unwrap().is_none());
    relation
        .save_many_quietly(parent, &mut batch[1..], &[joining("retry")])
        .await
        .unwrap();
    assert!(batch[1].state().exists());

    let forced = relation
        .force_create_quietly(
            parent,
            Attributes::from([
                ("title".into(), "forced pivot".into()),
                ("score".into(), 706.into()),
                ("author_id".into(), 44.into()),
            ]),
            joining("forced"),
        )
        .await
        .unwrap();
    assert_eq!(forced.get_attribute("author_id"), Some(&44.into()));
    let posts = FEATURED_POSTS
        .with_pivot(["label"])
        .get_results(parent)
        .await
        .unwrap();
    for (score, expected) in [
        (700, "first"),
        (702, "one"),
        (703, "two"),
        (704, "committed"),
        (705, "retry"),
        (706, "forced"),
    ] {
        let model = posts
            .iter()
            .find(|model| model.get_attribute("score") == Some(&score.into()))
            .unwrap();
        assert_eq!(
            model
                .get_relation::<Pivot>("pivot")
                .unwrap()
                .get_attribute("label"),
            Some(&expected.into())
        );
    }
}

async fn verify_create_or_first(db: &Connection, mysql: bool, split: bool) {
    for sql in [
        "CREATE TABLE relation_authors (id BIGINT PRIMARY KEY, name VARCHAR(80))",
        if mysql {
            "CREATE TABLE relation_posts (id BIGINT AUTO_INCREMENT PRIMARY KEY, title VARCHAR(80) UNIQUE, score BIGINT UNIQUE, deleted_at DATETIME, author_id BIGINT)"
        } else {
            "CREATE TABLE relation_posts (id INTEGER PRIMARY KEY AUTOINCREMENT, title VARCHAR(80) UNIQUE, score BIGINT UNIQUE, deleted_at DATETIME, author_id BIGINT)"
        },
        "CREATE TABLE relation_featured_posts (author_id BIGINT, post_id BIGINT, label VARCHAR(80) NOT NULL, PRIMARY KEY (author_id, post_id))",
        "INSERT INTO relation_authors VALUES (1, 'owner')",
        "INSERT INTO relation_posts (id, title, score, deleted_at) VALUES (2, 'existing', 20, NULL)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let parent = Author::query_on(db)
        .use_write_pdo()
        .find_or_fail(1)
        .await
        .unwrap();
    let relation = FEATURED_POSTS.with_pivot(["label"]);
    let create = |title: &str, score: i64, label: Value| {
        relation.create_or_first(
            &parent,
            Attributes::from([("title".into(), title.into())]),
            Attributes::from([("score".into(), score.into())]),
            Attributes::from([("label".into(), label)]),
        )
    };
    let first = create("new", 10, "new pivot".into()).await.unwrap();
    assert!(first.get_key().is_some());
    let existing = create("existing", 30, "existing pivot".into())
        .await
        .unwrap();
    assert_eq!(existing.get_key(), Some(&2.into()));
    let attached = create("existing", 40, "ignored".into()).await.unwrap();
    assert_eq!(attached.get_key(), Some(&2.into()));
    assert_eq!(
        attached
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"existing pivot".into())
    );
    assert!(matches!(
        create("unrelated conflict", 10, "unused".into()).await,
        Err(DatabaseError::Driver { .. })
    ));
    assert!(matches!(
        create("rollback", 50, Value::Null).await,
        Err(DatabaseError::Driver { .. })
    ));
    let mut rolled_back = Post::query_on(db).use_write_pdo();
    rolled_back.r#where("title", "=", "rollback");
    assert!(rolled_back.first().await.unwrap().is_none());
    if mysql {
        let (a, b) = tokio::join!(
            create("race", 60, "a".into()),
            create("race", 70, "b".into())
        );
        assert_eq!(a.unwrap().get_key(), b.unwrap().get_key());
    }
    if !split {
        db.statement(
            "INSERT INTO relation_posts (id, title, score, deleted_at) VALUES (100, 'unlinked', 90, NULL)",
            &[],
        )
        .await
        .unwrap();
        Box::pin(verify_first_and_update_or_create(db)).await;
        Box::pin(verify_restore_and_deferred_values(db)).await;
        Box::pin(verify_increment_or_create(db, mysql)).await;
        Box::pin(verify_non_pivot_counters(db, mysql)).await;
        Box::pin(verify_many_to_many_writes(db, &parent)).await;
    }
    for table in [
        "relation_featured_posts",
        "relation_posts",
        "relation_authors",
    ] {
        db.statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn many_to_many_first_and_update_or_create_on_sqlite() {
    let db = connect(json!({"driver":"sqlite","database":":memory:","max_connections":1}));
    Box::pin(verify_create_or_first(&db, false, false)).await;
}

#[tokio::test]
async fn create_or_first_reads_the_writer_when_replica_has_no_rows() {
    let dir = tempfile::tempdir().unwrap();
    let write = dir.path().join("write.sqlite");
    let read = dir.path().join("read.sqlite");
    std::fs::File::create(&write).unwrap();
    std::fs::File::create(&read).unwrap();
    let db = connect(
        json!({"driver":"sqlite","database":write,"read":{"database":read},"sticky":false}),
    );
    assert!(db.table("relation_posts").count("*").await.is_err());
    Box::pin(verify_create_or_first(&db, false, true)).await;
}

async fn verify_pivot_or_fail(db: &Connection, owner: &Author) {
    let keys = [1.into(), 2.into()];
    let label = |value: i64| Attributes::from([("label".into(), value.into())]);
    let plain = FEATURED_POSTS;
    plain.detach_or_fail(owner, None).await.unwrap();
    plain.attach_or_fail(owner, &keys, label(11)).await.unwrap();
    assert_eq!(
        plain
            .update_existing_pivot_or_fail(owner, &keys[1], label(88))
            .await
            .unwrap(),
        1
    );
    let custom = FEATURED_POSTS.using::<Membership>().with_pivot(["label"]);
    let count_events = || db.table("relation_membership_events").count("*");
    let baseline = count_events().await.unwrap();
    assert!(custom.detach_or_fail(owner, Some(&keys)).await.is_err());
    assert!(
        custom
            .update_existing_pivot_or_fail(owner, &keys[0], label(88))
            .await
            .is_err()
    );
    assert_eq!(custom.get_results(owner).await.unwrap().len(), 2);
    assert_eq!(count_events().await.unwrap(), baseline);

    assert_eq!(
        plain.detach_or_fail(owner, Some(&keys[1..])).await.unwrap(),
        1
    );
    let baseline = count_events().await.unwrap();
    assert!(
        custom
            .attach_or_fail(owner, &keys[1..], label(77))
            .await
            .is_err()
    );
    assert!(
        custom
            .sync_with_pivot_values_or_fail(owner, &keys[1..], label(77), true)
            .await
            .is_err()
    );
    let current = custom.get_results(owner).await.unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].get_key(), Some(&keys[0]));
    assert_eq!(
        current[0]
            .get_relation::<Membership>("pivot")
            .unwrap()
            .get_attribute_value("label")
            .unwrap(),
        Some(11.into())
    );
    assert_eq!(count_events().await.unwrap(), baseline);

    let changes = custom
        .sync_without_detaching_or_fail(owner, &keys[1..])
        .await
        .unwrap();
    assert_eq!(changes.attached, keys[1..]);
    assert!(changes.detached.is_empty());
    let changes = custom.sync_or_fail(owner, &keys[1..], true).await.unwrap();
    assert_eq!(changes.detached, keys[..1]);
}

struct DirectWriteEvents(Arc<std::sync::atomic::AtomicUsize>);

impl openrust::database::eloquent::concerns::ModelEventListener<DirectWritePost>
    for DirectWriteEvents
{
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut DirectWritePost,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        use openrust::database::eloquent::concerns::ModelEventResponse::Continue;
        Box::pin(async move {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if model.get_attribute("title") == Some(&"reject batch row".into()) {
                return Err(DatabaseError::InvalidQuery("rejected related row"));
            }
            Ok(Continue)
        })
    }
}

async fn verify_direct_write_variants(
    app: &openrust::foundation::Application,
    db: &Connection,
    parent: &Author,
) {
    use openrust::database::eloquent::concerns::ModelEvent::Creating;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    DirectWritePost::register_model_event(app, Creating, DirectWriteEvents(calls.clone())).unwrap();

    // This fixture has explicit, non-generated IDs.
    let make = |id: i64, title: &str| {
        let mut model =
            DirectWritePost::new_instance(Attributes::from([("title".into(), title.into())]))
                .unwrap();
        model.set_attribute("id", id);
        model
    };
    let attributes = |id: i64, title: &str| {
        Attributes::from([("id".into(), id.into()), ("title".into(), title.into())])
    };
    let relation = HasMany::<Author, DirectWritePost>::new("id", "author_id");
    let created = relation
        .create_many(
            parent,
            [
                attributes(101, "first batch row"),
                attributes(102, "second batch row"),
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        created
            .iter()
            .map(|model| model.get_key().cloned().unwrap())
            .collect::<Vec<_>>(),
        vec![101.into(), 102.into()]
    );
    let quiet = relation
        .one()
        .create_many_quietly(parent, [attributes(103, "quiet batch row")])
        .await
        .unwrap();
    assert_eq!(quiet[0].get_attribute("author_id"), parent.get_key());

    let mut rows = [
        make(104, "first saved row"),
        make(105, "reject batch row"),
        make(106, "unvisited row"),
    ];
    assert!(
        HasMany::<Author, DirectWritePost>::new("id", "author_id")
            .save_many(parent, &mut rows)
            .await
            .is_err()
    );
    assert!(rows[0].state().exists());
    assert!(!rows[1].state().exists());
    assert!(rows[2].get_attribute("author_id").is_none());
    HasOne::<Author, DirectWritePost>::new("id", "author_id")
        .save_many_quietly(parent, &mut rows[1..])
        .await
        .unwrap();
    assert!(
        rows.iter()
            .all(|row| row.state().exists() && row.get_attribute("author_id") == parent.get_key())
    );
    let morph = MorphMany::<Author, DirectWritePost>::new("id", "author_id", "title", "morph");
    let created = morph
        .create_quietly(parent, attributes(107, "overwritten"))
        .await
        .unwrap();
    assert_eq!(created.get_attribute("title"), Some(&"morph".into()));
    let mut model = make(108, "overwritten");
    assert!(morph.one().save_quietly(parent, &mut model).await.unwrap());
    assert_eq!(model.get_attribute("title"), Some(&"morph".into()));
    assert_eq!(model.get_attribute("author_id"), parent.get_key());
    assert_eq!(calls.load(Ordering::Relaxed), 4);

    let protected = |id: i64| {
        Attributes::from([
            ("id".into(), id.into()),
            ("title".into(), "forced".into()),
            ("score".into(), 37.into()),
            ("author_id".into(), 999.into()),
        ])
    };
    let direct = HasMany::<Author, DirectWritePost>::new("id", "author_id");
    let forced = direct.force_create(parent, protected(109)).await.unwrap();
    assert_eq!(forced.get_attribute("score"), Some(&3700.into()));
    assert_eq!(forced.get_attribute("author_id"), parent.get_key());

    let morph =
        || MorphMany::<Author, DirectWritePost>::new("id", "author_id", "title", "forced morph");
    let forced = morph()
        .force_create_many(parent, [protected(110), protected(111)])
        .await
        .unwrap();
    assert_eq!(
        forced
            .iter()
            .map(|model| model.get_key().cloned().unwrap())
            .collect::<Vec<_>>(),
        vec![110.into(), 111.into()]
    );
    assert!(
        forced
            .iter()
            .all(|model| model.get_attribute("title") == Some(&"forced morph".into()))
    );
    let quiet = morph()
        .one()
        .force_create_quietly(parent, protected(112))
        .await
        .unwrap();
    assert_eq!(quiet.get_attribute("title"), Some(&"forced morph".into()));
    let quiet = direct
        .one()
        .force_create_many_quietly(parent, [protected(113)])
        .await
        .unwrap();
    assert_eq!(quiet[0].get_attribute("score"), Some(&3700.into()));
    assert_eq!(calls.load(Ordering::Relaxed), 7);
    assert_eq!(
        DirectWritePost::query_on(db)
            .find_or_fail(113)
            .await
            .unwrap()
            .get_attribute("score"),
        Some(&3700.into())
    );

    for id in 101..=113 {
        db.table("relation_posts")
            .r#where("id", "=", id)
            .delete()
            .await
            .unwrap();
    }
}

struct DirectWritePost(ModelState);

impl Model for DirectWritePost {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("relation_posts")
            .incrementing(false)
            .fillable(&["id", "title"])
            .timestamps(None, None);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        const CASTS: &[(&str, AttributeCast)] = &[(
            "score",
            openrust::database::eloquent::casts::ClassCast::of::<CounterScale>(&100),
        )];
        CASTS
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

struct CounterEvents;

impl openrust::database::eloquent::concerns::ModelEventListener<Post> for CounterEvents {
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        _: &'a mut Post,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        Box::pin(async { panic!("counter events must run in the update transaction") })
    }

    fn handle_in_transaction<'a>(
        &'a self,
        event: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut Post,
        transaction: &'a mut openrust::database::Transaction,
    ) -> openrust::database::DatabaseFuture<
        'a,
        DatabaseResult<openrust::database::eloquent::concerns::ModelEventResponse>,
    > {
        use openrust::database::eloquent::concerns::{ModelEvent::*, ModelEventResponse::*};
        Box::pin(async move {
            let score = model.get_attribute("score").unwrap().clone();
            transaction
                .statement(
                    "INSERT INTO relation_counter_events (event, score) VALUES (?, ?)",
                    &[format!("{event:?}").into(), score],
                )
                .await?;
            if event == Updated && model.get_attribute("score") == Some(&14.into()) {
                return Err(DatabaseError::InvalidQuery(
                    "counter observer rejected update",
                ));
            }
            Ok(Continue)
        })
    }
}

async fn verify_counter_events(
    app: &openrust::foundation::Application,
    db: &Connection,
    parent: &Author,
) {
    use openrust::database::eloquent::concerns::ModelEvent::{Saved, Saving, Updated, Updating};
    for event in [Saving, Updating, Updated, Saved] {
        Post::register_model_event(app, event, CounterEvents).unwrap();
    }
    db.statement(
        "CREATE TABLE relation_counter_events (event VARCHAR(20), score BIGINT)",
        &[],
    )
    .await
    .unwrap();
    let relation = FEATURED_POSTS;
    let increment = || {
        relation.increment_or_create(
            parent,
            Attributes::from([("title".into(), "second".into())]),
            "score",
            10_i64,
            2_i64,
            Attributes::new(),
        )
    };
    assert_eq!(
        increment().await.unwrap().get_attribute("score"),
        Some(&12.into())
    );
    assert!(matches!(
        increment().await,
        Err(DatabaseError::InvalidQuery(
            "counter observer rejected update"
        ))
    ));
    let events = db
        .table("relation_counter_events")
        .order_by("event", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].get("event"), Some(&"Updated".into()));
    assert_eq!(events[1].get("event"), Some(&"Updating".into()));
    assert!(
        events
            .iter()
            .all(|event| event.get("score") == Some(&12.into()))
    );
    assert_eq!(
        Post::query_on(db)
            .find_or_fail(2)
            .await
            .unwrap()
            .get_attribute("score"),
        Some(&12.into())
    );
    let mut model = Post::query_on(db).find_or_fail(2).await.unwrap();
    assert_eq!(
        model
            .increment_quietly("score", 2, Attributes::new())
            .await
            .unwrap(),
        1
    );
    assert_eq!(model.get_attribute("score"), Some(&14.into()));
    assert_eq!(
        model
            .decrement_quietly("score", 1, Attributes::new())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        model
            .increment_each_quietly(
                Attributes::from([("score".into(), 3.into())]),
                Attributes::new()
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        model
            .decrement_each_quietly(
                Attributes::from([("score".into(), 2.into())]),
                Attributes::new()
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(model.get_attribute("score"), Some(&14.into()));
    assert!(model.get_dirty().unwrap().is_empty());
    assert_eq!(
        Post::query_on(db)
            .increment("score", 1, Attributes::new())
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        db.table("relation_counter_events")
            .count("*")
            .await
            .unwrap(),
        2
    );

    db.table("relation_posts")
        .r#where("id", "=", 2)
        .update(Attributes::from([(
            "updated_at".into(),
            "2000-01-01 00:00:00".into(),
        )]))
        .await
        .unwrap();
    let mut query = TouchPost::query_on(db);
    query.where_key(2);
    assert_eq!(
        query
            .increment_each(
                Attributes::from([("score".into(), 2.into())]),
                Attributes::new()
            )
            .await
            .unwrap(),
        1
    );
    let updated = TouchPost::query_on(db).find_or_fail(2).await.unwrap();
    assert_eq!(updated.get_attribute("score"), Some(&17.into()));
    assert_ne!(
        updated.get_attribute("updated_at"),
        Some(&"2000-01-01 00:00:00".into())
    );

    let mut query = TouchPost::query_on(db);
    query.where_key(2);
    assert_eq!(
        query
            .decrement_each(
                Attributes::from([("score".into(), 2.into())]),
                Attributes::from([("updated_at".into(), "2001-01-01 00:00:00".into())])
            )
            .await
            .unwrap(),
        1
    );
    let updated = TouchPost::query_on(db).find_or_fail(2).await.unwrap();
    assert_eq!(updated.get_attribute("score"), Some(&15.into()));
    assert_eq!(
        updated.get_attribute("updated_at"),
        Some(&"2001-01-01 00:00:00".into())
    );

    let mut query = TouchPost::query_on(db);
    query.where_key(3);
    assert_eq!(
        query
            .decrement("score", 1, Attributes::new())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        TouchPost::query_on(db)
            .with_trashed()
            .find_or_fail(3)
            .await
            .unwrap()
            .get_attribute("score"),
        Some(&10.into())
    );
    db.statement("DROP TABLE relation_counter_events", &[])
        .await
        .unwrap();
}

struct DefaultMembership(ModelState);

impl Model for DefaultMembership {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_default_memberships")
            .incrementing(false)
            .timestamps(None, None)
            .fillable(&["label"]);
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

    fn new_instance(attributes: Attributes) -> DatabaseResult<Self> {
        let mut model = Self::from_state(ModelState::default());
        model.set_attribute("label", 7);
        model.set_attribute("note", "model fallback");
        model.fill(attributes)?;
        Ok(model)
    }
}

use openrust::database::eloquent::concerns::{
    ModelEvent as PivotEvent, ModelEventResponse as PivotEventResponse,
};

#[derive(Clone)]
struct DefaultMembershipEvents(Arc<std::sync::Mutex<Vec<PivotEvent>>>);

impl openrust::database::eloquent::concerns::ModelEventListener<DefaultMembership>
    for DefaultMembershipEvents
{
    fn handle<'a>(
        &'a self,
        _: PivotEvent,
        _: &'a mut DefaultMembership,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<'a, DatabaseResult<PivotEventResponse>> {
        Box::pin(async { panic!("expected a transactional pivot event") })
    }

    fn handle_in_transaction<'a>(
        &'a self,
        event: PivotEvent,
        model: &'a mut DefaultMembership,
        transaction: &'a mut openrust::database::Transaction,
    ) -> openrust::database::DatabaseFuture<'a, DatabaseResult<PivotEventResponse>> {
        use PivotEvent::*;
        use PivotEventResponse::*;
        Box::pin(async move {
            self.0.lock().unwrap().push(event);
            let label = model.get_attribute("label").cloned();
            if event == Creating {
                assert_eq!(model.get_raw_original("label"), label.as_ref());
                assert!(model.get_dirty()?.is_empty());
                if label == Some(99.into()) {
                    return Ok(Cancel);
                }
            }
            if event == Updating {
                assert_eq!(model.get_raw_original("label"), Some(&7.into()));
                assert_eq!(
                    model.get_dirty()?,
                    Attributes::from([("label".into(), 8.into())])
                );
            }
            transaction
                .statement(
                    "INSERT INTO relation_membership_events (event) VALUES (?)",
                    &[format!("{event:?}").into()],
                )
                .await?;
            if event == Created && label == Some(77.into()) {
                return Err(DatabaseError::InvalidQuery(
                    "reject grouped pivot insertion",
                ));
            }
            Ok(Continue)
        })
    }
}

async fn verify_default_pivot_events(
    app: &openrust::foundation::Application,
    db: &Connection,
    owner: &Author,
) {
    use PivotEvent::*;
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    for event in [
        Saving, Creating, Created, Updating, Updated, Saved, Deleting, Deleted,
    ] {
        DefaultMembership::register_model_event(
            app,
            event,
            DefaultMembershipEvents(events.clone()),
        )
        .unwrap();
    }
    let drain = || std::mem::take(&mut *events.lock().unwrap());
    db.statement("DROP TABLE IF EXISTS relation_default_memberships", &[])
        .await
        .unwrap();
    db.statement("CREATE TABLE relation_default_memberships (author_id BIGINT NOT NULL, post_id BIGINT NOT NULL, label BIGINT NOT NULL, note VARCHAR(80), PRIMARY KEY (author_id, post_id))", &[]).await.unwrap();
    let base = || {
        BelongsToMany::<Author, Post>::new(
            "relation_default_memberships",
            "author_id",
            "post_id",
            "id",
            "id",
        )
        .using::<DefaultMembership>()
    };
    let relation = || base().with_pivot(["label", "note"]);
    let label = |value: i64| Attributes::from([("label".into(), value.into())]);
    relation()
        .attach_or_fail(owner, &[1.into()], Attributes::new())
        .await
        .unwrap();
    assert_eq!(drain(), [Saving, Creating, Created, Saved]);
    let linked = relation().get_results(owner).await.unwrap();
    assert_eq!(
        linked[0]
            .get_relation::<DefaultMembership>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&7.into())
    );

    let scoped = relation().where_pivot_with(|q| q.r#where("label", "=", 7));
    assert_eq!(
        scoped
            .update_existing_pivot_or_fail(owner, &1.into(), label(8))
            .await
            .unwrap(),
        1
    );
    assert_eq!(drain(), [Saving, Updating, Updated, Saved]);
    assert_eq!(scoped.detach_or_fail(owner, None).await.unwrap(), 0);
    assert!(drain().is_empty());

    db.statement(
        "UPDATE relation_default_memberships SET note = NULL WHERE author_id = ? AND post_id = ?",
        &[owner.get_key().unwrap().clone(), 1.into()],
    )
    .await
    .unwrap();
    let projected = base().get_results(owner).await.unwrap();
    let pivot = projected[0]
        .get_relation::<DefaultMembership>("pivot")
        .unwrap();
    assert_eq!(pivot.get_raw_original("label"), Some(&7.into()));
    assert_eq!(
        pivot.get_raw_original("note"),
        Some(&"model fallback".into())
    );
    assert!(!pivot.state().is_dirty());
    assert_eq!(
        projected[0].to_array().unwrap()["pivot"]["note"],
        "model fallback"
    );
    let selected = relation().get_results(owner).await.unwrap();
    let pivot = selected[0]
        .get_relation::<DefaultMembership>("pivot")
        .unwrap();
    assert_eq!(pivot.get_raw_original("label"), Some(&8.into()));
    assert_eq!(pivot.get_raw_original("note"), Some(&Value::Null));
    assert_eq!(pivot.get_raw_original("author_id"), owner.get_key());
    assert!(!pivot.state().is_dirty());
    assert_eq!(selected[0].to_array().unwrap()["pivot"]["label"], 8);
    assert_eq!(
        selected[0].to_array().unwrap()["pivot"].get("note"),
        Some(&Json::Null)
    );
    assert!(drain().is_empty());

    let committed_events = db
        .table("relation_membership_events")
        .count("*")
        .await
        .unwrap();
    let scoped = relation().where_pivot_with(|q| q.r#where("label", "=", 8));
    assert!(matches!(
        scoped
            .sync_with_pivot_values_or_fail(owner, &[2.into()], label(77), true)
            .await,
        Err(DatabaseError::InvalidQuery(
            "reject grouped pivot insertion"
        ))
    ));
    assert_eq!(drain(), [Deleting, Deleted, Saving, Creating, Created]);
    assert_eq!(
        db.table("relation_membership_events")
            .count("*")
            .await
            .unwrap(),
        committed_events
    );
    let linked = relation().get_results(owner).await.unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].get_key(), Some(&1.into()));
    assert_eq!(
        linked[0]
            .get_relation::<DefaultMembership>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&8.into())
    );

    relation()
        .attach_or_fail(owner, &[2.into()], label(99))
        .await
        .unwrap();
    assert_eq!(drain(), [Saving, Creating]);
    assert_eq!(relation().get_results(owner).await.unwrap().len(), 1);
    db.statement("DROP TABLE relation_default_memberships", &[])
        .await
        .unwrap();
}

async fn verify_custom_pivot(config: Json) {
    let mysql = config.get("url").is_some();
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        database::eloquent::concerns::ModelEvent::{Created, Deleted, Deleting, Saving, Updated},
        foundation::Application,
    };
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.config().unwrap();
    repository.set("database.default", json!("pivot")).unwrap();
    repository
        .set("database.connections.pivot", config)
        .unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    for (event, listener) in [Saving, Created, Updated, Deleting, Deleted]
        .into_iter()
        .map(|event| (event, MembershipEvents))
    {
        Membership::register_model_event(&app, event, listener).unwrap();
    }
    for sql in [
        "DROP TABLE IF EXISTS relation_membership_events",
        "DROP TABLE IF EXISTS relation_featured_posts",
        "DROP TABLE IF EXISTS relation_posts",
        "DROP TABLE IF EXISTS relation_authors",
        "CREATE TABLE relation_authors (id BIGINT PRIMARY KEY, name VARCHAR(80), updated_at VARCHAR(32))",
        "CREATE TABLE relation_posts (id BIGINT PRIMARY KEY, author_id BIGINT, title VARCHAR(80), deleted_at DATETIME, updated_at VARCHAR(32), score BIGINT DEFAULT 10, CHECK (id <> 4 OR updated_at = '2000-01-01 00:00:00'))",
        "CREATE TABLE relation_featured_posts (author_id BIGINT NOT NULL, post_id BIGINT NOT NULL, label VARCHAR(80), PRIMARY KEY (author_id, post_id))",
        "CREATE TABLE relation_membership_events (event VARCHAR(30) NOT NULL)",
        "INSERT INTO relation_authors VALUES (1,'first','2000-01-01 00:00:00'),(2,'second','2000-01-01 00:00:00')",
        "INSERT INTO relation_posts (id, author_id, title, deleted_at, updated_at) VALUES (1,1,'first',NULL,'2000-01-01 00:00:00'),(2,1,'second',NULL,'2000-01-01 00:00:00')",
        "INSERT INTO relation_featured_posts VALUES (2,1,'55'),(1,2,'99')",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let owner = Author::query_on(&db).find_or_fail(1).await.unwrap();
    let relation = FEATURED_POSTS
        .using::<Membership>()
        .with_pivot(["label"])
        .r#as("membership");
    assert!(
        relation
            .attach_with(
                &owner,
                &[1.into()],
                Attributes::from([("label".into(), 77.into())])
            )
            .await
            .is_err()
    );
    assert_eq!(
        db.table("relation_membership_events")
            .count("*")
            .await
            .unwrap(),
        0
    );
    relation
        .attach_with(
            &owner,
            &[1.into()],
            Attributes::from([("label".into(), "12".into())]),
        )
        .await
        .unwrap();
    assert!(
        FEATURED_POSTS
            .using::<Membership>()
            .chaperone_with(Some("invalid"), None)
            .is_err()
    );

    let baseline = if mysql { select_count(&db).await } else { 0 };
    let eager = Author::query_on(&db)
        .with(
            "posts",
            FEATURED_POSTS
                .using::<Membership>()
                .chaperone()
                .with_pivot(["label"])
                .r#as("membership"),
        )
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    for author in eager.iter() {
        let posts = author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap();
        for post in posts {
            let pivot = post.get_relation::<Membership>("membership").unwrap();
            assert_eq!(
                pivot
                    .get_relation::<Option<Arc<Author>>>("author")
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .get_key(),
                author.get_key()
            );
            assert_eq!(
                pivot
                    .get_relation::<Option<Arc<Post>>>("post")
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .get_key(),
                post.get_key()
            );
            assert_eq!(
                post.to_array().unwrap()["membership"]["post"]["id"],
                post.to_array().unwrap()["id"]
            );
            assert!(
                post.to_array().unwrap()["membership"]["post"]
                    .get("membership")
                    .is_none()
            );
        }
    }
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    let lazy = FEATURED_POSTS
        .using::<Membership>()
        .chaperone_with(Some("owner"), Some("item"))
        .unwrap();
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let posts = lazy.get_results(&owner).await.unwrap();
    let pivot = posts[0].get_relation::<Membership>("pivot").unwrap();
    assert!(pivot.relation_loaded("owner") && pivot.relation_loaded("item"));
    assert!(!pivot.relation_loaded("author"));
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 1);
    }
    let posts = lazy.without_chaperone().get_results(&owner).await.unwrap();
    assert!(
        !posts[0]
            .get_relation::<Membership>("pivot")
            .unwrap()
            .relation_loaded("owner")
    );
    assert!(
        FEATURED_POSTS
            .chaperone()
            .get_results(&owner)
            .await
            .unwrap()[0]
            .get_relation::<Pivot>("pivot")
            .is_some()
    );
    let posts = eager[0]
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    let pivot = posts
        .iter()
        .find(|post| post.get_key() == Some(&1.into()))
        .unwrap()
        .get_relation::<Membership>("membership")
        .unwrap();
    assert_eq!(pivot.get_table(), "relation_featured_posts");
    assert!(pivot.state().exists());
    assert_eq!(pivot.get_attribute_value("label").unwrap(), Some(12.into()));
    assert_eq!(
        posts
            .iter()
            .find(|post| post.get_key() == Some(&1.into()))
            .unwrap()
            .to_array()
            .unwrap()["membership"]["label"],
        12
    );
    assert_eq!(
        relation
            .update_existing_pivot(
                &owner,
                &1.into(),
                Attributes::from([("label".into(), 12.into())])
            )
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        relation
            .update_existing_pivot(
                &owner,
                &1.into(),
                Attributes::from([("label".into(), 20.into())])
            )
            .await
            .unwrap(),
        1
    );
    Box::pin(verify_pivot_reload(pivot)).await;
    let report = relation.sync(&owner, &[]).await.unwrap();
    assert_eq!(report.detached, vec![1.into()]);
    assert_eq!(relation.detach(&owner, Some(&[2.into()])).await.unwrap(), 0);
    assert_eq!(
        db.table("relation_featured_posts")
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        db.table("relation_membership_events")
            .count("*")
            .await
            .unwrap(),
        3
    );
    db.statement("CREATE TABLE relation_morph_memberships (subject_type VARCHAR(40), subject_id BIGINT, tag_id BIGINT, label VARCHAR(80), PRIMARY KEY (subject_type, subject_id, tag_id))", &[]).await.unwrap();
    db.statement(
        "INSERT INTO relation_morph_memberships VALUES ('author',1,2,'35')",
        &[],
    )
    .await
    .unwrap();
    let morph = MorphToMany::<Author, Post>::new(
        "relation_morph_memberships",
        "subject_id",
        "tag_id",
        "id",
        "id",
        "subject_type",
        "featured",
    )
    .using::<Membership>()
    .chaperone()
    .with_pivot(["label"]);
    let changes = morph
        .sync_with_pivot_values(
            &owner,
            &[2.into()],
            Attributes::from([("label".into(), "15".into())]),
            true,
        )
        .await
        .unwrap();
    assert_eq!(changes.attached, vec![2.into()]);
    let posts = morph.get_results(&owner).await.unwrap();
    let pivot = posts[0].get_relation::<Membership>("pivot").unwrap();
    assert!(pivot.relation_loaded("author") && pivot.relation_loaded("post"));
    assert_eq!(
        pivot.get_attribute("subject_type"),
        Some(&"featured".into())
    );
    assert_eq!(pivot.get_attribute_value("label").unwrap(), Some(15.into()));
    assert_eq!(
        morph
            .detach_or_fail(&owner, Some(&[2.into()]))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.table("relation_morph_memberships")
            .count("*")
            .await
            .unwrap(),
        1
    );
    let result = morph.toggle(&owner, &[2.into(), 2.into()]).await.unwrap();
    assert_eq!(result.attached, vec![2.into()]);
    let result = morph.toggle(&owner, &[2.into()]).await.unwrap();
    assert_eq!(result.detached, vec![2.into()]);
    assert_eq!(
        db.table("relation_morph_memberships")
            .count("*")
            .await
            .unwrap(),
        1
    );
    db.table("relation_featured_posts")
        .r#where("author_id", "=", 1)
        .r#where("post_id", "=", 2)
        .update(Attributes::from([("label".into(), "42".into())]))
        .await
        .unwrap();
    let rollback = relation
        .toggle_with_pivot_attributes(
            &owner,
            &[
                (2.into(), Attributes::new()),
                (1.into(), Attributes::from([("label".into(), 77.into())])),
            ],
        )
        .await;
    assert!(rollback.is_err());
    assert_eq!(
        db.table("relation_featured_posts")
            .r#where("author_id", "=", 1)
            .r#where("post_id", "=", 2)
            .count("*")
            .await
            .unwrap(),
        1
    );
    Box::pin(verify_toggle_and_touch(&db)).await;
    db.statement("INSERT INTO relation_posts (id, author_id, title, deleted_at, updated_at) VALUES (3,1,'deleted','2000-01-01 00:00:00','2000-01-01 00:00:00'),(4,1,'reject touch',NULL,'2000-01-01 00:00:00')", &[]).await.unwrap();
    Box::pin(verify_related_touch(&db)).await;
    Box::pin(verify_save_touches_related(&db)).await;
    Box::pin(verify_inferred_pivot_touch(&db)).await;
    Box::pin(verify_pivot_or_fail(&db, &owner)).await;
    Box::pin(verify_direct_write_variants(&app, &db, &owner)).await;
    Box::pin(verify_counter_events(&app, &db, &owner)).await;
    db.statement("DROP TABLE relation_morph_memberships", &[])
        .await
        .unwrap();
    Box::pin(verify_default_pivot_events(&app, &db, &owner)).await;
    for sql in [
        "DROP TABLE relation_membership_events",
        "DROP TABLE relation_featured_posts",
        "DROP TABLE relation_posts",
        "DROP TABLE relation_authors",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
}

struct KeyedPost(ModelState);

impl Model for KeyedPost {
    fn metadata() -> &'static ModelMetadata {
        Post::metadata()
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
                .enumerate()
                .map(|(index, model)| (format!("post-{index}"), model)),
        )
    }
}

async fn verify_collection_factory(db: &Connection, author: &Author, mysql: bool) {
    let selected = KeyedPost::query_on(db)
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert!(!selected.is_empty());
    assert!(selected.get_by_key("post-0").is_some());

    let keyed_posts = || {
        HasMany::<Author, KeyedPost>::new("id", "author_id")
            .chaperone("author")
            .constrain(|query| {
                query.with_attributes(Attributes::from([("score".into(), 7.into())]), false)
            })
    };
    let relation = keyed_posts();
    let loaded = relation.get_results(author).await.unwrap();
    assert!(!loaded.is_empty());
    assert_eq!(
        loaded.get_by_key("post-0").unwrap().get_key(),
        loaded[0].get_key()
    );
    let mut lazy = Author::from_state(author.state().clone());
    assert!(
        lazy.get_relation_or_load("keyedLazy", &relation)
            .await
            .unwrap()
            .get_by_key("post-0")
            .is_some()
    );

    let mut query = Author::query_on(db).with("keyedEager", keyed_posts());
    query.where_key(author.get_key().unwrap().clone());
    let eager = query.first_or_fail().await.unwrap();
    let cached = eager
        .get_relation::<RelatedCollection<KeyedPost>>("keyedEager")
        .unwrap();
    assert_eq!(
        cached.get_by_key("post-0").unwrap().get_key(),
        cached[0].get_key()
    );
    assert!(cached[0].relation_loaded("author"));
    assert!(
        eager.to_array().unwrap()["keyed_eager"]
            .get("post-0")
            .is_some()
    );
    let unsaved = Author::from_state(Default::default());
    assert!(relation.get_results(&unsaved).await.unwrap().is_empty());

    let models = relation
        .make_many(
            author,
            [
                Attributes::from([("title".into(), "first draft".into())]),
                Attributes::from([("title".into(), "second draft".into())]),
            ],
        )
        .await
        .unwrap();
    for (index, title) in ["first draft", "second draft"].into_iter().enumerate() {
        let model = models.get_by_key(format!("post-{index}")).unwrap();
        assert_eq!(model.get_attribute("title"), Some(&title.into()));
        assert_eq!(model.get_attribute("score"), Some(&7.into()));
        assert_eq!(model.get_attribute("author_id"), author.get_key());
        assert!(!model.state().exists());
        assert!(model.relation_loaded("author"));
    }
    let morph = MorphOne::<Author, Comment>::new("id", "subject_id", "subject_type", "author");
    let child = morph
        .make(
            author,
            Attributes::from([("body".into(), "draft subject".into())]),
        )
        .await
        .unwrap();
    assert_eq!(child.get_attribute("subject_id"), author.get_key());
    assert_eq!(child.get_attribute("subject_type"), Some(&"author".into()));
    assert!(!child.state().exists());
    let baseline = if mysql { select_count(db).await } else { 0 };
    assert!(models.multiply(-1).unwrap().is_empty());
    assert!(models.multiply(0).unwrap().is_empty());
    assert!(models.multiply(i64::MAX).is_err());
    let single = models.multiply(1).unwrap();
    assert_eq!(
        single[0].get_attribute("title"),
        models[0].get_attribute("title")
    );
    let mut repeated = models.multiply(3).unwrap().into_iter().collect::<Vec<_>>();
    assert_eq!(repeated.len(), 6);
    for (index, model) in repeated.iter().enumerate() {
        assert_eq!(
            model.get_attribute("title"),
            models[index % 2].get_attribute("title")
        );
        assert!(model.relation_loaded("author"));
    }
    repeated[0].set_attribute("title", "changed");
    assert_eq!(
        repeated[2].get_attribute("title"),
        Some(&"first draft".into())
    );
    assert_eq!(
        models[0].get_attribute("title"),
        Some(&"first draft".into())
    );
    let keyed = models.multiply(2).unwrap();
    assert!(keyed.get_by_key("post-0").is_none());
    assert_eq!(
        keyed.get_by_key(2).unwrap().get_attribute("title"),
        Some(&"first draft".into())
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 0);
    }
    let mut query = KeyedPost::query_on(db);
    query.where_in("title", vec!["first draft".into(), "second draft".into()]);
    assert!(query.get().await.unwrap().is_empty());
}

async fn verify_chaperone(db: &Connection, mysql: bool, parent: &Author, post: &Post) {
    let baseline = if mysql { select_count(db).await } else { 0 };
    let mut query = Author::query_on(db).with("posts", POSTS.chaperone("author"));
    query.where_in("id", vec![1.into(), 2.into()]);
    let authors = query.get().await.unwrap();
    let author_relation = AUTHOR;
    let mut count = 0;
    for author in authors {
        for loaded in author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap()
        {
            let mut post = Post::from_state(loaded.state().clone());
            let inverse = post
                .get_relation_or_load("author", &author_relation)
                .await
                .unwrap();
            let inverse = inverse.as_ref().unwrap();
            assert_eq!(inverse.get_key(), author.get_key());
            assert!(!inverse.relation_loaded("posts"));
            count += 1;
        }
    }
    assert!(count >= 2);
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }

    let plain = POSTS
        .inverse("author")
        .without_inverse()
        .get_results(parent)
        .await
        .unwrap();
    assert!(!plain[0].relation_loaded("author"));
    let saved = POSTS
        .chaperone("author")
        .create(
            parent,
            Attributes::from([("title".into(), "inverse snapshot".into())]),
        )
        .await
        .unwrap();
    assert_eq!(
        saved
            .get_relation::<Option<Arc<Author>>>("author")
            .unwrap()
            .as_ref()
            .unwrap()
            .get_key(),
        parent.get_key(),
    );
    db.table("relation_posts")
        .r#where("id", "=", saved.get_key().unwrap().clone())
        .delete()
        .await
        .unwrap();

    let relation = POST_COMMENTS.chaperone("subject");
    let made = relation
        .first_or_new(
            post,
            Attributes::from([("body".into(), "unsaved inverse".into())]),
            Attributes::new(),
        )
        .await
        .unwrap();
    let owner = made
        .get_relation::<Option<openrust::database::eloquent::relations::MorphTarget>>("subject")
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(owner.alias(), "post");
    assert_eq!(
        owner.downcast_ref::<Post>().unwrap().get_key(),
        post.get_key()
    );

    let saved = relation
        .create(
            post,
            Attributes::from([("body".into(), "saved inverse".into())]),
        )
        .await
        .unwrap();
    let baseline = if mysql { select_count(db).await } else { 0 };
    let loaded = POST_COMMENTS
        .one()
        .inverse("subject")
        .get_results(post)
        .await
        .unwrap()
        .unwrap();
    let mut loaded = Comment::from_state(loaded.state().clone());
    let subject =
        MorphTo::<Comment>::new("subject_type", "subject_id").target::<Post>("post", "id");
    let loaded_owner = loaded
        .get_relation_or_load("subject", &subject)
        .await
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(
        loaded_owner.downcast_ref::<Post>().unwrap().get_key(),
        post.get_key()
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    db.table("relation_comments")
        .r#where("id", "=", saved.get_key().unwrap().clone())
        .delete()
        .await
        .unwrap();

    let mut absent = Author::new_instance_on(db, Attributes::new()).unwrap();
    absent.set_attribute("id", 999);
    let default = FIRST_POST
        .chaperone("author")
        .with_default(|_, _| {})
        .get_results(&absent)
        .await
        .unwrap()
        .unwrap();
    assert!(!default.state().exists());
    assert_eq!(
        default
            .get_relation::<Option<Arc<Author>>>("author")
            .unwrap()
            .as_ref()
            .unwrap()
            .get_key(),
        absent.get_key()
    );
}

async fn verify_eager_limits<const N: usize>(db: &Connection, mysql: bool, parents: [&Author; N]) {
    let featured = || {
        FEATURED_POSTS
            .with_pivot_value("label", "rank fixture")
            .unwrap()
            .with_pivot(["label"])
    };
    let mut expected = Vec::new();
    for owner in parents {
        let mut posts = Vec::new();
        let mut comments = Vec::new();
        for rank in 1..=3 {
            let post = POSTS
                .create(
                    owner,
                    Attributes::from([("title".into(), format!("rank fixture {rank}").into())]),
                )
                .await
                .unwrap();
            let mut comment = POST_COMMENTS
                .create(
                    &post,
                    Attributes::from([("body".into(), format!("rank fixture {rank}").into())]),
                )
                .await
                .unwrap();
            comment.set_attribute("post_id", post.get_key().unwrap().clone());
            comment.save().await.unwrap();
            posts.push(post.get_key().unwrap().clone());
            comments.push(comment.get_key().unwrap().clone());
        }
        featured().attach(owner, &posts).await.unwrap();
        expected.push((posts, comments));
    }
    let parent_keys = parents.map(|owner| owner.get_key().unwrap().clone());

    for offset in [0_usize, 1] {
        let mut query = Author::query_on(db)
            .with(
                "posts",
                POSTS.constrain(move |mut query| {
                    query.r#where("title", "like", "rank fixture %");
                    query.order_by("id", "desc").limit(2).offset(offset as u64)
                }),
            )
            .with(
                "featured",
                featured().constrain(move |query| {
                    query
                        .order_by("relation_posts.id", "desc")
                        .limit(2)
                        .offset(offset as u64)
                }),
            )
            .with(
                "comments",
                COMMENTS_THROUGH.constrain(move |mut query| {
                    query.r#where("relation_comments.body", "like", "rank fixture %");
                    query
                        .order_by("relation_comments.id", "desc")
                        .limit(2)
                        .offset(offset as u64)
                }),
            )
            .order_by("id", "asc");
        query.where_in("id", parent_keys.to_vec());
        let baseline = if mysql { select_count(db).await } else { 0 };
        let loaded = query.get().await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 4);
        }
        assert_eq!(loaded.model_keys(), parent_keys);
        for (author, (post_keys, comment_keys)) in loaded.iter().zip(&expected) {
            let indexes = [2 - offset, 1 - offset];
            for name in ["posts", "featured"] {
                let posts = author
                    .get_relation::<RelatedCollection<Post>>(name)
                    .unwrap();
                assert_eq!(
                    posts
                        .iter()
                        .map(|post| post.get_key().unwrap())
                        .collect::<Vec<_>>(),
                    indexes.map(|index| &post_keys[index])
                );
                for post in posts {
                    assert!(post.get_attribute("laravel_row").is_none());
                    if name == "featured" {
                        let pivot = post.get_relation::<Pivot>("pivot").unwrap();
                        assert_eq!(pivot.get_attribute("label"), Some(&"rank fixture".into()));
                        assert_eq!(pivot.get_attribute("author_id"), author.get_key());
                    }
                }
            }
            let comments = author
                .get_relation::<RelatedCollection<Comment>>("comments")
                .unwrap();
            assert_eq!(
                comments
                    .iter()
                    .map(|comment| comment.get_key().unwrap())
                    .collect::<Vec<_>>(),
                indexes.map(|index| &comment_keys[index])
            );
            assert!(comments.iter().all(|comment| {
                comment.get_attribute("laravel_row").is_none()
                    && comment.get_attribute("laravel_through_key").is_none()
            }));
        }
    }
}

async fn verify(db: Connection, other: Connection, mysql: bool) {
    db.unprepared("DROP TABLE IF EXISTS relation_subject_tags; DROP TABLE IF EXISTS relation_comments; DROP TABLE IF EXISTS relation_featured_posts; DROP TABLE IF EXISTS relation_posts; DROP TABLE IF EXISTS relation_authors").await.unwrap();
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY AUTOINCREMENT"
    };
    db.statement(
        &format!(
            "CREATE TABLE relation_authors (id {id}, mentor_id BIGINT, name VARCHAR(80) NOT NULL)"
        ),
        &[],
    )
    .await
    .unwrap();
    db.statement(&format!("CREATE TABLE relation_posts (id {id}, author_id BIGINT REFERENCES relation_authors(id), title VARCHAR(80) NOT NULL, score BIGINT DEFAULT 0, deleted_at DATETIME)"), &[]).await.unwrap();
    db.statement("CREATE TABLE relation_featured_posts (author_id BIGINT NOT NULL, post_id BIGINT NOT NULL, label VARCHAR(80), created_at VARCHAR(32), updated_at VARCHAR(32), linked_at VARCHAR(32), PRIMARY KEY (author_id, post_id), FOREIGN KEY (author_id) REFERENCES relation_authors(id), FOREIGN KEY (post_id) REFERENCES relation_posts(id))", &[]).await.unwrap();
    db.statement(&format!("CREATE TABLE relation_comments (id {id}, post_id BIGINT, subject_id BIGINT, subject_type VARCHAR(80), body VARCHAR(80), deleted_at DATETIME)"), &[]).await.unwrap();
    db.statement("CREATE TABLE relation_subject_tags (subject_type VARCHAR(80) NOT NULL, subject_id BIGINT NOT NULL, tag_id BIGINT NOT NULL, label VARCHAR(80), linked_at VARCHAR(32), PRIMARY KEY (subject_type, subject_id, tag_id), FOREIGN KEY (tag_id) REFERENCES relation_authors(id))", &[]).await.unwrap();
    let author =
        |name: &str| Author::query_on(&db).create(Attributes::from([("name".into(), name.into())]));
    let first = author("First").await.unwrap();
    let mut second = author("Second").await.unwrap();
    second.set_attribute("mentor_id", first.get_key().unwrap().clone());
    second.save().await.unwrap();
    let third = author("No posts").await.unwrap();
    assert_eq!(
        Author::query_on(&db)
            .has(&MENTEES)
            .unwrap()
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![1.into()]
    );
    let mut p1 = POSTS
        .create(
            &first,
            Attributes::from([
                ("title".into(), "First post".into()),
                ("score".into(), 10.into()),
            ]),
        )
        .await
        .unwrap();
    let p2 = POSTS
        .create(
            &second,
            Attributes::from([
                ("title".into(), "Second post".into()),
                ("score".into(), 20.into()),
            ]),
        )
        .await
        .unwrap();
    let mut hidden = POSTS
        .create(
            &first,
            Attributes::from([
                ("title".into(), "Hidden post".into()),
                ("score".into(), 99.into()),
            ]),
        )
        .await
        .unwrap();
    assert!(hidden.delete().await.unwrap());
    verify_related_column_queries(&db).await;
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let nested = Author::query_on(&db)
        .with_where_has(
            "posts",
            POSTS.constrain(|mut query| {
                query.r#where("title", "=", "First post");
                query.with("author", AUTHOR)
            }),
        )
        .unwrap()
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 3);
    }
    assert_eq!(nested.model_keys(), vec![1.into()]);
    let posts = nested[0]
        .get_relation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert_eq!(posts.len(), 1);
    let parent = posts[0]
        .get_relation::<Option<Arc<Author>>>("author")
        .unwrap();
    assert_eq!(parent.as_ref().unwrap().get_key(), first.get_key());
    assert_eq!(
        Author::query_on(&db)
            .where_doesnt_have(&POSTS)
            .unwrap()
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![3.into()]
    );
    let filtered = POSTS.constrain(|mut query| {
        query
            .r#where("title", "=", "First post")
            .or_where("title", "=", "Second post");
        query.order_by("title", "desc").limit(1)
    });
    assert_eq!(
        filtered.get_results(&first).await.unwrap()[0].get_key(),
        p1.get_key()
    );
    let filtered = Author::query_on(&db)
        .with("posts", filtered)
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(
        filtered
            .iter()
            .map(|model| model
                .get_relation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .len())
            .collect::<Vec<_>>(),
        vec![1, 1, 0],
    );

    let mut extra_first = POSTS
        .create(
            &first,
            Attributes::from([
                ("title".into(), "Additional first".into()),
                ("score".into(), 11.into()),
            ]),
        )
        .await
        .unwrap();
    let mut extra_second = POSTS
        .create(
            &second,
            Attributes::from([
                ("title".into(), "Additional second".into()),
                ("score".into(), 21.into()),
            ]),
        )
        .await
        .unwrap();
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let latest = Author::query_on(&db)
        .with(
            "posts",
            POSTS.constrain(|query| query.order_by("score", "desc").limit(1).offset(1)),
        )
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    for (author, key) in latest.iter().zip([p1.get_key(), p2.get_key()]) {
        let posts = author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].get_key(), key);
        assert!(posts[0].get_attribute("laravel_row").is_none());
    }
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let aggregates = Author::query_on(&db)
        .with_count("posts", &POSTS)
        .unwrap()
        .with_count(
            "posts as matching_posts",
            &POSTS.constrain(|mut query| {
                query.r#where("score", ">", 10);
                query.order_by("score", "desc")
            }),
        )
        .unwrap()
        .with_exists("posts", &POSTS)
        .unwrap()
        .with_sum("posts", &POSTS, "score")
        .unwrap()
        .with_avg("posts", &POSTS, "score")
        .unwrap()
        .with_min("posts", &POSTS, "score")
        .unwrap()
        .with_max("posts", &POSTS, "score")
        .unwrap()
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 1);
    }
    assert_eq!(aggregates[0].get_attribute("name"), Some(&"First".into()));
    let numeric = |attribute: &str| match aggregates[0].get_attribute(attribute) {
        Some(Value::Integer(value)) => value.to_string().parse::<f64>().unwrap(),
        Some(Value::Unsigned(value)) => value.to_string().parse::<f64>().unwrap(),
        Some(Value::Real(value)) => *value,
        Some(Value::Text(value)) => value.parse::<f64>().unwrap(),
        other => panic!("expected numeric attribute, got {other:?}"),
    };
    assert!((numeric("posts_count") - 2.0).abs() < f64::EPSILON);
    assert!((numeric("matching_posts") - 1.0).abs() < f64::EPSILON);
    assert!((numeric("posts_sum_score") - 21.0).abs() < f64::EPSILON);
    assert!((numeric("posts_avg_score") - 10.5).abs() < f64::EPSILON);
    assert!((numeric("posts_min_score") - 10.0).abs() < f64::EPSILON);
    assert!((numeric("posts_max_score") - 11.0).abs() < f64::EPSILON);
    assert_eq!(
        aggregates[0].get_attribute("posts_exists"),
        Some(&true.into())
    );
    assert_eq!(
        aggregates[2].get_attribute("posts_exists"),
        Some(&false.into())
    );
    assert_eq!(
        aggregates[2].get_attribute("posts_sum_score"),
        Some(&Value::Null)
    );
    assert_eq!(aggregates[2].get_attribute("posts_count"), Some(&0.into()));
    assert_eq!(
        Author::query_on(&db)
            .with_count("mentees", &MENTEES)
            .unwrap()
            .find_or_fail(1)
            .await
            .unwrap()
            .get_attribute("mentees_count"),
        Some(&1.into())
    );
    assert_eq!(
        Author::query_on(&db)
            .has_count(&POSTS, ">=", 2)
            .unwrap()
            .order_by("id", "asc")
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![1.into(), 2.into()]
    );
    assert_eq!(
        Author::query_on(&db)
            .has_count(&POSTS, "=", 0)
            .unwrap()
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![3.into()]
    );
    assert!(Author::query_on(&db).has_count(&POSTS, "like", 2).is_err());

    extra_first.set_attribute("score", 10);
    extra_first.save().await.unwrap();
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let ranked = Author::query_on(&db)
        .with("top", POSTS.one().latest_of_many(&["score", "id"]))
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    let winner = ranked[0]
        .get_relation::<Option<Arc<Post>>>("top")
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(winner.get_key(), extra_first.get_key());
    assert!(
        ranked[2]
            .get_relation::<Option<Arc<Post>>>("top")
            .unwrap()
            .is_none()
    );
    for (relation, expected) in [
        (
            POSTS.one().latest_of_many(&["score", "title"]),
            p1.get_key(),
        ),
        (POSTS.one().oldest_of_many(&["score", "id"]), p1.get_key()),
        (POSTS.one().oldest_of_many(&[]), extra_first.get_key()),
        (POSTS.one().oldest_of_many("score"), extra_first.get_key()),
        (
            POSTS.one().oldest_of_many(Some("score")),
            extra_first.get_key(),
        ),
        (
            POSTS.one().of_many_by("score", OneOfManyAggregate::Min),
            p1.get_key(),
        ),
        (
            POSTS.one().of_many(&[
                ("score", OneOfManyAggregate::Max),
                ("id", OneOfManyAggregate::Min),
            ]),
            p1.get_key(),
        ),
        (
            POSTS
                .one()
                .latest_of_many(Some("score"))
                .where_candidates(|query| {
                    query
                        .r#where("title", "=", "First post")
                        .or_where("title", "=", "Second post")
                })
                .unwrap(),
            p1.get_key(),
        ),
        (
            POSTS
                .one()
                .latest_of_many(Some("score"))
                .constrain(|query| query.with_trashed()),
            hidden.get_key(),
        ),
    ] {
        assert_eq!(
            relation
                .get_results(&first)
                .await
                .unwrap()
                .unwrap()
                .get_key(),
            expected
        );
    }
    let mut null_rank = POSTS
        .create(
            &third,
            Attributes::from([
                ("title".into(), "Null rank".into()),
                ("score".into(), Value::Null),
            ]),
        )
        .await
        .unwrap();
    assert!(
        POSTS
            .one()
            .latest_of_many(Some("score"))
            .get_results(&third)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        Author::query_on(&db)
            .has(&POSTS.one().latest_of_many(Some("score")))
            .unwrap()
            .order_by("id", "asc")
            .get()
            .await
            .unwrap()
            .model_keys(),
        vec![1.into(), 2.into()]
    );
    null_rank.force_delete().await.unwrap();
    extra_first.force_delete().await.unwrap();
    extra_second.force_delete().await.unwrap();
    assert_eq!(p1.get_attribute("author_id"), first.get_key());
    assert_eq!(p2.get_attribute("author_id"), second.get_key());
    assert_eq!(
        FIRST_POST
            .get_results(&second)
            .await
            .unwrap()
            .unwrap()
            .get_key(),
        p2.get_key()
    );
    assert!(FIRST_POST.get_results(&third).await.unwrap().is_none());

    let baseline = if mysql { select_count(&db).await } else { 0 };
    let mut authors = Author::query_on(&db)
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    for author in authors.all() {
        POSTS.get_results(author).await.unwrap();
    }
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 4);
    }
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let authors_with_posts = Author::query_on(&db)
        .with("posts", POSTS)
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    let related = |author: &Author| {
        author
            .get_relation::<RelatedCollection<Post>>("posts")
            .unwrap()
            .len()
    };
    assert_eq!(
        authors_with_posts.iter().map(related).collect::<Vec<_>>(),
        vec![1, 1, 0]
    );

    let baseline = if mysql { select_count(&db).await } else { 0 };
    authors.load("posts", &POSTS).await.unwrap();
    authors.load_missing("posts", &POSTS).await.unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 1);
    }
    assert!(authors.contains(first.get_key().unwrap()).unwrap());
    assert_eq!(
        authors
            .find(first.get_key().unwrap())
            .unwrap()
            .unwrap()
            .get_attribute("name"),
        Some(&"First".into())
    );
    assert_eq!(authors.model_keys(), vec![1.into(), 2.into(), 3.into()]);
    assert_eq!(
        authors
            .only(&[2_u64.to_string().into()])
            .unwrap()
            .model_keys(),
        vec![2.into()]
    );

    Box::pin(verify_collection_collapse(&db, mysql)).await;
    verify_collection_keys(&db, &other).await;

    Box::pin(verify_belongs_to_cache(&db, mysql, &p1, &first, &second)).await;
    Box::pin(verify_belongs_to_owner_keys(
        &db, mysql, &p1, &first, &second,
    ))
    .await;

    p1.load("author", &AUTHOR).await.unwrap();
    let related = p1.get_relation::<Option<Arc<Author>>>("author").unwrap();
    assert_eq!(related.as_ref().unwrap().get_key(), first.get_key());
    AUTHOR.associate(&mut p1, &second).unwrap();
    assert!(!p1.relation_loaded("author"));
    assert!(p1.save().await.unwrap());
    let found = Post::query_on(&db)
        .with("author", AUTHOR)
        .find_or_fail(p1.get_key().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(
        found
            .get_relation::<Option<Arc<Author>>>("author")
            .unwrap()
            .as_ref()
            .unwrap()
            .get_key(),
        second.get_key()
    );
    AUTHOR.dissociate(&mut p1);
    assert!(p1.save().await.unwrap());
    assert!(AUTHOR.get_results(&p1).await.unwrap().is_none());
    assert_eq!(POSTS.get_results(&first).await.unwrap().len(), 0);
    assert_eq!(POSTS.get_results(&second).await.unwrap().len(), 1);
    let first_key = p1.get_key().unwrap().clone();
    let second_key = p2.get_key().unwrap().clone();
    FEATURED_POSTS
        .attach_with(
            &first,
            &[first_key.clone(), hidden.get_key().unwrap().clone()],
            Attributes::from([("label".into(), "featured".into())]),
        )
        .await
        .unwrap();
    FEATURED_POSTS
        .attach(&second, std::slice::from_ref(&second_key))
        .await
        .unwrap();
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let featured = Author::query_on(&db)
        .with(
            "featured",
            FEATURED_POSTS
                .with_pivot(["label"])
                .constrain(|query| query.with_casts([("id", AttributeCast::String)])),
        )
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    {
        let array = featured.to_array().unwrap();
        assert_eq!(array[0]["featured"][0]["pivot"]["label"], "featured");
        assert_eq!(array[2]["featured"], json!([]));
    }
    assert!(matches!(
        featured.all()[0]
            .get_relation::<RelatedCollection<Post>>("featured")
            .unwrap()[0]
            .get_attribute_value("id")
            .unwrap(),
        Some(Value::Text(_))
    ));
    assert_eq!(
        featured
            .iter()
            .map(|author| author
                .get_relation::<RelatedCollection<Post>>("featured")
                .unwrap()
                .len())
            .collect::<Vec<_>>(),
        vec![1, 1, 0]
    );
    assert_eq!(
        FEATURED_POSTS
            .update_existing_pivot(
                &first,
                &first_key,
                Attributes::from([("label".into(), "updated".into())]),
            )
            .await
            .unwrap(),
        1,
    );
    assert!(
        FEATURED_POSTS
            .update_existing_pivot(
                &first,
                &first_key,
                Attributes::from([("author_id".into(), 99.into())]),
            )
            .await
            .is_err()
    );
    let pivot_rows = FEATURED_POSTS
        .with_pivot(["label"])
        .r#as("membership")
        .get_results(&first)
        .await
        .unwrap();
    let pivot = pivot_rows[0].get_relation::<Pivot>("membership").unwrap();
    assert_eq!(pivot.get_attribute("label"), Some(&"updated".into()));
    assert_eq!(pivot.get_attribute("author_id"), first.get_key());
    assert_eq!(pivot_rows[0].get_attribute("author_id"), Some(&Value::Null));
    let nested_pivot = Author::query_on(&db)
        .with_where_has(
            "featured",
            FEATURED_POSTS.constrain(|mut query| {
                query.r#where("title", "=", "Second post");
                query.with("author", AUTHOR)
            }),
        )
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(nested_pivot.model_keys(), vec![2.into()]);
    let posts = nested_pivot[0]
        .get_relation::<RelatedCollection<Post>>("featured")
        .unwrap();
    assert_eq!(posts.len(), 1);
    assert!(posts[0].relation_loaded("author"));

    let changes = FEATURED_POSTS
        .sync_without_detaching(&first, std::slice::from_ref(&second_key))
        .await
        .unwrap();
    assert_eq!(changes.attached, vec![second_key.clone()]);
    assert!(changes.detached.is_empty());
    let records = [
        (
            first_key.clone(),
            Attributes::from([("label".into(), "first".into())]),
        ),
        (
            second_key.clone(),
            Attributes::from([("label".into(), "second".into())]),
        ),
    ];
    let changes = FEATURED_POSTS
        .sync_with_pivot_attributes(&first, &records, false)
        .await
        .unwrap();
    assert_eq!(changes.updated, vec![first_key.clone(), second_key.clone()]);
    assert_eq!(
        FEATURED_POSTS
            .sync_with_pivot_attributes(&first, &records, false)
            .await
            .unwrap(),
        Default::default(),
    );
    let invalid = [
        (
            first_key.clone(),
            Attributes::from([("label".into(), "must roll back".into())]),
        ),
        (9999.into(), Attributes::new()),
    ];
    assert!(
        FEATURED_POSTS
            .sync_with_pivot_attributes(&first, &invalid, true)
            .await
            .is_err()
    );
    assert_eq!(FEATURED_POSTS.get_results(&first).await.unwrap().len(), 2);
    let limited_pivots = Author::query_on(&db)
        .with(
            "featured",
            FEATURED_POSTS
                .with_pivot(["label"])
                .constrain(|query| query.order_by("score", "asc").limit(1)),
        )
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    let links = limited_pivots[0]
        .get_relation::<RelatedCollection<Post>>("featured")
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].get_key(), Some(&first_key));
    assert_eq!(
        links[0]
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"first".into())
    );
    let changes = FEATURED_POSTS
        .sync_with_pivot_values(
            &first,
            std::slice::from_ref(&first_key),
            Attributes::from([("label".into(), Value::Null)]),
            true,
        )
        .await
        .unwrap();
    assert!(changes.attached.is_empty());
    assert_eq!(changes.detached.len(), 2);
    assert_eq!(changes.updated, vec![first_key.clone()]);
    assert_eq!(FEATURED_POSTS.detach(&first, Some(&[])).await.unwrap(), 0);
    assert_eq!(FEATURED_POSTS.detach(&first, None).await.unwrap(), 1);
    assert!(FEATURED_POSTS.get_results(&first).await.unwrap().is_empty());
    assert_eq!(FEATURED_POSTS.get_results(&second).await.unwrap().len(), 1);
    Box::pin(verify_pivot_constraints(
        &db,
        &first,
        &first_key,
        &second_key,
    ))
    .await;
    Box::pin(verify_pivot_timestamps(
        &db,
        &first,
        &first_key,
        &second_key,
    ))
    .await;
    Box::pin(verify_pivot_key_contracts(&db, &first))
        .await
        .unwrap();
    AUTHOR.associate(&mut p1, &first).unwrap();
    p1.save().await.unwrap();
    Box::pin(verify_singular_identity(
        &db, &other, mysql, &first, &p1, &hidden,
    ))
    .await;
    verify_through_and_morph(&db, &other, mysql, &first, &p1, &hidden).await;
    Box::pin(verify_morph_paths(&db, mysql)).await;
    verify_one_through(&db, mysql, &first).await;
    verify_collection_reload(&db, mysql).await;
    verify_relation_autoload(&db, mysql).await;
    verify_relation_chain(&db, mysql).await;
    Box::pin(verify_named_paths(&db, mysql)).await;
    Box::pin(verify_eager_columns(&db, mysql)).await;
    Box::pin(verify_path_constraints(&db)).await;

    verify_morph_batches(&db, mysql).await;
    verify_relation_identity(&db).await;
    verify_lazy_loading_policy(&db, mysql).await;
    verify_default_models(&db, mysql).await;
    verify_default_eager_loads(&db, mysql).await;
    verify_morph_wildcard(&db, mysql).await;
    Box::pin(verify_chaperone(&db, mysql, &first, &p1)).await;
    Box::pin(verify_collection_factory(&db, &first, mysql)).await;
    Box::pin(verify_eager_limits(&db, mysql, [&first, &second])).await;
    db.statement("DROP TABLE relation_featured_posts", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE relation_subject_tags", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE relation_comments", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE relation_posts", &[])
        .await
        .unwrap();
    db.statement("DROP TABLE relation_authors", &[])
        .await
        .unwrap();
}

async fn verify_dynamic_relations(config: Json) {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        foundation::Application,
    };

    let mut first = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    let mut second = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    first.boot().await.unwrap();
    second.boot().await.unwrap();

    let connection = |app: &Application| {
        let repository = app.config().unwrap();
        repository
            .set("database.default", json!("dynamic"))
            .unwrap();
        repository
            .set("database.connections.dynamic", config.clone())
            .unwrap();
        app.container()
            .make::<DatabaseManager>()
            .unwrap()
            .connection(None)
            .unwrap()
    };
    let db = connection(&first);
    let other = connection(&second);
    for sql in [
        "DROP TABLE IF EXISTS relation_posts",
        "DROP TABLE IF EXISTS relation_authors",
        "CREATE TABLE relation_authors (id BIGINT PRIMARY KEY, name VARCHAR(80))",
        "CREATE TABLE relation_posts (id BIGINT PRIMARY KEY, author_id BIGINT, title VARCHAR(80), deleted_at DATETIME)",
        "INSERT INTO relation_authors VALUES (1,'first'),(2,'second')",
        "INSERT INTO relation_posts VALUES (1,1,'first',NULL),(2,1,'second',NULL),(3,2,'third',NULL)",
    ] {
        db.statement(sql, &[]).await.unwrap();
        if config.get("url").is_none() {
            other.statement(sql, &[]).await.unwrap();
        }
    }

    use openrust::database::eloquent::relations::{MorphMap, MorphMapEntry};

    MorphMap::enforce_morph_map(&first, [MorphMapEntry::model::<Author>("writer")], false).unwrap();
    MorphMap::morph_map(&second, [MorphMapEntry::model::<Author>("member")], false).unwrap();
    let owner = Author::query_on(&db).find_or_fail(1).await.unwrap();
    let other_owner = Author::query_on(&other).find_or_fail(1).await.unwrap();
    assert_eq!(owner.get_morph_class().unwrap(), "writer");
    assert_eq!(other_owner.get_morph_class().unwrap(), "member");
    assert!(matches!(
        Post::query_on(&db)
            .find_or_fail(1)
            .await
            .unwrap()
            .get_morph_class(),
        Err(DatabaseError::InvalidQuery(
            "model requires a morph map entry"
        ))
    ));
    let registry = first.container().make::<MorphMap>().unwrap();
    assert_eq!(
        registry.get_morphed_model("writer"),
        Some(std::any::type_name::<Author>())
    );
    assert_eq!(registry.get_morphed_model("member"), None);
    assert!(MorphMap::morph_map(&first, [MorphMapEntry::model::<Post>("")], false).is_err());
    assert_eq!(owner.get_morph_class().unwrap(), "writer");

    db.statement("DROP TABLE IF EXISTS relation_comments", &[])
        .await
        .unwrap();
    let id = if config.get("url").is_some() {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY"
    };
    db.statement(&format!("CREATE TABLE relation_comments (id {id}, subject_id BIGINT, subject_type VARCHAR(80), body VARCHAR(80), deleted_at DATETIME)"), &[]).await.unwrap();
    let mapped =
        MorphMany::<Author, Comment>::mapped(&owner, "id", "subject_id", "subject_type").unwrap();
    let mut comment = mapped
        .create(&owner, Attributes::from([("body".into(), "mapped".into())]))
        .await
        .unwrap();
    assert_eq!(
        comment.get_attribute("subject_type"),
        Some(&"writer".into())
    );
    let inverse = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target_mapped::<Author>(&db, "id")
        .unwrap();
    assert_eq!(
        inverse
            .get_results(&comment)
            .await
            .unwrap()
            .unwrap()
            .downcast_ref::<Author>()
            .unwrap()
            .get_key(),
        owner.get_key()
    );
    inverse.associate_mapped(&mut comment, &owner).unwrap();
    assert_eq!(
        comment.get_attribute("subject_type"),
        Some(&"writer".into())
    );
    MorphMap::morph_map(&first, [MorphMapEntry::model::<Author>("author_v2")], true).unwrap();
    assert_eq!(owner.get_morph_class().unwrap(), "author_v2");
    assert_eq!(registry.get_morph_alias::<Author>(), "author_v2");
    let inverse = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target_mapped::<Author>(&db, "id")
        .unwrap();
    assert_eq!(
        inverse
            .get_results(&comment)
            .await
            .unwrap()
            .unwrap()
            .alias(),
        "writer"
    );
    inverse.associate_mapped(&mut comment, &owner).unwrap();
    assert_eq!(
        comment.get_attribute("subject_type"),
        Some(&"author_v2".into())
    );
    assert!(registry.get_morphed_model("writer").is_some());
    db.statement("DROP TABLE relation_comments", &[])
        .await
        .unwrap();

    type Posts = HasMany<Author, Post>;
    let named = DynamicRelation::<Posts>::new("selectedPosts");
    let register = |app: &Application, title| {
        Author::resolve_relation_using(app, "selectedPosts", move |_| {
            Posts::new("id", "author_id").constrain(move |mut query| {
                query.r#where("title", "=", title);
                query
            })
        })
        .unwrap();
    };
    register(&first, "first");
    let mut author = Author::query_on(&db).find_or_fail(1).await.unwrap();
    let mut other_author = Author::query_on(&other).find_or_fail(1).await.unwrap();
    assert!(
        other_author
            .relation_resolver::<Posts>("selectedPosts")
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        other_author
            .get_dynamic_relation_or_load::<Posts>("selectedPosts")
            .await,
        Err(DatabaseError::InvalidQuery("unknown dynamic relationship"))
    ));
    assert!(!other_author.relation_loaded("selectedPosts"));
    assert!(matches!(
        Author::query_on(&other).where_has(&named),
        Err(DatabaseError::InvalidQuery("unknown dynamic relationship"))
    ));
    register(&second, "second");
    assert_eq!(
        other_author
            .get_dynamic_relation_or_load::<Posts>("selectedPosts")
            .await
            .unwrap()[0]
            .get_key(),
        Some(&2.into())
    );
    assert_eq!(
        author
            .get_dynamic_relation_or_load::<Posts>("selectedPosts")
            .await
            .unwrap()[0]
            .get_key(),
        Some(&1.into())
    );
    assert!(author.to_array().unwrap().contains_key("selected_posts"));
    assert!(matches!(
        author.relation_resolver::<BelongsTo<Author, Post>>("selectedPosts"),
        Err(DatabaseError::InvalidQuery(
            "dynamic relationship descriptor type"
        ))
    ));

    Author::resolve_relation_using(&second, "selectedPosts", |_| Posts::new("id", "author_id"))
        .unwrap();
    let exists = Author::query_on(&db)
        .where_has(&named)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(exists.len(), 1);
    assert_eq!(exists[0].get_key(), Some(&1.into()));
    let absent = Author::query_on(&db)
        .where_doesnt_have(&named)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(absent.len(), 1);
    assert_eq!(absent[0].get_key(), Some(&2.into()));
    let counts = Author::query_on(&other)
        .has_count(&named, ">=", 2)
        .unwrap()
        .with_count("selectedPosts", &named)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(counts.len(), 1);
    assert_eq!(
        counts[0].get_attribute("selected_posts_count"),
        Some(&2.into())
    );
    let wrong = DynamicRelation::<BelongsTo<Author, Post>>::new("selectedPosts");
    assert!(matches!(
        Author::query_on(&db).where_has(&wrong),
        Err(DatabaseError::InvalidQuery(
            "dynamic relationship descriptor type"
        ))
    ));

    register(&first, "second");
    assert_eq!(
        author
            .get_dynamic_relation_or_load::<Posts>("selectedPosts")
            .await
            .unwrap()[0]
            .get_key(),
        Some(&1.into())
    );
    author.unset_relation("selectedPosts");
    assert_eq!(
        author
            .get_dynamic_relation_or_load::<Posts>("selectedPosts")
            .await
            .unwrap()[0]
            .get_key(),
        Some(&2.into())
    );

    Author::resolve_relation_using(&first, "selectedPosts", |parent| {
        let title = if parent.get_key() == Some(&1.into()) {
            "first"
        } else {
            "third"
        };
        Posts::new("id", "author_id").constrain(move |mut query| {
            query.r#where("title", "=", title);
            query
        })
    })
    .unwrap();
    let authors = Author::query_on(&db)
        .order_by("id", "asc")
        .with(
            "selectedPosts",
            DynamicRelation::<Posts>::new("selectedPosts"),
        )
        .get()
        .await
        .unwrap();
    for (author, id) in authors.iter().zip([1, 3]) {
        let posts = author
            .get_relation::<RelatedCollection<Post>>("selectedPosts")
            .unwrap();
        assert_eq!(posts[0].get_key(), Some(&id.into()));
    }
    let prototype_matches = Author::query_on(&db)
        .where_has(&named)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(prototype_matches.len(), 1);
    assert_eq!(prototype_matches[0].get_key(), Some(&2.into()));
    Box::pin(verify_dynamic_fallback(&first, &second, &db, &other)).await;
    for table in ["relation_posts", "relation_authors"] {
        db.statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
}

async fn verify_dynamic_fallback(
    first: &openrust::foundation::Application,
    second: &openrust::foundation::Application,
    db: &Connection,
    other: &Connection,
) {
    type Posts = HasMany<Author, Post>;
    type ComposedPosts = HasMany<StrictAuthor, Post>;
    let name = "fallbackPosts";
    let relation = DynamicRelation::<ComposedPosts>::new(name);
    let base = |model: &StrictAuthor| Author::from_state(model.state().clone());
    StrictAuthor::inherit_relation_using(first, name, base, Posts::for_parent).unwrap();
    assert!(matches!(
        StrictAuthor::query_on(db).where_has(&relation),
        Err(DatabaseError::InvalidQuery("unknown dynamic relationship"))
    ));
    Author::resolve_relation_using(first, name, |_| {
        Posts::new("id", "author_id").constrain(|mut query| {
            query.r#where("title", "=", "third");
            query
        })
    })
    .unwrap();
    let authors = StrictAuthor::query_on(db)
        .with_where_has(name, relation)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(authors.len(), 1);
    assert_eq!(authors[0].get_key(), Some(&2.into()));
    assert_eq!(
        authors[0]
            .get_relation::<RelatedCollection<Post>>(name)
            .unwrap()[0]
            .get_key(),
        Some(&3.into())
    );
    let outside = StrictAuthor::query_on(other).find_or_fail(2).await.unwrap();
    assert!(
        outside
            .relation_resolver::<ComposedPosts>(name)
            .unwrap()
            .is_none()
    );
    StrictAuthor::inherit_relation_using(second, name, base, Posts::for_parent).unwrap();
    assert!(
        outside
            .relation_resolver::<ComposedPosts>(name)
            .unwrap()
            .is_none()
    );

    StrictAuthor::resolve_relation_using(first, name, |_| ComposedPosts::new("id", "author_id"))
        .unwrap();
    let relation = DynamicRelation::<ComposedPosts>::new(name);
    assert_eq!(
        StrictAuthor::query_on(db)
            .where_has(&relation)
            .unwrap()
            .get()
            .await
            .unwrap()
            .len(),
        2
    );
    let base_relation = DynamicRelation::<Posts>::new(name);
    assert_eq!(
        Author::query_on(db)
            .where_has(&base_relation)
            .unwrap()
            .get()
            .await
            .unwrap()
            .len(),
        1
    );

    let cyclic = "cyclic";
    StrictAuthor::inherit_relation_using(first, cyclic, base, Posts::for_parent).unwrap();
    Author::inherit_relation_using(
        first,
        cyclic,
        |model| StrictAuthor::from_state(model.state().clone()),
        ComposedPosts::for_parent,
    )
    .unwrap();
    assert!(matches!(
        StrictAuthor::query_on(db).where_has(&DynamicRelation::<ComposedPosts>::new(cyclic)),
        Err(DatabaseError::InvalidQuery(
            "cyclic dynamic relationship inheritance"
        ))
    ));
}

#[tokio::test]
async fn relationship_keys_and_eager_loading_on_sqlite() {
    Box::pin(verify_dynamic_relations(
        json!({"driver":"sqlite","database":":memory:","max_connections":1}),
    ))
    .await;
    Box::pin(verify(
        connect(json!({"driver":"sqlite","database":":memory:","max_connections":1,"foreign_key_constraints":true})),
        connect(json!({"driver":"sqlite","database":":memory:","max_connections":1})),
        false,
    ))
    .await;
    Box::pin(verify_custom_pivot(
        json!({"driver":"sqlite","database":":memory:","max_connections":1}),
    ))
    .await;
}

#[tokio::test]
async fn relationship_keys_and_query_counts_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_RELATION_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_RELATION_MYSQL").is_none());
    if let Some(url) = url {
        let create_db = connect(json!({"url":url,"max_connections":2}));
        Box::pin(verify_create_or_first(&create_db, true, false)).await;
        let db = connect(json!({"url":url,"max_connections":1}));
        // Keep the large integration fixture off the default debug test stack.
        let other = connect(json!({"url":url,"max_connections":1}));
        Box::pin(verify(db, other, true)).await;
        Box::pin(verify_dynamic_relations(
            json!({"url":url,"max_connections":1}),
        ))
        .await;
        Box::pin(verify_custom_pivot(json!({"url":url,"max_connections":1}))).await;
    }
}

struct UpsertChild(ModelState);

impl Model for UpsertChild {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_upsert_children")
            .unique_id(openrust::database::eloquent::UniqueId::Ulid);
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
}

async fn verify_related_upsert(db: &Connection) {
    db.statement("drop table if exists relation_upsert_children", &[])
        .await
        .unwrap();
    db.statement(
        "create table relation_upsert_children (
            id varchar(26) primary key, owner_id integer not null, owner_type varchar(30) not null,
            code varchar(30) not null, body varchar(30) not null,
            created_at varchar(30) not null, updated_at varchar(30) not null,
            unique (owner_type, owner_id, code)
        )",
        &[],
    )
    .await
    .unwrap();
    let parent = |id| {
        let mut parent = Author::new_instance_on(db, Attributes::new()).unwrap();
        parent.set_attribute("id", Value::Integer(id));
        parent
    };
    let first = parent(1);
    let second = parent(2);
    let make = |code: &str, body: &str| {
        Attributes::from([
            ("code".into(), code.into()),
            ("body".into(), body.into()),
            ("owner_id".into(), 99.into()),
            ("owner_type".into(), "direct".into()),
        ])
    };
    let unique = &["owner_type", "owner_id", "code"];
    let many = HasMany::<Author, UpsertChild>::new("id", "owner_id");
    let one = HasOne::<Author, UpsertChild>::new("id", "owner_id");
    let morph_many = MorphMany::<Author, UpsertChild>::new("id", "owner_id", "owner_type", "morph");
    let morph_one = MorphOne::<Author, UpsertChild>::new("id", "owner_id", "owner_type", "morph");

    assert_eq!(many.upsert(&first, vec![], &[], None).await.unwrap(), 0);
    assert_eq!(
        many.upsert(
            &first,
            vec![make("a", "first"), make("b", "other")],
            unique,
            None
        )
        .await
        .unwrap(),
        2
    );
    many.upsert(&second, vec![make("a", "second parent")], unique, None)
        .await
        .unwrap();
    morph_many
        .upsert(&first, vec![make("a", "first morph")], unique, None)
        .await
        .unwrap();
    let lookup = |owner_type: &'static str, id: i64| {
        let mut query = UpsertChild::query_on(db);
        query
            .r#where("owner_type", "=", owner_type)
            .r#where("owner_id", "=", id)
            .r#where("code", "=", "a");
        query.first_or_fail()
    };
    let original = lookup("direct", 1).await.unwrap();
    let mut update = make("a", "updated");
    update.insert("created_at".into(), "ignored on update".into());
    update.insert("updated_at".into(), "explicit timestamp".into());
    one.upsert(&first, vec![update], unique, Some(&["body"]))
        .await
        .unwrap();
    morph_one
        .upsert(&first, vec![make("a", "updated morph")], unique, None)
        .await
        .unwrap();
    let changed = lookup("direct", 1).await.unwrap();
    assert_eq!(original.get_attribute("id"), changed.get_attribute("id"));
    assert!(matches!(changed.get_attribute("id"), Some(Value::Text(id)) if id.len() == 26));
    assert_eq!(
        original.get_attribute("created_at"),
        changed.get_attribute("created_at")
    );
    assert_eq!(
        changed.get_attribute("updated_at"),
        Some(&"explicit timestamp".into())
    );
    assert_eq!(changed.get_attribute("body"), Some(&"updated".into()));
    assert_eq!(
        lookup("direct", 2).await.unwrap().get_attribute("body"),
        Some(&"second parent".into())
    );
    assert_eq!(
        lookup("morph", 1).await.unwrap().get_attribute("body"),
        Some(&"updated morph".into())
    );
    verify_forwarded_upsert(db, &first, &lookup).await;
    verify_inverse_upsert(db, &lookup).await;
    db.statement("drop table relation_upsert_children", &[])
        .await
        .unwrap();
}

async fn verify_forwarded_upsert<F>(
    db: &Connection,
    parent: &Author,
    lookup: &impl Fn(&'static str, i64) -> F,
) where
    F: std::future::Future<Output = DatabaseResult<UpsertChild>>,
{
    let through =
        HasManyThrough::<Author, Post, UpsertChild>::new("author_id", "owner_id", "id", "id");
    let through_one =
        HasOneThrough::<Author, Post, UpsertChild>::new("author_id", "owner_id", "id", "id");
    let pivot = BelongsToMany::<Author, UpsertChild>::new(
        "relation_upsert_links",
        "author_id",
        "child_id",
        "id",
        "id",
    );
    let morph = MorphToMany::<Author, UpsertChild>::new(
        "relation_upsert_links",
        "author_id",
        "child_id",
        "id",
        "id",
        "owner_type",
        "author",
    );
    db.statement("drop table if exists relation_upsert_links", &[])
        .await
        .unwrap();
    db.statement(
        "create table relation_upsert_links (
            author_id integer not null, child_id varchar(26) not null, owner_type varchar(30)
        )",
        &[],
    )
    .await
    .unwrap();
    let columns = &["owner_type", "owner_id", "code"];
    let row = |kind: &str, body: &str| {
        Attributes::from([
            ("owner_type".into(), kind.into()),
            ("owner_id".into(), 77.into()),
            ("code".into(), "a".into()),
            ("body".into(), body.into()),
        ])
    };
    through
        .upsert(parent, vec![row("through", "inserted")], columns, None)
        .await
        .unwrap();
    through_one
        .upsert(
            parent,
            vec![row("through", "updated")],
            columns,
            Some(&["body"]),
        )
        .await
        .unwrap();
    pivot
        .upsert(parent, vec![row("pivot", "inserted")], columns, None)
        .await
        .unwrap();
    morph
        .upsert(
            parent,
            vec![row("pivot", "updated")],
            columns,
            Some(&["body"]),
        )
        .await
        .unwrap();
    for kind in ["through", "pivot"] {
        let saved = lookup(kind, 77).await.unwrap();
        assert_eq!(saved.get_attribute("body"), Some(&"updated".into()));
        assert!(matches!(saved.get_attribute("id"), Some(Value::Text(id)) if id.len() == 26));
    }
    assert!(
        db.table("relation_upsert_links")
            .get()
            .await
            .unwrap()
            .is_empty()
    );
    let mut invalid = row("through", "missing key");
    invalid.remove("owner_id");
    assert!(
        through
            .upsert(parent, vec![invalid], columns, None)
            .await
            .is_err()
    );
    assert_eq!(
        lookup("through", 77).await.unwrap().get_attribute("body"),
        Some(&"updated".into())
    );
    db.statement("drop table relation_upsert_links", &[])
        .await
        .unwrap();
}

struct UpsertOwner(ModelState);

impl Model for UpsertOwner {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata =
            ModelMetadata::new("relation_upsert_owners").timestamps(None, None);
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
}

async fn verify_inverse_upsert<F>(db: &Connection, lookup: &impl Fn(&'static str, i64) -> F)
where
    F: std::future::Future<Output = DatabaseResult<UpsertChild>>,
{
    db.statement("drop table if exists relation_upsert_owners", &[])
        .await
        .unwrap();
    db.statement(
        "create table relation_upsert_owners (id integer primary key, body varchar(30) not null)",
        &[],
    )
    .await
    .unwrap();
    let mut child = Comment::new_instance_on(db, Attributes::new()).unwrap();
    child.set_attribute("subject_id", 999);
    let belongs_to = BelongsTo::<Comment, UpsertOwner>::new("subject_id", "id");
    let morph = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<UpsertOwner>("owner", "id")
        .target::<UpsertChild>("other", "id");
    let owner =
        |body: &str| Attributes::from([("id".into(), 42.into()), ("body".into(), body.into())]);
    assert_eq!(morph.upsert(&child, vec![], &[], None).await.unwrap(), 0);
    for (alias, message) in [
        (Value::Null, "missing morph type"),
        ("".into(), "missing morph type"),
        ("unregistered".into(), "unknown morph type"),
        (1.into(), "morph type column"),
    ] {
        child.set_attribute("subject_type", alias);
        assert!(matches!(
            morph.upsert(&child, vec![owner("invalid")], &["id"], None).await,
            Err(DatabaseError::InvalidQuery(error)) if error == message
        ));
    }
    assert!(
        db.table("relation_upsert_owners")
            .get()
            .await
            .unwrap()
            .is_empty()
    );

    belongs_to
        .upsert(&child, vec![owner("created")], &["id"], None)
        .await
        .unwrap();
    child.set_attribute("subject_type", "owner");
    morph
        .upsert(&child, vec![owner("updated")], &["id"], Some(&["body"]))
        .await
        .unwrap();
    let updated = UpsertOwner::query_on(db).find_or_fail(42).await.unwrap();
    assert_eq!(updated.get_attribute("body"), Some(&"updated".into()));

    child.set_attribute("subject_type", "other");
    let attributes = Attributes::from([
        ("owner_type".into(), "inverse".into()),
        ("owner_id".into(), 42.into()),
        ("code".into(), "a".into()),
        ("body".into(), "other target".into()),
    ]);
    morph
        .upsert(
            &child,
            vec![attributes],
            &["owner_type", "owner_id", "code"],
            None,
        )
        .await
        .unwrap();
    let other = lookup("inverse", 42).await.unwrap();
    assert!(matches!(other.get_attribute("id"), Some(Value::Text(id)) if id.len() == 26));
    assert_eq!(other.get_attribute("body"), Some(&"other target".into()));
    assert_eq!(child.get_attribute("subject_id"), Some(&999.into()));
    assert_eq!(child.get_attribute("subject_type"), Some(&"other".into()));
    db.statement("drop table relation_upsert_owners", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn related_upsert_on_sqlite() {
    verify_related_upsert(&connect(json!({
        "driver": "sqlite", "database": ":memory:", "max_connections": 1,
    })))
    .await;
}

#[tokio::test]
async fn related_upsert_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_RELATION_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_RELATION_MYSQL").is_none());
    if let Some(url) = url {
        verify_related_upsert(&connect(json!({"url": url, "max_connections": 1}))).await;
    }
}
