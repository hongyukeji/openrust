use std::{collections::BTreeMap, sync::Arc};

use openrust::database::eloquent::RelatedCollection;
use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Builder, Collection, DynamicRelation, EagerLoads, Model, ModelMetadata,
            ModelState, NamedRelations, RelationAggregate, RelationAggregateQuery, Touches,
            casts::AttributeCast,
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

    fn preventsLazyLoading(&self) -> bool {
        true
    }
}

struct AuthorWithDefaults(ModelState);

impl Model for AuthorWithDefaults {
    fn namedRelations() -> NamedRelations<Self> {
        NamedRelations::default().nested("posts", || HasMany::<Self, Post>::new("id", "author_id"))
    }

    fn automaticallyEagerLoadRelationships() -> bool {
        true
    }

    fn preventsLazyLoading(&self) -> bool {
        true
    }

    fn handleLazyLoadingViolationUsing(&self, violation: DatabaseError) -> DatabaseResult<()> {
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

    fn withDefaultRelations(relations: EagerLoads<Self>) -> EagerLoads<Self> {
        relations
            .with("posts", HasMany::<Self, Post>::new("id", "author_id"))
            .with("mentees", HasMany::<Self, Author>::new("id", "mentor_id"))
    }

    fn withDefaultCounts(
        query: Builder<'static, Self>,
    ) -> openrust::database::DatabaseResult<Builder<'static, Self>> {
        query.withCount("posts", &HasMany::<Self, Post>::new("id", "author_id"))
    }
}

impl Model for Author {
    fn namedRelations() -> NamedRelations<Self> {
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
    fn namedRelations() -> NamedRelations<Self> {
        NamedRelations::default()
            .nested("comments", || POST_COMMENTS)
            .nested("details", || POST_COMMENTS)
            .nested("author", || AUTHOR)
    }

    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_posts")
            .fillable(&["title", "score"])
            .timestamps(None, None)
            .softDeletes("deleted_at");
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
    fn namedRelations() -> NamedRelations<Self> {
        NamedRelations::default()
            .nested("post", || BelongsTo::<Self, Post>::new("post_id", "id"))
            .morphTo("subject", || {
                MorphTo::<Self>::new("subject_type", "subject_id")
                    .target::<Author>("author", "id")
                    .target::<Post>("post", "id")
            })
    }

    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("relation_comments")
            .fillable(&["body"])
            .timestamps(None, None)
            .softDeletes("deleted_at");
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
        .selectOne("SHOW SESSION STATUS LIKE 'Com_select'", &[])
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
    assert!(AUTHOR.isNot(post, None));
    assert!(FIRST_POST.is(author, Some(post)).await.unwrap());

    let mut child = Post::newInstance(Attributes::new()).unwrap();
    let mut owner = Author::newInstance(Attributes::new()).unwrap();
    child.setAttribute("owner_code", "+00042");
    owner.setAttribute("code", 42_i64);
    let by_code = BelongsTo::new("owner_code", "code");
    assert!(by_code.is(&child, Some(&owner)));
    child.setAttribute("owner_code", "42other");
    assert!(by_code.isNot(&child, Some(&owner)));
    child.setAttribute("owner_code", "0");
    owner.setAttribute("code", 0_i64);
    assert!(by_code.isNot(&child, Some(&owner)));
    let mut same_name_owner = Author::newInstanceOn(other, Attributes::new()).unwrap();
    same_name_owner.setAttribute("id", author.getKey().unwrap().clone());
    assert!(AUTHOR.isNot(post, Some(&same_name_owner)));

    let mut comment = Comment::newInstance(Attributes::new()).unwrap();
    comment.setAttribute("subject_id", author.getKey().unwrap().clone());
    comment.setAttribute("subject_type", "author");
    let subject = MorphTo::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id");
    let mut detached = Author::newInstance(Attributes::new()).unwrap();
    detached.setAttribute("id", author.getKey().unwrap().clone());
    assert!(subject.is(&comment, Some(&detached)));
    assert!(subject.isNot(&comment, Some(post)));
    comment.setAttribute("subject_type", "unknown");
    assert!(subject.isNot(&comment, Some(&detached)));
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
    let latest = POSTS.one().latestOfMany(None);
    assert!(latest.is(author, Some(&newest)).await.unwrap());
    assert!(latest.isNot(author, Some(post)).await.unwrap());
    assert!(latest.isNot(author, Some(hidden)).await.unwrap());
    newest.forceDelete().await.unwrap();
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
    visible.setAttribute("post_id", post.getKey().unwrap().clone());
    visible.save().await.unwrap();
    let mut hidden_comment = POST_COMMENTS
        .create(
            hidden,
            Attributes::from([("body".into(), "hidden parent".into())]),
        )
        .await
        .unwrap();
    hidden_comment.setAttribute("post_id", hidden.getKey().unwrap().clone());
    hidden_comment.save().await.unwrap();
    let mut deleted = POST_COMMENTS
        .create(post, Attributes::from([("body".into(), "deleted".into())]))
        .await
        .unwrap();
    deleted.setAttribute("post_id", post.getKey().unwrap().clone());
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
        profile.getAttribute("subject_id"),
        visible.getAttribute("subject_id")
    );
    let comments = Author::through(POSTS).has(HasMany::<Post, Comment>::new("id", "post_id"));
    let found = comments.getResults(author).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].getKey(), visible.getKey());
    assert_eq!(
        comments
            .withTrashedParents()
            .getResults(author)
            .await
            .unwrap()
            .len(),
        2,
    );
    let comment = Author::through(FIRST_POST).has(HasOne::<Post, Comment>::new("id", "post_id"));
    assert_eq!(
        comment.getResults(author).await.unwrap().unwrap().getKey(),
        visible.getKey()
    );
    let owner =
        Author::through(AUTHOR_COMMENT).has(HasMany::<Comment, Author>::new("subject_id", "id"));
    let found = owner.getResults(author).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].getKey(), author.getKey());
    let found = Author::queryOn(db)
        .has(&owner)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(found.modelKeys(), vec![author.getKey().unwrap().clone()]);
    assert!(AUTHOR_COMMENT.is(author, Some(&profile)).await.unwrap());
    assert!(
        AUTHOR_COMMENT
            .latestOfMany(None)
            .isNot(author, Some(&visible))
            .await
            .unwrap()
    );
    assert_eq!(POST_COMMENTS.getResults(post).await.unwrap().len(), 1);
    assert_eq!(
        POST_COMMENTS
            .one()
            .latestOfMany(None)
            .getResults(post)
            .await
            .unwrap()
            .unwrap()
            .getKey(),
        visible.getKey(),
    );
    assert_eq!(
        AUTHOR_COMMENT
            .getResults(author)
            .await
            .unwrap()
            .unwrap()
            .getKey(),
        profile.getKey()
    );
    assert_eq!(
        FIRST_COMMENT_THROUGH
            .getResults(author)
            .await
            .unwrap()
            .unwrap()
            .getKey(),
        visible.getKey()
    );

    let baseline = if mysql { select_count(db).await } else { 0 };
    let authors = Author::queryOn(db)
        .with(
            "recentComments",
            COMMENTS_THROUGH.constrain(|query| query.withCasts([("id", AttributeCast::String)])),
        )
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }
    assert!(matches!(
        authors.all()[0]
            .getRelation::<RelatedCollection<Comment>>("recentComments")
            .unwrap()[0]
            .getAttributeValue("id")
            .unwrap(),
        Some(Value::Text(_))
    ));
    assert_eq!(
        authors
            .iter()
            .map(|model| model
                .getRelation::<RelatedCollection<Comment>>("recentComments")
                .unwrap()
                .len())
            .collect::<Vec<_>>(),
        vec![1, 0, 0]
    );
    let before_projection = if mysql { select_count(db).await } else { 0 };
    {
        let json: Json = serde_json::from_str(&authors.toJson().unwrap()).unwrap();
        assert_eq!(json[0]["recent_comments"][0]["body"], "visible");
        assert_eq!(json[1]["recent_comments"], json!([]));
        assert!(
            !authors.all()[0]
                .attributesToArray()
                .unwrap()
                .contains_key("recent_comments")
        );
        let mut first = authors.into_iter().next().unwrap();
        first.makeHidden(&["recentComments"]);
        assert!(!first.toArray().unwrap().contains_key("recent_comments"));
    }
    if mysql {
        assert_eq!(select_count(db).await, before_projection);
    }
    assert_eq!(
        COMMENTS_THROUGH
            .withTrashedParents()
            .getResults(author)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        Author::queryOn(db)
            .has(&COMMENTS_THROUGH)
            .unwrap()
            .get()
            .await
            .unwrap()
            .modelKeys(),
        vec![1.into()]
    );

    let subjects = || {
        MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Author>("author", "id")
            .target::<Post>("post", "id")
    };
    let baseline = if mysql { select_count(db).await } else { 0 };
    let comments = Comment::queryOn(db)
        .with("subject", subjects())
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 3);
    }
    let mut comments = comments.into_iter();
    let first = comments.next().unwrap();
    let target = first
        .getRelation::<Option<MorphTarget>>("subject")
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(target.alias(), "post");
    assert!(first.toArray().unwrap()["subject"].is_object());
    assert_eq!(
        target.downcast_ref::<Post>().unwrap().getKey(),
        post.getKey()
    );
    let second = comments.next().unwrap();
    assert_eq!(second.toArray().unwrap()["subject"], Json::Null);
    assert!(
        second
            .getRelation::<Option<MorphTarget>>("subject")
            .unwrap()
            .is_none()
    );
    let third = comments.next().unwrap();
    assert_eq!(
        third
            .getRelation::<Option<MorphTarget>>("subject")
            .unwrap()
            .as_ref()
            .unwrap()
            .downcast_ref::<Author>()
            .unwrap()
            .getKey(),
        author.getKey()
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
    let valid = Comment::queryOn(db)
        .whereHasMorph(&subject, &["post", "author"])
        .unwrap()
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(
        valid.modelKeys(),
        vec![
            visible.getKey().unwrap().clone(),
            profile.getKey().unwrap().clone()
        ]
    );
    let missing = Comment::queryOn(db)
        .whereDoesntHaveMorph(&subject, &["post"])
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(
        missing.modelKeys(),
        vec![hidden_comment.getKey().unwrap().clone()]
    );
    assert!(
        Comment::queryOn(db)
            .whereHasMorph(&subject, &["unknown"])
            .is_err()
    );
    let missing = Comment::queryOn(db)
        .hasMorphCount(&subject, &["post"], "=", 0)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(
        missing.modelKeys(),
        vec![hidden_comment.getKey().unwrap().clone()]
    );
    let counted_subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id")
        .morphWithPaths::<Post>("post", &["author"])
        .unwrap()
        .morphWithCount::<Post>("post", "id", "comments", POST_COMMENTS)
        .morphWith::<Post>("post", |loads| loads.with("comments", POST_COMMENTS))
        .unwrap()
        .constrain::<Post>("post", "id", |mut query| {
            query.r#where("score", ">", 0);
            query
        });
    let counted = Comment::queryOn(db)
        .with("subject", counted_subjects)
        .findOrFail(visible.getKey().unwrap().clone())
        .await
        .unwrap();
    let counted_post = counted
        .getRelation::<Option<MorphTarget>>("subject")
        .unwrap()
        .as_ref()
        .unwrap()
        .downcast_ref::<Post>()
        .unwrap();
    assert_eq!(counted_post.getAttribute("comments_count"), Some(&1.into()));
    assert!(counted_post.relationLoaded("author"));
    assert!(counted_post.relationLoaded("comments"));
    assert!(matches!(
        MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Post>("post", "id")
            .morphWithPaths::<Author>("post", &["posts"]),
        Err(DatabaseError::InvalidQuery(
            "morph type does not match model"
        ))
    ));
    Box::pin(verify_morph_with_constraint(db, visible.getKey().unwrap())).await;

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
        .setAttribute("subject_type", "missing")
        .setAttribute("subject_id", 1);
    assert!(subject.getResults(&profile).await.is_err());
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
    let key = author.getKey().unwrap().clone();
    post_tags
        .attachWith(
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
    assert_eq!(tagged_posts.getResults(author).await.unwrap().len(), 1);
    assert_eq!(
        Author::queryOn(db)
            .has(&author_tags)
            .unwrap()
            .get()
            .await
            .unwrap()
            .modelKeys(),
        vec![1.into()]
    );
    assert!(
        post_tags
            .updateExistingPivot(
                post,
                &key,
                Attributes::from([("subject_type".into(), "author".into())])
            )
            .await
            .is_err()
    );
    assert_eq!(
        Post::queryOn(db)
            .whereAttachedTo(&post_tags, std::slice::from_ref(author))
            .unwrap()
            .get()
            .await
            .unwrap()
            .modelKeys(),
        vec![1.into()]
    );
    let mut tagged = Post::queryOn(db);
    tagged.r#where("id", "=", 2).orWhere("id", "=", 3);
    assert_eq!(
        tagged
            .orWhereAttachedTo(&post_tags, std::slice::from_ref(author))
            .unwrap()
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap()
            .modelKeys(),
        vec![1.into(), 2.into()]
    );
    let tags = post_tags
        .withTimestampColumns(None, Some("linked_at"))
        .unwrap()
        .orderByPivotDesc("linked_at")
        .withPivot(["label"])
        .getResults(post)
        .await
        .unwrap();
    let pivot = tags[0].getRelation::<MorphPivot>("pivot").unwrap();
    assert_eq!(pivot.getAttribute("subject_type"), Some(&"post".into()));
    assert_eq!(pivot.getAttribute("label"), Some(&"post link".into()));
    assert_eq!(pivot.getAttribute("linked_at"), Some(&Value::Null));
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
        .wherePivot("label", "=", "post link")
        .orWherePivotNull("label");
    assert_eq!(post_tags.sync(post, &[]).await.unwrap().detached, vec![key]);
    assert!(tagged_posts.getResults(author).await.unwrap().is_empty());
    assert_eq!(author_tags.getResults(author).await.unwrap().len(), 1);
}

fn verify_relation_identity(db: &Connection) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let author = Author::queryOn(db).findOrFail(1).await.unwrap();
        let post = Post::queryOn(db).findOrFail(1).await.unwrap();
        let other = Author::queryOn(db).findOrFail(3).await.unwrap();
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
            let mut query = Comment::queryOn(db);
            query.r#where("body", "like", "identity %");
            query.orderBy("body", "asc")
        };
        let post_id = relation.identity("post", &post).unwrap();
        let author_id = relation.identity("author", &author).unwrap();
        assert!(relation.identity("author", &post).is_err());
        let found = scoped()
            .whereMorphedTo(&relation, &[post_id, author_id])
            .unwrap()
            .get()
            .await
            .unwrap();
        let bodies = |models: &Collection<Comment>| {
            models
                .iter()
                .map(|model| model.getAttribute("body").cloned().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            bodies(&found),
            vec!["identity author".into(), "identity post".into()]
        );
        let negated = scoped()
            .whereNotMorphedTo(&relation, &[relation.identity("post", &post).unwrap()])
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
            .whereMorphType(&relation, None)
            .get()
            .await
            .unwrap();
        assert_eq!(bodies(&unset), vec!["identity blank".into()]);
        assert!(scoped().whereMorphedTo(&relation, &[]).is_err());

        let custom =
            MorphTo::<Comment>::new("subject_type", "body").target::<Author>("author", "name");
        let id = custom.identity("author", &author).unwrap();
        assert!(
            scoped()
                .whereMorphedTo(&relation, std::slice::from_ref(&id))
                .is_err()
        );
        let matched = Comment::queryOn(db)
            .whereMorphedTo(&custom, &[id])
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(bodies(&matched), vec!["First".into()]);

        let mut disjunction = Comment::queryOn(db);
        disjunction.r#where("body", "=", "identity blank");
        let found = disjunction
            .orWhereMorphedTo(&relation, &[relation.identity("author", &author).unwrap()])
            .unwrap()
            .orderBy("body", "asc")
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

        let mut disjunction = Comment::queryOn(db);
        disjunction.r#where("body", "=", "identity post");
        let found = disjunction
            .orWhereNotMorphedTo(&relation, &[relation.identity("post", &post).unwrap()])
            .unwrap()
            .get()
            .await
            .unwrap();
        assert!(bodies(&found).contains(&"identity post".into()));
        assert!(bodies(&found).contains(&"identity author".into()));
        assert!(!bodies(&found).contains(&"identity null key".into()));

        let by_name = BelongsTo::<Comment, Author>::new("body", "name");
        let matched = Comment::queryOn(db)
            .whereBelongsTo(&by_name, std::slice::from_ref(&author))
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(bodies(&matched), vec!["First".into()]);
        let mut disjunction = Comment::queryOn(db);
        disjunction.r#where("body", "=", "identity blank");
        let found = disjunction
            .orWhereBelongsTo(&by_name, &[author])
            .unwrap()
            .orderBy("body", "asc")
            .get()
            .await
            .unwrap();
        assert_eq!(
            bodies(&found),
            vec!["First".into(), "identity blank".into()]
        );
        FEATURED_POSTS.attach(&other, &[1.into()]).await.unwrap();
        let matched = Author::queryOn(db)
            .whereAttachedTo(&FEATURED_POSTS, &[post])
            .unwrap()
            .get()
            .await
            .unwrap();
        assert!(matched.modelKeys().contains(&3.into()));
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
    edited.setAttribute("name", "Local owner");
    let baseline = if mysql { select_count(db).await } else { 0 };

    let unsaved = Author::newInstance(Attributes::new()).unwrap();
    assert!(matches!(
        relation.associate(&mut model, &unsaved),
        Err(DatabaseError::InvalidQuery("relationship owner key"))
    ));
    assert_eq!(model.getAttribute("author_id"), first.getKey());
    relation.associate(&mut model, &edited).unwrap();
    assert_eq!(model.getAttribute("author_id"), second.getKey());
    assert!(model.relationLoaded("comments"));
    assert_eq!(model.toArray().unwrap()["author"]["name"], "Local owner");
    assert!(
        model
            .getRelationOrLoad("author", &relation)
            .await
            .unwrap()
            .is_some()
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 0);
    }

    relation.associateKey(&mut model, first.getKey().unwrap().clone());
    assert!(!model.relationLoaded("author"));
    assert!(model.relationLoaded("comments"));
    assert_eq!(
        model
            .getRelationOrLoad("author", &relation)
            .await
            .unwrap()
            .as_ref()
            .unwrap()
            .getKey(),
        first.getKey()
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }

    relation.disassociate(&mut model);
    assert_eq!(model.getAttribute("author_id"), Some(&Value::Null));
    assert!(model.relationLoaded("author") && model.relationLoaded("comments"));
    assert!(
        model
            .getRelationOrLoad("author", &relation)
            .await
            .unwrap()
            .is_none()
    );
    assert!(model.toArray().unwrap()["author"].is_null());
    assert_eq!(
        Post::queryOn(db)
            .findOrFail(post.getKey().unwrap().clone())
            .await
            .unwrap()
            .getAttribute("author_id"),
        first.getKey()
    );
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
    snapshot.setAttribute("title", "Unpersisted subject title");
    let baseline = if mysql { select_count(db).await } else { 0 };
    assert!(subject.associate(profile, "post", author).is_err());
    assert_eq!(
        profile
            .getRelation::<Option<MorphTarget>>("subject")
            .unwrap()
            .as_ref()
            .unwrap()
            .alias(),
        "author",
    );
    let mut foreign = Post::newInstanceOn(other, Attributes::new()).unwrap();
    foreign.setAttribute("id", 1);
    assert!(matches!(
        subject.associate(profile, "post", &foreign),
        Err(DatabaseError::InvalidQuery(
            "mixed relationship connections"
        ))
    ));
    subject.associate(profile, "post", &snapshot).unwrap();
    assert!(profile.relationLoaded("post"));
    let target = profile
        .getRelationOrLoad("subject", &subject)
        .await
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(target.alias(), "post");
    assert_eq!(
        target.downcast_ref::<Post>().unwrap().getAttribute("title"),
        Some(&"Unpersisted subject title".into())
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 0);
    }
    assert_eq!(
        profile.toArray().unwrap()["subject"]["title"],
        "Unpersisted subject title"
    );
    let key = profile.getKey().unwrap().clone();
    let stored = Comment::queryOn(db).findOrFail(key.clone()).await.unwrap();
    assert_eq!(stored.getAttribute("subject_type"), Some(&"author".into()));

    profile.save().await.unwrap();
    assert_eq!(POST_COMMENTS.getResults(post).await.unwrap().len(), 2);
    subject.dissociate(profile);
    assert!(profile.relationLoaded("post"));
    assert!(profile.relationLoaded("subject"));
    assert!(
        profile
            .getRelationOrLoad("subject", &subject)
            .await
            .unwrap()
            .is_none()
    );
    assert!(profile.toArray().unwrap()["subject"].is_null());
    profile.save().await.unwrap();
    let stored = Comment::queryOn(db).findOrFail(key).await.unwrap();
    assert_eq!(stored.getAttribute("subject_type"), Some(&Value::Null));
    assert_eq!(stored.getAttribute("subject_id"), Some(&Value::Null));
}

async fn verify_morph_defaults(db: &Connection, visible: &Comment, hidden: &Comment) {
    let defaults = || {
        MorphTo::<Comment>::new("subject_type", "subject_id")
            .target::<Post>("post", "id")
            .target::<Author>("author", "id")
            .withDefault::<Post>("post", |post, parent| {
                post.setAttribute("title", parent.getAttribute("body").unwrap().clone());
            })
            .unwrap()
            .withDefault::<Author>("author", |author, _| {
                author.setAttribute("name", "Guest");
            })
            .unwrap()
    };

    let mut query = Comment::queryOn(db).with("subject", defaults());
    query.whereIn(
        "id",
        vec![
            visible.getKey().unwrap().clone(),
            hidden.getKey().unwrap().clone(),
        ],
    );
    let parents = Box::pin(query.get()).await.unwrap();
    for (parent, exists) in [(visible, true), (hidden, false)] {
        let row = parents.find(parent.getKey().unwrap()).unwrap().unwrap();
        let target = row
            .getRelation::<Option<MorphTarget>>("subject")
            .unwrap()
            .as_ref()
            .unwrap();
        let post = target.downcast_ref::<Post>().unwrap();
        assert_eq!(post.state().exists(), exists);
        if !exists {
            assert!(post.getKey().is_none());
            assert_eq!(post.getConnectionName(), parent.getConnectionName());
            assert_eq!(row.toArray().unwrap()["subject"]["title"], "hidden parent");
        }
    }
    let mut missing = Comment::from_state(hidden.state().clone());
    missing.setAttribute("subject_id", 999999);
    let result = defaults().getResults(&missing).await.unwrap().unwrap();
    assert!(!result.downcast_ref::<Post>().unwrap().state().exists());
    missing.setAttribute("subject_type", Value::Null);
    assert!(defaults().getResults(&missing).await.unwrap().is_none());
    let result = defaults()
        .defaultForMissingType("author")
        .unwrap()
        .getResults(&missing)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.alias(), "author");
    assert_eq!(
        result
            .downcast_ref::<Author>()
            .unwrap()
            .getAttribute("name"),
        Some(&"Guest".into())
    );
    missing.setAttribute("subject_type", "unknown");
    assert!(matches!(
        defaults().getResults(&missing).await,
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
        (morph().withTrashed(), [true, true, true]),
        (morph().withoutTrashed(), [true, false, true]),
        (morph().onlyTrashed(), [false, true, true]),
        (morph().onlyTrashed().withTrashed(), [false, true, true]),
        (morph().withoutTrashed().onlyTrashed(), [false, false, true]),
        (
            morph()
                .withTrashed()
                .constrain::<Post>("post", "id", Builder::withoutTrashed),
            [true, false, true],
        ),
        (
            morph()
                .onlyTrashed()
                .constrain::<Post>("post", "id", Builder::withTrashed),
            [false, true, true],
        ),
    ] {
        let mut query = Comment::queryOn(db).with("subject", relation);
        let parents = [visible, hidden, profile];
        query.whereIn(
            "id",
            parents
                .iter()
                .map(|model| model.getKey().unwrap().clone())
                .collect(),
        );
        let rows = Box::pin(query.get()).await.unwrap();
        for (parent, loaded) in parents.into_iter().zip(expected) {
            let row = rows.find(parent.getKey().unwrap()).unwrap().unwrap();
            let target = row.getRelation::<Option<MorphTarget>>("subject").unwrap();
            assert_eq!(target.is_some(), loaded);
        }
    }

    let target = morph()
        .withTrashed()
        .getResults(hidden)
        .await
        .unwrap()
        .unwrap();
    assert!(target.downcast_ref::<Post>().unwrap().trashed());
    assert!(
        morph()
            .withoutTrashed()
            .getResults(hidden)
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
        .morphWithPaths::<Post>("post", &["author"])
        .unwrap()
        .morphWithCount::<Post>("post", "id", "comments", POST_COMMENTS);
    let comment = Comment::queryOn(db)
        .with("subject", morph)
        .findOrFail(key.clone())
        .await
        .unwrap();
    assert!(
        comment
            .getRelation::<Option<MorphTarget>>("subject")
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
            .getRelation::<Option<MorphTarget>>("subject")
            .unwrap()
            .clone()
    };
    let index = comments
        .iter()
        .position(|comment| {
            subject(comment).is_some_and(|target| {
                target.alias() == "post"
                    && target.downcast_ref::<Post>().unwrap().getKey() == Some(&1.into())
            })
        })
        .unwrap();
    let before = subject(&comments[index]).unwrap();
    assert!(
        !before
            .downcast_ref::<Post>()
            .unwrap()
            .getRelation::<RelatedCollection<Comment>>("comments")
            .unwrap()
            .is_empty()
    );
    assert!(comments[index].toArray().unwrap()["subject"]["comments"].is_array());
    let author = comments
        .iter()
        .find_map(|comment| subject(comment).filter(|target| target.alias() == "author"))
        .unwrap();
    assert!(
        !author
            .downcast_ref::<Author>()
            .unwrap()
            .relationLoaded("comments")
    );

    let empty = POST_COMMENTS.constrain(|mut query| {
        query.r#where("id", "<", 0);
        query
    });
    let baseline = if mysql { select_count(db).await } else { 0 };
    comments
        .loadMorph("subject", subjects, "post", "comments", &empty)
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    assert_eq!(
        comments[index].toArray().unwrap()["subject"]["comments"],
        json!([])
    );
    assert!(
        !before
            .downcast_ref::<Post>()
            .unwrap()
            .getRelation::<RelatedCollection<Comment>>("comments")
            .unwrap()
            .is_empty()
    );

    let invalid = POST_COMMENTS.constrain(|mut query| {
        query.r#where("relation_comments.missing_comment_column", "=", 1);
        query
    });
    assert!(
        comments
            .loadMorph("subject", subjects, "post", "comments", &invalid)
            .await
            .is_err()
    );
    assert_eq!(
        comments[index].toArray().unwrap()["subject"]["comments"],
        json!([])
    );

    comments
        .loadMorphPaths::<Post>("subject", subjects, "post", &["comments.post"])
        .await
        .unwrap();
    let nested = subject(&comments[index]).unwrap();
    let children = nested
        .downcast_ref::<Post>()
        .unwrap()
        .getRelation::<RelatedCollection<Comment>>("comments")
        .unwrap();
    assert!(!children.is_empty());
    assert!(
        children
            .iter()
            .all(|comment| comment.relationLoaded("post"))
    );
    let projected = comments[index].toArray().unwrap();
    assert!(projected["subject"]["comments"].is_array());

    let mut single = Comment::from_state(comments[index].state().clone());
    single
        .loadMorph("subject", subjects, "post", "comments", &empty)
        .await
        .unwrap();
    assert_eq!(single.toArray().unwrap()["subject"]["comments"], json!([]));
    assert_eq!(comments[index].toArray().unwrap(), projected);
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
        .loadMorphAggregates("subject", subjects, "post", &aggregates)
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    let mut post_index = None;
    for (index, comment) in comments.iter().enumerate() {
        let subject = comment
            .getRelation::<Option<MorphTarget>>("subject")
            .unwrap();
        match subject.as_ref().map(MorphTarget::alias) {
            Some("post") => {
                let post = subject.as_ref().unwrap().downcast_ref::<Post>().unwrap();
                let has_feedback = post.getAttribute("last_feedback").unwrap() != &Value::Null;
                assert_eq!(
                    post.getAttribute("has_feedback"),
                    Some(&has_feedback.into())
                );
                assert_eq!(
                    post.getRawOriginal("last_feedback"),
                    post.getAttribute("last_feedback")
                );
                post_index = Some(index);
            }
            Some("author") => {
                let author = subject.as_ref().unwrap().downcast_ref::<Author>().unwrap();
                assert!(author.getAttribute("posts_count").is_some());
                assert!(author.getAttribute("last_feedback").is_none());
            }
            None => {}
            Some(alias) => panic!("unexpected alias {alias}"),
        }
    }
    let row = &comments[post_index.unwrap()];
    let mut model = Comment::from_state(row.state().clone());
    model
        .loadMorphCount(
            "subject",
            subjects,
            "post",
            "comments as feedback_count",
            &relation,
        )
        .await
        .unwrap();
    assert!(model.toArray().unwrap()["subject"]["feedback_count"].is_u64());
    assert!(
        row.toArray().unwrap()["subject"]
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
            .loadMorphAggregates("subject", subjects, "post", &invalid)
            .await,
        Err(DatabaseError::InvalidQuery("duplicate aggregate alias"))
    ));
    assert!(matches!(
        comments
            .loadMorphAggregates("subject", subjects, "author", &aggregates)
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
        let mut comments = Comment::queryOn(db)
            .with("subject", subjects())
            .get()
            .await
            .unwrap();
        assert!(
            comments
                .morphModels::<Author>("subject", &subjects(), "post")
                .await
                .is_err()
        );
        let baseline = if mysql { select_count(db).await } else { 0 };
        {
            let mut posts = comments
                .loadMorph("subject", &subjects(), "post", "comments", &POST_COMMENTS)
                .await
                .unwrap();
            assert_eq!(posts.all().len(), 2);
            let first = posts
                .all()
                .iter()
                .position(|model| model.getKey() == Some(&1.into()))
                .unwrap();
            let comments = posts
                .getRelationOrLoad(first, "comments", &POST_COMMENTS)
                .await
                .unwrap();
            assert!(!comments.is_empty());
        }
        {
            let authors = comments
                .loadMorphCount("subject", &subjects(), "author", "posts", &POSTS)
                .await
                .unwrap();
            assert_eq!(authors.all().len(), 1);
            assert_eq!(authors.all()[0].getKey(), Some(&1.into()));
            assert!(authors.getAttribute(0, "posts_count").is_some());
            assert!(authors.getAttribute(0, "comments_count").is_none());
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
        let mut roots = Comment::queryOn(db)
            .with("subject", subjects())
            .get()
            .await
            .unwrap()
            .withRelationshipAutoloading();
        let post_indexes: Vec<_> = roots
            .iter()
            .enumerate()
            .filter_map(|(index, root)| {
                let target = root
                    .getRelation::<Option<MorphTarget>>("subject")?
                    .as_ref()?;
                (target.alias() == "post").then_some(index)
            })
            .collect();
        assert!(post_indexes.len() >= 2);
        let baseline = if mysql { select_count(db).await } else { 0 };
        for &index in &post_indexes {
            let mut root = roots.modelAt(index).unwrap();
            assert!(
                root.morphModel::<Author>("subject", &morph, "author")
                    .await
                    .unwrap()
                    .is_none()
            );
            let mut post = root
                .morphModel::<Post>("subject", &morph, "post")
                .await
                .unwrap()
                .unwrap();
            post.getRelationOrLoad("comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        for root in roots.iter() {
            let Some(target) = root.getRelation::<Option<MorphTarget>>("subject").unwrap() else {
                continue;
            };
            match target.alias() {
                "post" => assert!(
                    target
                        .downcast_ref::<Post>()
                        .unwrap()
                        .relationLoaded("comments")
                ),
                "author" => assert!(
                    !target
                        .downcast_ref::<Author>()
                        .unwrap()
                        .relationLoaded("comments")
                ),
                _ => unreachable!(),
            }
        }

        let author_relation = AUTHOR;
        let posts_relation = POSTS;
        let baseline = if mysql { select_count(db).await } else { 0 };
        for &index in &post_indexes {
            let mut root = roots.modelAt(index).unwrap();
            let mut post = root
                .morphModel::<Post>("subject", &morph, "post")
                .await
                .unwrap()
                .unwrap();
            let mut author = post
                .relatedModelAt("author", &author_relation, 0)
                .await
                .unwrap()
                .unwrap();
            assert!(
                !author
                    .getRelationOrLoad("posts", &posts_relation)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        for index in post_indexes {
            let subject = &roots.toArray().unwrap()[index]["subject"];
            assert!(subject["author"]["posts"].is_array());
        }

        let mut moved_posts = Vec::new();
        for mut root in Comment::queryOn(db)
            .with("subject", subjects())
            .get()
            .await
            .unwrap()
            .withRelationshipAutoloading()
        {
            if let Some(post) = root
                .getRelationOrLoad("subject", &morph)
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
            post.getRelationOrLoad("comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }

        let mut authors = Author::queryOn(db).get().await.unwrap();
        let mut comments = authors
            .relatedModels("comments", &COMMENTS_THROUGH)
            .await
            .unwrap();
        let posts = comments
            .morphModels::<Post>("subject", &subjects(), "post")
            .await
            .unwrap();
        assert!(!posts.all().is_empty());
    })
}

async fn verify_path_constraints(db: &Connection) {
    let path = "posts.comments.post";
    let baseline = Author::queryOn(db)
        .whereHasPath(path)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(!baseline.is_empty());
    let column = Author::queryOn(db)
        .whereRelationPath(path, "id", ">", 0)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(column.modelKeys(), baseline.modelKeys());
    let closure = Author::queryOn(db)
        .whereRelationPathWith(path, |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "<", 0).orWhere(
                &format!("{table}.id"),
                ">",
                0,
            )
        })
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(closure.modelKeys(), baseline.modelKeys());
    let mut either = Author::queryOn(db);
    either.whereKey(0);
    assert_eq!(
        either
            .orWhereRelationPath(path, "id", ">", 0)
            .unwrap()
            .get()
            .await
            .unwrap()
            .modelKeys(),
        baseline.modelKeys()
    );
    let all = Author::queryOn(db).get().await.unwrap();
    let missing = Author::queryOn(db)
        .whereDoesntHaveRelationPath(path, "id", "<", 0)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(missing.modelKeys(), all.modelKeys());
    let mut either_missing = Author::queryOn(db);
    either_missing.whereKey(0);
    assert_eq!(
        either_missing
            .orWhereDoesntHaveRelationPath(path, "id", "<", 0)
            .unwrap()
            .get()
            .await
            .unwrap()
            .modelKeys(),
        all.modelKeys()
    );

    let selected_comment = Comment::queryOn(db)
        .whereHasPath("post")
        .unwrap()
        .orderBy("id", "asc")
        .firstOrFail()
        .await
        .unwrap();
    let id = selected_comment.getKey().unwrap().clone();
    let loaded = Author::queryOn(db)
        .withWhereRelationPathWith("posts.comments", move |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "=", id.clone())
        })
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(loaded.len(), 1);
    let posts = loaded[0]
        .getRelation::<RelatedCollection<Post>>("posts")
        .unwrap();
    let comments: Vec<_> = posts
        .iter()
        .flat_map(|post| {
            post.getRelation::<RelatedCollection<Comment>>("comments")
                .unwrap()
                .iter()
        })
        .collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].getKey(), selected_comment.getKey());
    assert!(
        Author::queryOn(db)
            .whereRelationPath("posts.bad", "id", "=", 1)
            .is_err()
    );
}

async fn verify_model_reload_relations(db: &Connection, model: &Author) {
    let fresh = model
        .freshWithPaths(&["posts.comments"])
        .await
        .unwrap()
        .unwrap();
    assert!(fresh.relationLoaded("posts"));
    assert_eq!(fresh.getKey(), model.getKey());
    let mut author = Author::queryOn(db)
        .withPaths(&["posts"])
        .unwrap()
        .findOrFail(model.getKey().unwrap().clone())
        .await
        .unwrap();
    let before = author
        .getRelation::<RelatedCollection<Post>>("posts")
        .unwrap();
    let cached = before[0].clone();
    author.refresh().await.unwrap();
    let after = author
        .getRelation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert!(!Arc::ptr_eq(&cached, &after[0]));

    let mut tx = db.beginTransaction().await.unwrap();
    let post_key = after[0].getKey().unwrap().clone();
    tx.table("relation_posts")
        .r#where("id", "=", post_key.clone())
        .update(Attributes::from([(
            "title".into(),
            "Uncommitted title".into(),
        )]))
        .await
        .unwrap();
    author.refreshForUpdate(&mut tx).await.unwrap();
    let posts = author
        .getRelation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert_eq!(
        posts
            .iter()
            .find(|post| post.getKey() == Some(&post_key))
            .unwrap()
            .getAttribute("title"),
        Some(&"Uncommitted title".into()),
    );
    tx.rollBack(None).await.unwrap();

    author.load("unregistered", &POSTS).await.unwrap();
    let prior_name = author.getRawOriginal("name").unwrap().clone();
    let key = author.getKey().unwrap().clone();
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
    assert_eq!(author.getAttribute("name"), Some(&"Database reload".into()));
    assert_eq!(author.getRawOriginal("name"), Some(&prior_name));
    db.table("relation_authors")
        .r#where("id", "=", key)
        .update(Attributes::from([("name".into(), prior_name)]))
        .await
        .unwrap();
}

async fn verify_eager_columns(db: &Connection, mysql: bool) {
    assert!(matches!(
        Author::queryOn(db).withPaths(&["posts:id,"]),
        Err(DatabaseError::InvalidQuery("invalid relationship columns"))
    ));
    let baseline = if mysql { select_count(db).await } else { 0 };
    let author = Author::queryOn(db)
        .withPaths(&[
            "posts:id,author_id,title",
            "posts.comments:id,subject_id,body",
        ])
        .unwrap()
        .findOrFail(1)
        .await
        .unwrap();
    let posts = author
        .getRelation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert!(!posts.is_empty());
    let mut comments_count = 0;
    for post in posts {
        assert!(post.getAttribute("title").is_some());
        assert!(post.getAttribute("score").is_none());
        assert_eq!(post.getAttribute("author_id"), author.getKey());
        for comment in post
            .getRelation::<RelatedCollection<Comment>>("comments")
            .unwrap()
        {
            assert_eq!(comment.getAttribute("subject_id"), post.getKey());
            assert!(comment.getAttribute("body").is_some());
            assert!(comment.getAttribute("subject_type").is_none());
            comments_count += 1;
        }
    }
    assert!(comments_count > 0);
    if mysql {
        assert_eq!(select_count(db).await - baseline, 3);
    }

    let mut author = Author::queryOn(db).findOrFail(1).await.unwrap();
    author
        .loadMissingPaths(&["posts:id,author_id"])
        .await
        .unwrap();
    author
        .loadMissingPaths(&["posts:id,author_id,title"])
        .await
        .unwrap();
    let posts = author
        .getRelation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert!(
        posts
            .iter()
            .all(|post| post.getAttribute("title").is_none())
    );

    let post_id = posts[0].getKey().unwrap().clone();
    FEATURED_POSTS
        .syncWithoutDetaching(&author, std::slice::from_ref(&post_id))
        .await
        .unwrap();
    let joined = Author::queryOn(db)
        .withPaths(&["featured:id,title", "commentsThrough:id,body"])
        .unwrap()
        .findOrFail(1)
        .await
        .unwrap();
    let featured = joined
        .getRelation::<RelatedCollection<Post>>("featured")
        .unwrap();
    let post = featured
        .iter()
        .find(|post| post.getKey() == Some(&post_id))
        .unwrap();
    assert!(post.getAttribute("author_id").is_none());
    assert!(post.getRelation::<Pivot>("pivot").is_some());
    let through = joined
        .getRelation::<RelatedCollection<Comment>>("commentsThrough")
        .unwrap();
    assert!(!through.is_empty());
    assert!(
        through
            .iter()
            .all(|comment| comment.getAttribute("post_id").is_none())
    );

    let comment = Comment::queryOn(db)
        .withPaths(&["subject:id"])
        .unwrap()
        .whereMorphType(
            &MorphTo::<Comment>::new("subject_type", "subject_id").target::<Post>("post", "id"),
            Some("post"),
        )
        .firstOrFail()
        .await
        .unwrap();
    let owner = comment
        .getRelation::<Option<openrust::database::eloquent::relations::MorphTarget>>("subject")
        .unwrap()
        .as_ref()
        .unwrap()
        .downcast_ref::<Post>()
        .unwrap();
    assert_eq!(owner.getKey(), comment.getAttribute("subject_id"));
    assert!(owner.getAttribute("title").is_none());
}

async fn verify_named_paths(db: &Connection, mysql: bool) {
    assert!(matches!(
        Author::queryOn(db).withPaths(&["posts..comments"]),
        Err(DatabaseError::InvalidQuery("invalid relationship path"))
    ));
    assert!(matches!(
        Author::queryOn(db).withPaths(&["posts.unknown"]),
        Err(DatabaseError::InvalidQuery("unknown relationship path"))
    ));
    let matching = Author::queryOn(db)
        .whereHasPath("posts.comments.post")
        .unwrap()
        .get()
        .await
        .unwrap();
    let missing = Author::queryOn(db)
        .whereDoesntHavePath("posts.comments.post")
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(!matching.is_empty());
    assert!(!missing.is_empty());
    assert!(matching.iter().all(|author| {
        missing
            .iter()
            .all(|other| other.getKey() != author.getKey())
    }));
    assert_eq!(
        matching.len() + missing.len(),
        Author::queryOn(db).get().await.unwrap().len()
    );
    let one_comment_post = Author::queryOn(db)
        .hasPathCount("posts.comments.post", "=", 1)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(one_comment_post.len(), matching.len());
    let two_comment_posts = Author::queryOn(db)
        .hasPathCount("posts.comments.post", ">=", 2)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(two_comment_posts.is_empty());
    assert!(matches!(
        Author::queryOn(db).whereHasPath("posts."),
        Err(DatabaseError::InvalidQuery("invalid relationship path"))
    ));
    assert!(matches!(
        Author::queryOn(db).whereDoesntHavePath("posts.unknown"),
        Err(DatabaseError::InvalidQuery("unknown relationship path"))
    ));
    let baseline = if mysql { select_count(db).await } else { 0 };
    let mut authors = Author::queryOn(db)
        .withPaths(&["posts", "posts.comments", "posts.comments", "posts.author"])
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
                .getRelation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .is_empty()
        })
        .unwrap();
    Box::pin(verify_model_reload_relations(db, with_posts)).await;
    let metadata_baseline = if mysql { select_count(db).await } else { 0 };
    assert_eq!(
        authors.getQueueableClass(),
        Some(std::any::type_name::<Author>())
    );
    assert_eq!(authors.getQueueableIds(), authors.modelKeys());
    assert_eq!(authors.getQueueableConnection().unwrap(), Some("relations"));
    let shared = authors.getQueueableRelations();
    assert!(shared.contains(&"posts".into()));
    for author in authors.iter() {
        assert!(
            shared
                .iter()
                .all(|path| author.getQueueableRelations().contains(path))
        );
    }
    if mysql {
        assert_eq!(select_count(db).await - metadata_baseline, 0);
    }
    let mut found_comments = false;
    for author in authors.iter() {
        for post in author
            .getRelation::<RelatedCollection<Post>>("posts")
            .unwrap()
        {
            assert!(post.relationLoaded("author"));
            found_comments |= !post
                .getRelation::<RelatedCollection<Comment>>("comments")
                .unwrap()
                .is_empty();
        }
    }
    assert!(found_comments);
    let baseline = if mysql { select_count(db).await } else { 0 };
    authors
        .loadMissingPaths(&["posts.comments.post", "posts.author"])
        .await
        .unwrap();
    authors
        .loadMissingPaths(&["posts.comments.post"])
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    let mut found = false;
    for author in authors.iter() {
        for post in author
            .getRelation::<RelatedCollection<Post>>("posts")
            .unwrap()
        {
            for comment in post
                .getRelation::<RelatedCollection<Comment>>("comments")
                .unwrap()
            {
                assert!(comment.relationLoaded("post"));
                found = true;
            }
        }
    }
    assert!(found);
    let mut model = Author::queryOn(db).firstOrFail().await.unwrap();
    model.loadPaths(&["posts.comments"]).await.unwrap();
    model
        .loadMissingPaths(&["posts.comments.post"])
        .await
        .unwrap();
    assert!(model.relationLoaded("posts"));
    let mut loaded = Author::queryOn(db)
        .whereHasPath("posts")
        .unwrap()
        .firstOrFail()
        .await
        .unwrap();
    loaded.loadPaths(&["posts.author"]).await.unwrap();
    loaded.makeHidden(&["posts"]);
    assert_eq!(loaded.getQueueableRelations(), ["posts", "posts.author"]);
    let mut other = Author::from_state(loaded.state().clone());
    other.unsetRelation("posts");
    other.loadPaths(&["posts"]).await.unwrap();
    let key = loaded.getQueueableId().unwrap().clone();
    let models = Collection::new(vec![loaded, other]);
    assert_eq!(models.getQueueableIds(), [key.clone(), key]);
    assert_eq!(models.getQueueableRelations(), ["posts"]);
    let mixed = Collection::new(vec![
        Author::from_state(models[0].state().clone()),
        Author::newInstance(Attributes::new()).unwrap(),
    ]);
    assert!(matches!(
        mixed.getQueueableConnection(),
        Err(DatabaseError::InvalidQuery(
            "queueable collection has multiple connections"
        ))
    ));
    assert_eq!(mixed.getQueueableIds()[1], Value::Null);
    let empty = Collection::<Author>::new(vec![]);
    assert_eq!(empty.getQueueableClass(), None);
    assert_eq!(empty.getQueueableConnection().unwrap(), None);
    assert!(empty.getQueueableIds().is_empty());
    assert!(empty.getQueueableRelations().is_empty());
}

async fn verify_morph_paths(db: &Connection, mysql: bool) {
    let author = Author::queryOn(db)
        .whereHasPath("posts")
        .unwrap()
        .firstOrFail()
        .await
        .unwrap();
    let table = || db.table("relation_comments");
    let profile = table()
        .insertGetId(Attributes::from([
            ("body".into(), "named morph profile".into()),
            ("subject_type".into(), "author".into()),
            ("subject_id".into(), author.getKey().unwrap().clone()),
        ]))
        .await
        .unwrap();
    let no_target = table()
        .insertGetId(Attributes::from([(
            "body".into(),
            "named morph null".into(),
        )]))
        .await
        .unwrap();
    let post = Post::queryOn(db)
        .whereHasPath("comments")
        .unwrap()
        .firstOrFail()
        .await
        .unwrap();
    let additional = table()
        .insertGetId(Attributes::from([
            ("body".into(), "named morph counted".into()),
            ("subject_type".into(), "post".into()),
            ("subject_id".into(), post.getKey().unwrap().clone()),
        ]))
        .await
        .unwrap();
    let selected = Comment::queryOn(db)
        .whereRelationPath("subject", "id", "=", author.getKey().unwrap().clone())
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(selected.find(&profile).unwrap().is_some());
    assert!(selected.find(&no_target).unwrap().is_none());
    let baseline = if mysql { select_count(db).await } else { 0 };
    let comments = Comment::queryOn(db)
        .orderBy("id", "asc")
        .withPaths(&["subject", "subject.details", "subject.details"])
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
            .getRelation::<Option<MorphTarget>>("subject")
            .unwrap()
        else {
            assert_eq!(comment.getQueueableRelations(), ["subject"]);
            continue;
        };
        assert!(
            comment
                .getQueueableRelations()
                .contains(&"subject.details".into())
        );
        aliases.insert(target.alias());
        let len = if let Some(author) = target.downcast_ref::<Author>() {
            author
                .getRelation::<RelatedCollection<Post>>("details")
                .unwrap()
                .len()
        } else {
            target
                .downcast_ref::<Post>()
                .unwrap()
                .getRelation::<RelatedCollection<Comment>>("details")
                .unwrap()
                .len()
        };
        if len > 0 {
            expected.push(comment.getKey().unwrap().clone());
        }
        if len >= 2 {
            counted.push(comment.getKey().unwrap().clone());
        }
    }
    let mut deferred = Comment::queryOn(db)
        .withPaths(&["subject"])
        .unwrap()
        .get()
        .await
        .unwrap();
    let baseline = if mysql { select_count(db).await } else { 0 };
    deferred
        .loadMissingPaths(&["subject.details"])
        .await
        .unwrap();
    deferred
        .loadMissingPaths(&["subject.details"])
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }
    let projected = deferred.toArray().unwrap();
    assert!(
        projected
            .iter()
            .any(|comment| comment["subject"].get("details").is_some())
    );
    assert_eq!(aliases, ["post", "author"].into_iter().collect());
    assert!(!expected.is_empty());
    assert!(!counted.is_empty());
    let matched = Comment::queryOn(db)
        .whereHasPath("subject.details")
        .unwrap()
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    let absent = Comment::queryOn(db)
        .whereDoesntHavePath("subject.details")
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(!absent.is_empty());
    assert_eq!(matched.len() + absent.len(), comments.len());
    assert_eq!(matched.modelKeys(), expected);
    assert_eq!(
        Comment::queryOn(db)
            .hasPathCount("subject.details", ">=", 2)
            .unwrap()
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap()
            .modelKeys(),
        counted
    );
    assert!(matches!(
        Comment::queryOn(db).withPaths(&["subject.comments"]),
        Err(DatabaseError::InvalidQuery("unknown relationship path"))
    ));
    let baseline = if mysql { select_count(db).await } else { 0 };
    let discovered = Comment::queryOn(db)
        .whereHasPathAll("subject.details")
        .await
        .unwrap()
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(discovered.modelKeys(), expected);
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }
    assert_eq!(
        Comment::queryOn(db)
            .hasPathCountAll("subject.details", ">=", 2)
            .await
            .unwrap()
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap()
            .modelKeys(),
        counted
    );
    let missing = Comment::queryOn(db)
        .whereDoesntHavePathAll("subject.details")
        .await
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(missing.len(), absent.len());
    assert!(missing.modelKeys().contains(&no_target));
    Box::pin(verify_discovered_path_targets(db, &expected)).await;
    let unknown = table()
        .insertGetId(Attributes::from([
            ("body".into(), "unknown named morph".into()),
            ("subject_type".into(), "unknown".into()),
            ("deleted_at".into(), "2026-01-01 00:00:00".into()),
        ]))
        .await
        .unwrap();
    assert!(matches!(
        Comment::queryOn(db)
            .whereHasPathAll("subject.details")
            .await,
        Err(DatabaseError::InvalidQuery("unknown morph type"))
    ));
    table()
        .whereIn("id", vec![unknown, profile.clone()])
        .delete()
        .await
        .unwrap();
    let only_posts = Comment::queryOn(db)
        .whereHasPathAll("subject.comments")
        .await
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(!only_posts.is_empty());
    Box::pin(verify_discovered_path_filter(db, &additional, &no_target)).await;
    assert!(absent.modelKeys().contains(&no_target));
    db.table("relation_comments")
        .whereIn("id", vec![no_target, additional])
        .delete()
        .await
        .unwrap();
}

async fn verify_discovered_path_targets(db: &Connection, expected: &[Value]) {
    let query = Comment::queryOn(db)
        .withWhereRelationPathAll("subject.details", "id", ">", 0)
        .await
        .unwrap()
        .orderBy("id", "asc");
    let mixed = Box::pin(query.get()).await.unwrap();
    assert_eq!(mixed.modelKeys(), expected);
    let mut aliases = std::collections::HashSet::new();
    for row in mixed.toArray().unwrap() {
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
    let query = Comment::queryOn(db)
        .whereRelationPathAll(path, "body", "=", body)
        .await
        .unwrap()
        .orderBy("id", "asc");
    let filtered = Box::pin(query.get()).await.unwrap();
    assert!(filtered.modelKeys().contains(additional));

    let query = Comment::queryOn(db)
        .whereDoesntHaveRelationPathAll(path, "body", "=", body)
        .await
        .unwrap();
    let absent = Box::pin(query.get()).await.unwrap();
    assert!(absent.modelKeys().contains(no_target));
    assert!(!absent.modelKeys().contains(additional));

    let query = Comment::queryOn(db)
        .withWhereRelationPathAllWith(path, move |conditions, qualifier| {
            conditions.r#where(&format!("{qualifier}.body"), "=", body)
        })
        .await
        .unwrap()
        .orderBy("id", "asc");
    let loaded = Box::pin(query.get()).await.unwrap();
    assert_eq!(loaded.modelKeys(), filtered.modelKeys());
    for row in loaded.toArray().unwrap() {
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
        let mut authors = Author::queryOn(db)
            .with("posts", POSTS)
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap();
        let mut posts = authors.relatedModels("posts", &POSTS).await.unwrap();
        assert!(posts.all().len() > 1);
        let broken = POST_COMMENTS.constrain(|mut query| {
            query.r#where("relation_comments.unknown_relation_column", "=", "error");
            query
        });
        assert!(
            posts
                .getRelationOrLoad(0, "comments", &broken)
                .await
                .is_err()
        );
        let baseline = if mysql { select_count(db).await } else { 0 };
        for index in 0..posts.all().len() {
            posts
                .getRelationOrLoad(index, "comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        let mut comments = posts
            .relatedModels("comments", &POST_COMMENTS)
            .await
            .unwrap();
        assert!(comments.all().len() >= 2);
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        let post = BelongsTo::<Comment, Post>::new("post_id", "id");
        let linked = comments.relatedModels("post", &post).await.unwrap();
        assert!(linked.all().len() >= 2);
        assert!(authors.iter().all(|author| author.relationLoaded("posts")));

        let mut posts = Post::queryOn(db)
            .with("author", AUTHOR)
            .get()
            .await
            .unwrap();
        let owners = posts.relatedModels("author", &AUTHOR).await.unwrap();
        let unique = owners
            .all()
            .iter()
            .map(|owner| owner.getKey().unwrap().clone())
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
        let ordered = || StrictAuthor::queryOn(db).orderBy("id", "asc");
        let mut strict = ordered().findOrFail(1).await.unwrap();
        let mut group = ordered().get().await.unwrap();
        let baseline = if mysql { select_count(db).await } else { 0 };
        assert!(matches!(
            strict.getRelationOrLoad("posts", &posts).await,
            Err(DatabaseError::LazyLoadingViolation { model, relation })
                if model == std::any::type_name::<StrictAuthor>() && relation == "posts"
        ));
        assert!(matches!(
            group.getRelationOrLoad(0, "posts", &posts).await,
            Err(DatabaseError::LazyLoadingViolation { .. })
        ));
        assert!(!strict.relationLoaded("posts"));
        assert!(!group[0].relationLoaded("posts"));
        if mysql {
            assert_eq!(select_count(db).await - baseline, 0);
        }

        strict.load("posts", &posts).await.unwrap();
        let baseline = if mysql { select_count(db).await } else { 0 };
        assert!(
            !strict
                .getRelationOrLoad("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        let mut group = group.withRelationshipAutoloading();
        assert!(
            group
                .getRelationOrLoad(2, "posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(group.iter().all(|model| model.relationLoaded("posts")));
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }

        let mut fresh = StrictAuthor::newInstance(Attributes::new()).unwrap();
        assert!(
            fresh
                .getRelationOrLoad("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        let mut created = ordered()
            .create(Attributes::from([("name".into(), "New author".into())]))
            .await
            .unwrap();
        assert!(created.state().wasRecentlyCreated());
        assert!(
            created
                .getRelationOrLoad("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );
        created.delete().await.unwrap();

        let mut allowed = AuthorWithDefaults::queryOn(db)
            .withOnly(|loads| loads)
            .findOrFail(1)
            .await
            .unwrap();
        let allowed_posts = HasMany::<AuthorWithDefaults, Post>::new("id", "author_id");
        assert!(
            !allowed
                .getRelationOrLoad("posts", &allowed_posts)
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
        let ordered = || Author::queryOn(db).orderBy("id", "asc");
        let mut plain = ordered().get().await.unwrap();
        plain.getRelationOrLoad(0, "posts", &POSTS).await.unwrap();
        assert!(plain[0].relationLoaded("posts"));
        assert!(!plain[1].relationLoaded("posts"));
        let mut group = plain.withRelationshipAutoloading().except(&[]).unwrap();
        let broken = POSTS.constrain(|mut query| {
            query.r#where("relation_posts.unknown_relation_column", "=", "error");
            query
        });
        assert!(group.modelAt(20).is_none());
        assert!(
            group
                .modelAt(1)
                .unwrap()
                .getRelationOrLoad("posts", &broken)
                .await
                .is_err()
        );
        assert!(group[0].relationLoaded("posts"));
        assert!(!group[1].relationLoaded("posts"));
        assert!(!group[2].relationLoaded("posts"));

        let baseline = if mysql { select_count(db).await } else { 0 };
        {
            let mut author = group.modelAt(1).unwrap();
            assert_eq!(author.getKey(), Some(&2.into()));
            author.getRelationOrLoad("posts", &POSTS).await.unwrap();
        }
        assert!(
            group
                .getRelationOrLoad(2, "posts", &POSTS)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(group.iter().all(|author| author.relationLoaded("posts")));
        assert!(group.getRelationOrLoad(20, "posts", &POSTS).await.is_err());
        let wrong_output = HasOne::<Author, Post>::new("id", "author_id");
        assert!(
            group
                .getRelationOrLoad(0, "posts", &wrong_output)
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
            .withRelationshipAutoloading()
            .into_iter();
        let mut first = detached.next().unwrap();
        let mut second = detached.next().unwrap();
        let mut empty = detached.next().unwrap();
        drop(detached);
        assert!(first.getRelationOrLoad("posts", &broken).await.is_err());
        assert!(!first.relationLoaded("posts"));
        let baseline = if mysql { select_count(db).await } else { 0 };
        let posts = POSTS;
        let first_posts = first.getRelationOrLoad("posts", &posts).await.unwrap();
        let mut first_post = Post::from_state(first_posts[0].state().clone());
        assert!(second.relationLoaded("posts"));
        assert!(second.getQueueableRelations().contains(&"posts".into()));
        assert!(second.toArray().unwrap()["posts"].is_array());
        let second_posts = second
            .getRelation::<RelatedCollection<Post>>("posts")
            .unwrap();
        let mut second_post = Post::from_state(second_posts[0].state().clone());
        assert!(empty.relationLoaded("posts"));
        assert!(
            empty
                .getRelationOrLoad("posts", &posts)
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
                .getRelationOrLoad("comments", &bad_comments)
                .await
                .is_err()
        );
        assert!(!second_post.relationLoaded("comments"));
        let baseline = if mysql { select_count(db).await } else { 0 };
        let comments = POST_COMMENTS;
        let (left, right) = tokio::join!(
            first_post.getRelationOrLoad("comments", &comments),
            second_post.getRelationOrLoad("comments", &comments)
        );
        left.unwrap();
        right.unwrap();
        assert!(second.toArray().unwrap()["posts"][0]["comments"].is_array());
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        first.unsetRelation("posts");
        assert!(!first.relationLoaded("posts"));
        assert!(!first.toArray().unwrap().contains_key("posts"));
        assert!(second.relationLoaded("posts"));
        assert!(second.getQueueableRelations().contains(&"posts".into()));
        assert!(first.getRelationOrLoad("posts", &broken).await.is_err());
        second.unsetRelation("posts");
        second.setAttribute("id", 3);
        assert!(
            second
                .getRelationOrLoad("posts", &posts)
                .await
                .unwrap()
                .is_empty()
        );

        let mut other = ordered().get().await.unwrap();
        assert!(other.iter().all(|author| !author.relationLoaded("posts")));
        let posts = POSTS;
        let mut descendant = ordered().get().await.unwrap();
        descendant
            .getRelationOrLoad(0, "posts", &POSTS)
            .await
            .unwrap();
        let mut descendant = descendant.withRelationshipAutoloading();
        let broken_comments = POST_COMMENTS.constrain(|mut query| {
            query.r#where("relation_comments.unknown_relation_column", "=", "error");
            query
        });
        {
            let mut author = descendant.modelAt(0).unwrap();
            let mut post = author
                .relatedModelAt("posts", &posts, 0)
                .await
                .unwrap()
                .unwrap();
            assert!(
                post.getRelationOrLoad("comments", &broken_comments)
                    .await
                    .is_err()
            );
            assert!(!post.relationLoaded("comments"));
        }
        let baseline = if mysql { select_count(db).await } else { 0 };
        for index in [0, 1] {
            let mut author = descendant.modelAt(index).unwrap();
            let mut post = author
                .relatedModelAt("posts", &posts, 0)
                .await
                .unwrap()
                .unwrap();
            post.getRelationOrLoad("comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        let json = descendant.toArray().unwrap();
        assert!(json[0]["posts"][0].get("comments").is_some());
        assert!(json[1]["posts"][0].get("comments").is_some());
        assert!(
            descendant
                .modelAt(2)
                .unwrap()
                .relatedModelAt("posts", &posts, 0)
                .await
                .unwrap()
                .is_none()
        );
        {
            let mut author = other.modelAt(0).unwrap();
            let mut post = author
                .relatedModelAt("posts", &posts, 0)
                .await
                .unwrap()
                .unwrap();
            post.getRelationOrLoad("comments", &POST_COMMENTS)
                .await
                .unwrap();
        }
        assert!(!other[1].relationLoaded("posts"));
        assert!(
            other.toArray().unwrap()[0]["posts"][0]
                .get("comments")
                .is_some()
        );
        other
            .modelAt(2)
            .unwrap()
            .getRelationOrLoad("posts", &POSTS)
            .await
            .unwrap();
        assert!(!other[1].relationLoaded("posts"));
        let mut single = ordered().findOrFail(3).await.unwrap();
        assert!(
            single
                .getRelationOrLoad("posts", &POSTS)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            single
                .getRelationOrLoad("posts", &broken)
                .await
                .unwrap()
                .is_empty()
        );

        let mut opted_in = AuthorWithDefaults::queryOn(db)
            .withOnly(|loads| loads)
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap();
        let posts = HasMany::<AuthorWithDefaults, Post>::new("id", "author_id");
        assert!(!opted_in[0].relationLoaded("posts"));
        opted_in
            .getRelationOrLoad(0, "posts", &posts)
            .await
            .unwrap();
        assert!(opted_in.iter().all(|author| author.relationLoaded("posts")));
    })
}

fn verify_default_models(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let empty = Author::queryOn(db).findOrFail(3).await.unwrap();
        let blank_post = Post::newInstance(Attributes::new()).unwrap();
        let default_owner = AUTHOR.withDefault(|owner, post| {
            assert!(post.getAttribute("author_id").is_none());
            owner.setAttribute("name", "Guest author");
        });
        let fallback = default_owner
            .getResults(&blank_post)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fallback.getAttribute("name"), Some(&"Guest author".into()));
        assert!(fallback.getKey().is_none());
        assert!(!fallback.state().exists());

        let newest = POSTS.one().latestOfMany(None).withDefault(|post, parent| {
            assert_eq!(post.getAttribute("author_id"), parent.getKey());
            post.setAttribute("title", "No posts yet");
        });
        let baseline = if mysql { select_count(db).await } else { 0 };
        let authors = Author::queryOn(db)
            .with("newest", newest)
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        let newest = |index: usize| {
            authors[index]
                .getRelation::<Option<Arc<Post>>>("newest")
                .unwrap()
                .as_ref()
                .unwrap()
        };
        assert!(newest(0).state().exists());
        assert_eq!(
            newest(2).getAttribute("title"),
            Some(&"No posts yet".into())
        );
        assert_eq!(newest(2).getConnectionName(), Some(db.getName()));
        assert!(!newest(2).state().exists());

        let through = FIRST_COMMENT_THROUGH.withDefault(|_, _| {});
        let through_default = through.getResults(&empty).await.unwrap().unwrap();
        assert!(!through_default.state().exists());
        assert!(through_default.getAttribute("post_id").is_none());
        assert!(
            Author::queryOn(db)
                .whereDoesntHave(&through)
                .unwrap()
                .get()
                .await
                .unwrap()
                .modelKeys()
                .contains(&3.into())
        );

        let morph = AUTHOR_COMMENT.withDefault(|comment, _| {
            assert_eq!(comment.getAttribute("subject_type"), Some(&"author".into()));
        });
        let morph_default = morph.getResults(&empty).await.unwrap().unwrap();
        assert_eq!(morph_default.getAttribute("subject_id"), empty.getKey());
        assert!(!morph_default.state().exists());

        let mut no_owner = Post::queryOn(db).make(Attributes::new()).unwrap();
        no_owner.setAttribute("author_id", 999_999);
        let filtered = AUTHOR.withDefault(|model, _| {
            model.setAttribute("name", "Missing owner");
        });
        let missing = filtered.getResults(&no_owner).await.unwrap().unwrap();
        assert_eq!(missing.getConnectionName(), Some(db.getName()));
        assert_eq!(missing.getAttribute("name"), Some(&"Missing owner".into()));
        assert!(missing.getKey().is_none());
    })
}

fn verify_default_eager_loads(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let ordered = || AuthorWithDefaults::queryOn(db).orderBy("id", "asc");
        let posts = || HasMany::<AuthorWithDefaults, Post>::new("id", "author_id");
        let baseline = if mysql { select_count(db).await } else { 0 };
        let models = ordered().get().await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 3);
        }
        assert!(
            models
                .iter()
                .all(|model| model.relationLoaded("posts") && model.relationLoaded("mentees"))
        );
        assert!(
            !models[0]
                .getRelation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .is_empty()
        );
        assert!(
            models[2]
                .getRelation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .is_empty()
        );

        let baseline = if mysql { select_count(db).await } else { 0 };
        let without_posts = ordered().without(&["posts"]).findOrFail(1).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        assert!(!without_posts.relationLoaded("posts"));
        assert!(without_posts.relationLoaded("mentees"));
        assert!(without_posts.getAttribute("posts_count").is_some());
        let only = ordered()
            .withOnly(|relations| relations.with("other_posts", posts()))
            .findOrFail(1)
            .await
            .unwrap();
        assert!(only.relationLoaded("other_posts"));
        assert!(!only.relationLoaded("posts"));
        assert!(!only.relationLoaded("mentees"));
        assert!(only.getAttribute("posts_count").is_some());
        let projected = ordered()
            .withOnly(|loads| loads)
            .select(["id"])
            .findOrFail(1)
            .await
            .unwrap();
        assert!(projected.getAttribute("posts_count").is_none());

        let narrow = || {
            posts().constrain(|mut query| {
                query.r#where("title", "=", "no matching posts");
                query
            })
        };
        let explicit = ordered()
            .with("posts", narrow())
            .findOrFail(1)
            .await
            .unwrap();
        assert!(
            explicit
                .getRelation::<RelatedCollection<Post>>("posts")
                .unwrap()
                .is_empty()
        );
        assert!(explicit.relationLoaded("mentees"));
        let source = Collection::new(vec![
            ordered()
                .withOnly(|loads| loads)
                .findOrFail(1)
                .await
                .unwrap(),
            ordered()
                .withOnly(|loads| loads)
                .findOrFail(1)
                .await
                .unwrap(),
        ]);
        let baseline = if mysql { select_count(db).await } else { 0 };
        let fresh = source.freshWith("posts", &narrow()).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 3);
        }
        assert_eq!(fresh.len(), 2);
        assert!(
            fresh
                .iter()
                .all(|model| model.getAttribute("posts_count").is_some())
        );
        assert!(fresh.iter().all(|model| {
            model.relationLoaded("mentees")
                && model
                    .getRelation::<RelatedCollection<Post>>("posts")
                    .unwrap()
                    .is_empty()
        }));

        let baseline = if mysql { select_count(db).await } else { 0 };
        let named =
            Box::pin(source.freshWithPaths(&["posts.comments", "posts.author", "posts.comments"]))
                .await
                .unwrap();
        assert_eq!(named.modelKeys(), source.modelKeys());
        if mysql {
            assert_eq!(select_count(db).await - baseline, 5);
        }
        for model in named.iter() {
            assert!(model.relationLoaded("mentees"));
            let posts = model
                .getRelation::<RelatedCollection<Post>>("posts")
                .unwrap();
            assert!(!posts.is_empty());
            assert!(
                posts
                    .iter()
                    .all(|post| post.relationLoaded("comments") && post.relationLoaded("author"))
            );
        }
        assert!(source.iter().all(|model| !model.relationLoaded("posts")));
        assert!(matches!(
            Box::pin(source.freshWithPaths(&["posts.invalid"])).await,
            Err(DatabaseError::InvalidQuery("unknown relationship path"))
        ));
        assert!(
            Box::pin(Collection::<AuthorWithDefaults>::new(vec![]).freshWithPaths(&["posts"]))
                .await
                .unwrap()
                .is_empty()
        );

        let baseline = if mysql { select_count(db).await } else { 0 };
        let mut keyed = source.toQuery().unwrap().get().await.unwrap();
        assert!(keyed[0].getAttribute("posts_count").is_none());
        keyed.loadCount("posts", &posts()).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        assert!(!keyed[0].relationLoaded("posts"));
        assert!(keyed[0].getAttribute("posts_count").is_some());

        let via_posts = HasManyThrough::<Author, Post, AuthorWithDefaults>::new(
            "author_id",
            "id",
            "id",
            "author_id",
        )
        .constrain(|query| query.withOnly(|loads| loads));
        let plain_author = Author::queryOn(db).findOrFail(1).await.unwrap();
        let matched = via_posts.getResults(&plain_author).await.unwrap();
        assert!(!matched.is_empty());
        assert!(
            matched
                .iter()
                .all(|model| model.getAttribute("posts_count").is_some())
        );
    })
}

fn verify_related_column_queries(db: &Connection) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let filtered = Author::queryOn(db)
            .withWhereRelation(
                "posts",
                POSTS.constrain(|mut query| {
                    query
                        .r#where("title", "=", "First post")
                        .orWhere("title", "=", "Second post");
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
        assert_eq!(filtered.modelKeys(), [2.into()]);
        let loaded = filtered[0]
            .getRelation::<RelatedCollection<Post>>("posts")
            .unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].getAttribute("title"), Some(&"Second post".into()));
        let mut either = Author::queryOn(db);
        either.whereKey(3);
        assert_eq!(
            either
                .orWhereRelation(POSTS, "score", ">", 15)
                .unwrap()
                .orderBy("id", "asc")
                .get()
                .await
                .unwrap()
                .modelKeys(),
            [2.into(), 3.into()],
        );
        let mut either_missing = Author::queryOn(db);
        either_missing.whereKey(2);
        assert_eq!(
            either_missing
                .orWhereDoesntHaveRelation(POSTS, "score", ">", 15)
                .unwrap()
                .orderBy("id", "asc")
                .get()
                .await
                .unwrap()
                .modelKeys(),
            [1.into(), 2.into(), 3.into()],
        );
        assert_eq!(
            Author::queryOn(db)
                .whereDoesntHaveRelation(POSTS, "score", ">", 15)
                .unwrap()
                .orderBy("id", "asc")
                .get()
                .await
                .unwrap()
                .modelKeys(),
            [1.into(), 3.into()],
        );
        assert_eq!(
            Author::queryOn(db)
                .whereRelation(MENTEES, "id", "=", 2)
                .unwrap()
                .get()
                .await
                .unwrap()
                .modelKeys(),
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
            let mut query = Comment::queryOn(db);
            query.r#where("body", "like", "wild %");
            query
        };
        let column_match = selected()
            .whereMorphRelation(subjects, &["author", "post"], "id", "=", 1)
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(column_match.len(), 1);
        assert_eq!(
            column_match[0].getAttribute("body"),
            Some(&"wild live".into())
        );
        let column_absent = selected()
            .whereMorphDoesntHaveRelation(subjects, &["post"], "id", "=", 1)
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(column_absent.len(), 1);
        assert_eq!(
            column_absent[0].getAttribute("body"),
            Some(&"wild missing".into())
        );
        let mut either = Comment::queryOn(db);
        either.r#where("body", "=", "wild null");
        let either = either
            .orWhereMorphRelation(subjects, &["author"], "id", "=", 1)
            .unwrap()
            .get()
            .await
            .unwrap();
        assert!(
            either
                .iter()
                .any(|model| model.getAttribute("body") == Some(&"wild live".into()))
        );
        assert!(
            either
                .iter()
                .any(|model| model.getAttribute("body") == Some(&"wild null".into()))
        );
        assert!(
            !either
                .iter()
                .any(|model| model.getAttribute("body") == Some(&"wild missing".into()))
        );
        let mut either_missing = Comment::queryOn(db);
        either_missing.r#where("body", "=", "wild null");
        let either_missing = either_missing
            .orWhereMorphDoesntHaveRelation(subjects, &["post"], "id", "=", 1)
            .unwrap()
            .get()
            .await
            .unwrap();
        assert!(
            either_missing
                .iter()
                .any(|model| model.getAttribute("body") == Some(&"wild missing".into()))
        );
        assert!(
            either_missing
                .iter()
                .any(|model| model.getAttribute("body") == Some(&"wild null".into()))
        );
        assert!(
            !either_missing
                .iter()
                .any(|model| model.getAttribute("body") == Some(&"wild live".into()))
        );
        assert!(
            selected()
                .whereMorphRelation(subjects, &["unknown"], "id", "=", 1)
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
            .tryConstrain::<Author>("unused", "id", |_| {
                Err(openrust::database::DatabaseError::InvalidQuery(
                    "unused morph query",
                ))
            });
        db.insert("INSERT INTO relation_comments (body, subject_type, subject_id) VALUES (?, ?, ?), (?, ?, ?), (?, ?, ?)",
            &["wild live".into(), "author".into(), 1.into(),
              "wild missing".into(), "post".into(), 999_999.into(),
              "wild null".into(), Value::Null, Value::Null]).await.unwrap();
        let selected = || {
            let mut query = Comment::queryOn(db);
            query.r#where("body", "like", "wild %");
            query.orderBy("body", "asc")
        };
        verify_morph_column_queries(db, &subjects).await;
        let baseline = if mysql { select_count(db).await } else { 0 };
        let present = selected()
            .whereHasMorphAll(&subjects)
            .await
            .unwrap()
            .get()
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        assert_eq!(present.len(), 1);
        assert_eq!(present[0].getAttribute("body"), Some(&"wild live".into()));
        let absent = selected()
            .whereDoesntHaveMorphAll(&subjects)
            .await
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(
            absent
                .iter()
                .map(|comment| comment.getAttribute("body").cloned().unwrap())
                .collect::<Vec<_>>(),
            vec!["wild missing".into(), "wild null".into()]
        );
        let zero = selected()
            .hasMorphCountAll(&subjects, "=", 0)
            .await
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(zero.modelKeys(), absent.modelKeys());

        db.insert("INSERT INTO relation_comments (body, subject_type, subject_id, deleted_at) VALUES (?, ?, ?, ?)",
            &["hidden unknown".into(), "unregistered".into(), 1.into(), "2026-01-01 00:00:00".into()]).await.unwrap();
        assert!(matches!(
            selected().whereHasMorphAll(&subjects).await,
            Err(openrust::database::DatabaseError::InvalidQuery(
                "unknown morph type"
            ))
        ));

        db.statement("UPDATE relation_comments SET subject_type = NULL", &[])
            .await
            .unwrap();
        assert!(
            selected()
                .whereHasMorphAll(&subjects)
                .await
                .unwrap()
                .get()
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            selected()
                .whereDoesntHaveMorphAll(&subjects)
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
            .getResults(author)
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
        tied.setAttribute("post_id", second_post.getKey().unwrap().clone());
        tied.save().await.unwrap();

        assert!(
            FIRST_COMMENT_THROUGH
                .is(author, Some(&first))
                .await
                .unwrap()
        );
        assert!(FIRST_COMMENT_THROUGH.is(author, Some(&tied)).await.unwrap());
        assert!(FIRST_COMMENT_THROUGH.isNot(author, None).await.unwrap());
        assert!(
            COMMENTS_THROUGH
                .one()
                .latestOfMany(None)
                .is(author, Some(&tied))
                .await
                .unwrap()
        );
        assert!(
            COMMENTS_THROUGH
                .one()
                .latestOfMany(None)
                .isNot(author, Some(&first))
                .await
                .unwrap()
        );
        let latest = || COMMENTS_THROUGH.one().latestOfMany(Some("body"));
        let baseline = if mysql { select_count(db).await } else { 0 };
        let authors = Author::queryOn(db)
            .with("latest_comment", latest())
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        let loaded = |index: usize| {
            authors[index]
                .getRelation::<Option<Arc<Comment>>>("latest_comment")
                .unwrap()
        };
        assert_eq!(loaded(0).as_ref().unwrap().getKey(), tied.getKey());
        assert!(loaded(1).is_none());
        assert!(loaded(2).is_none());

        let oldest = COMMENTS_THROUGH
            .one()
            .oldestOfMany(None)
            .getResults(author)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(oldest.getKey(), first.getKey());
        let first_key = first.getKey().unwrap().clone();
        let constrained = latest()
            .whereCandidates(move |query| query.r#where("id", "=", first_key.clone()))
            .unwrap();
        assert_eq!(
            constrained
                .getResults(author)
                .await
                .unwrap()
                .unwrap()
                .getKey(),
            first.getKey()
        );
        assert_eq!(
            Author::queryOn(db)
                .hasCount(&latest(), "=", 1)
                .unwrap()
                .get()
                .await
                .unwrap()
                .modelKeys(),
            vec![1.into()]
        );
        let has_latest = Author::queryOn(db)
            .withExists("latest_comment", &latest())
            .unwrap()
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap();
        assert_eq!(
            has_latest[0].getAttribute("latest_comment_exists"),
            Some(&true.into())
        );
        assert_eq!(
            has_latest[1].getAttribute("latest_comment_exists"),
            Some(&false.into())
        );
        let shared_path =
            HasManyThrough::<Post, Post, Comment>::new("author_id", "post_id", "author_id", "id")
                .one()
                .ofMany(&[
                    ("body", OneOfManyAggregate::Max),
                    ("id", OneOfManyAggregate::Min),
                ]);
        let projected = Post::queryOn(db)
            .withMax("latest_comment as chosen_comment_id", &shared_path, "id")
            .unwrap()
            .findOrFail(second_post.getKey().unwrap().clone())
            .await
            .unwrap();
        assert_eq!(projected.getAttribute("chosen_comment_id"), first.getKey());

        let eligible = |query: openrust::database::query::Conditions| {
            query.r#where("body", "=", "hidden parent")
        };
        let excluding = latest().whereCandidates(eligible).unwrap();
        assert!(excluding.getResults(author).await.unwrap().is_none());
        let including = latest()
            .withTrashedParents()
            .whereCandidates(eligible)
            .unwrap();
        assert_eq!(
            including
                .getResults(author)
                .await
                .unwrap()
                .unwrap()
                .getAttribute("body"),
            Some(&"hidden parent".into())
        );
        let deleted = COMMENTS_THROUGH
            .one()
            .latestOfMany(None)
            .constrain(|query| query.withTrashed())
            .whereCandidates(|query| query.r#where("body", "=", "deleted"))
            .unwrap();
        assert_eq!(
            deleted
                .getResults(author)
                .await
                .unwrap()
                .unwrap()
                .getAttribute("body"),
            Some(&"deleted".into())
        );
    })
}

fn verify_collection_keys<'a>(
    db: &'a Connection,
    other: &'a Connection,
) -> openrust::database::DatabaseFuture<'a, ()> {
    Box::pin(async move {
        let ordered = || Author::queryOn(db).orderBy("id", "asc");
        let authors = ordered().get().await.unwrap();
        let labels = authors.map(|model, index| (index, model.getKey().cloned()));
        assert_eq!(labels.get(1), Some(&(1, Some(2.into()))));
        let indexed = authors
            .mapWithKeys(|model, index| [(format!("author-{index}"), model.getKey().cloned())]);
        assert_eq!(indexed.get("author-2"), Some(&Some(3.into())));
        let ids = authors.pluck(|model| model.getKey().cloned());
        assert_eq!(ids.get(0), Some(&Some(1.into())));
        let repeated = authors.pluckWithKeys(|_| "id", |model| model.getKey().cloned());
        assert_eq!(repeated.count(), 1);
        assert_eq!(repeated.get("id"), Some(&Some(3.into())));
        let retained = ordered().get().await.unwrap().mapModels(|model, _| model);
        assert_eq!(retained.modelKeys(), authors.modelKeys());
        assert_eq!(retained.toBase().get(1).unwrap().getKey(), Some(&2.into()));
        let empty = Collection::<Author>::new(vec![]);
        assert!(empty.map::<i32>(|_, _| unreachable!()).isEmpty());
        assert_eq!(authors.keys().get(2), Some(&2.into()));
        let keyed = ordered()
            .get()
            .await
            .unwrap()
            .mapModelsWithKeys(|model, index| {
                [(if index == 1.into() { "7" } else { "last" }, model)]
            });
        assert_eq!(keyed.modelKeys(), [3.into(), 2.into()]);
        assert_eq!(keyed.getByKey(7).unwrap().getKey(), Some(&2.into()));
        assert_eq!(keyed.first().unwrap().getKey(), Some(&3.into()));
        assert_eq!(keyed.keys().get(0), Some(&"last".into()));
        assert_eq!(
            keyed.map(|model, _| model.getKey().cloned()).get("last"),
            Some(&Some(3.into()))
        );
        assert_eq!(
            serde_json::from_str::<Json>(&keyed.toJson().unwrap()).unwrap()["7"]["id"],
            json!(2)
        );
        let fresh = keyed.fresh().await.unwrap();
        assert_eq!(fresh.getByKey("last").unwrap().getKey(), Some(&3.into()));
        let zipped = keyed.fresh().await.unwrap().zip(["a", "b", "c"]);
        assert_eq!(zipped.count(), 3);
        assert_eq!(
            zipped.get(0).unwrap().0.as_ref().unwrap().getKey(),
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
            .zipMany([&["a"][..], &["x", "y", "z"], &[]]);
        assert_eq!(columns.count(), 3);
        assert_eq!(columns.get(0).unwrap().1, [Some(&"a"), Some(&"x"), None]);
        assert_eq!(columns.get(1).unwrap().1, [None, Some(&"y"), None]);
        let (model, values) = columns.get(2).unwrap();
        assert!(model.is_none());
        assert_eq!(values, &[None, Some(&"z"), None]);
        let empty_columns = keyed.fresh().await.unwrap().zipMany::<i32, [i32; 0]>([]);
        assert_eq!(empty_columns.count(), 2);
        assert!(empty_columns.get(0).unwrap().1.is_empty());

        let flipped = keyed.flipBy(|author| (author.getKey() == Some(&2.into())).then_some("42"));
        assert_eq!(flipped.count(), 1);
        assert_eq!(flipped.get(42), Some(&7.into()));
        let collision = keyed.flipBy(|_| Some("shared"));
        assert_eq!(collision.count(), 1);
        assert_eq!(collision.get("shared"), Some(&7.into()));
        let padding = &keyed[0];
        let no_padding = keyed.fresh().await.unwrap().pad(2, padding).unwrap();
        assert!(no_padding.get(7).is_some());
        let padded = keyed.fresh().await.unwrap().pad(-4, padding).unwrap();
        assert_eq!(padded.count(), 4);
        assert_eq!(padded.get(2).unwrap().getKey(), Some(&2.into()));
        assert!(padded.get("last").is_some());
        let right = keyed.fresh().await.unwrap().pad(3, padding).unwrap();
        assert_eq!(right.get(1).unwrap().getKey(), Some(&3.into()));
        let flat = keyed.fresh().await.unwrap().flatten();
        assert_eq!(flat.get(1).unwrap().getKey(), Some(&2.into()));
        assert!(flat.get("last").is_none());
        assert_eq!(
            fresh
                .mapModels(|model, _| model)
                .toBase()
                .get(7)
                .unwrap()
                .getKey(),
            Some(&2.into())
        );
        assert!(empty.mapModels(|model, _| model).is_empty());
        assert!(std::ptr::eq(
            authors.findOrFail(&"2".into()).unwrap(),
            &authors[1]
        ));
        assert!(!authors.doesntContain(&2.into()).unwrap());
        assert!(authors.doesntContain(&20.into()).unwrap());
        for error in [
            authors.findOrFail(&20.into()).err(),
            authors.findManyOrFail(&[1.into(), 20.into()]).err(),
            authors.findManyOrFail(&[Value::Null]).err(),
            Collection::<Author>::new(vec![])
                .findOrFail(&1.into())
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
                .map(|author| author.getKey().unwrap().clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            found_keys(
                authors
                    .findMany(&[3.into(), 20.into(), "1".into()])
                    .unwrap()
            ),
            vec![1.into(), 3.into()]
        );
        assert_eq!(
            found_keys(
                authors
                    .findManyOrFail(&[3.into(), "1".into(), 1.into()])
                    .unwrap()
            ),
            vec![1.into(), 3.into()]
        );
        assert!(authors.findManyOrFail(&[]).unwrap().is_empty());
        let reloaded = ordered().findOrFail(2).await.unwrap();
        assert!(authors[1].is(&reloaded));
        assert!(authors.containsModel(&reloaded));
        let mut same_table = StrictAuthor::queryOn(db).make(Attributes::new()).unwrap();
        same_table.setAttribute("id", reloaded.getKey().unwrap().clone());
        assert!(authors.containsModel(&same_table));
        same_table.setAttribute("id", "2");
        assert!(authors.doesntContainModel(&same_table));
        assert!(std::ptr::eq(
            authors.findModelOrFail(&reloaded).unwrap(),
            &authors[1]
        ));
        let mut post = Post::queryOn(db).make(Attributes::new()).unwrap();
        post.setAttribute("id", reloaded.getKey().unwrap().clone());
        assert!(authors[1].isNot(&post));
        assert!(authors.doesntContainModel(&post));
        assert!(std::ptr::eq(
            authors.findModel(&post).unwrap().unwrap(),
            &authors[1]
        ));
        post.setAttribute("id", 20);
        assert!(matches!(
            authors.findModelOrFail(&post),
            Err(DatabaseError::ModelNotFound("relation_authors"))
        ));
        let mut unbound = Author::newInstance(Attributes::new()).unwrap();
        unbound.setAttribute("id", reloaded.getKey().unwrap().clone());
        assert_ne!(unbound.getConnectionName(), reloaded.getConnectionName());
        assert!(authors.doesntContainModel(&unbound));
        unbound.setAttribute("id", "2");
        assert!(authors.findModel(&unbound).unwrap().is_some());
        let mut visited = Vec::new();
        assert!(authors.containsWhere(|author, index| {
            visited.push(index);
            author.getAttribute("name") == Some(&"Second".into())
        }));
        assert_eq!(visited, [0, 1]);
        assert!(authors.doesntContainWhere(|_, index| index == 20));
        let replacement = || async {
            let mut updated = ordered().findOrFail(1).await.unwrap();
            updated.setAttribute("name", "Replacement");
            updated
        };
        let merged = ordered()
            .get()
            .await
            .unwrap()
            .merge([replacement().await])
            .unwrap();
        assert_eq!(merged.modelKeys(), vec![1.into(), 2.into(), 3.into()]);
        assert_eq!(merged[0].getAttribute("name"), Some(&"Replacement".into()));
        let mut duplicate = ordered()
            .get()
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();
        duplicate.push(replacement().await);
        let duplicate = Collection::new(duplicate);
        assert_eq!(
            found_keys(duplicate.findMany(&[1.into(), 1.into()]).unwrap()),
            vec![1.into(), 1.into()]
        );
        assert!(matches!(
            duplicate.findManyOrFail(&[1.into(), 20.into()]),
            Err(DatabaseError::ModelNotFound("relation_authors"))
        ));
        let mut dictionary_models: Vec<_> = duplicate
            .iter()
            .map(|model| Author::from_state(model.state().clone()))
            .collect();
        dictionary_models[3].setAttribute("id", "1");
        let mut binary = Author::newInstance(Attributes::new()).unwrap();
        binary.setAttribute("id", Value::Bytes(vec![0, 255]));
        dictionary_models.push(binary);
        dictionary_models.push(Author::newInstance(Attributes::new()).unwrap());
        let dictionary_models = Collection::new(dictionary_models);
        let dictionary = dictionary_models.getDictionary().unwrap();
        let keys: Vec<_> = dictionary.keys().map(Vec::as_slice).collect();
        assert_eq!(keys, [&b"1"[..], &b"2"[..], &b"3"[..], &[0, 255]]);
        assert!(std::ptr::eq(
            dictionary[b"1".as_slice()],
            &dictionary_models[3]
        ));
        assert_eq!(
            dictionary[b"1".as_slice()].getAttribute("name"),
            Some(&"Replacement".into())
        );
        assert!(
            Collection::<Author>::new(vec![])
                .getDictionary()
                .unwrap()
                .is_empty()
        );

        let projected = duplicate.duplicatesBy(|author| author.getKey().cloned());
        assert_eq!(projected.get(3), Some(&Some(1.into())));
        assert_eq!(projected.count(), 1);
        let fresh = duplicate.fresh().await.unwrap();
        let strict = fresh.duplicatesStrict();
        assert_eq!(strict.keys().get(0), Some(&3.into()));
        let unique_by = duplicate.fresh().await.unwrap().uniqueBy(|author, _| {
            author.getKey().and_then(|key| match key {
                Value::Integer(key) => Some(*key),
                _ => None,
            })
        });
        assert_eq!(unique_by.modelKeys(), [1.into(), 2.into(), 3.into()]);
        let counts = duplicate.countBy(|author, _| author.getKey() == Some(&1.into()));
        assert_eq!(counts.get(true), Some(&2));
        assert_eq!(counts.get(false), Some(&2));
        let (duplicates, rest) =
            duplicate.partition(|author, _| author.getKey() == Some(&1.into()));
        assert_eq!(duplicates.keys().get(1), Some(&3.into()));
        assert_eq!(rest.keys().get(0), Some(&1.into()));
        let distinct = duplicates.merge(rest).unwrap().unique().unwrap();
        assert_eq!(distinct.modelKeys(), vec![1.into(), 2.into(), 3.into()]);
        assert_eq!(
            distinct[0].getAttribute("name"),
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
                .modelKeys(),
            vec![1.into()]
        );
        assert_eq!(
            ordered()
                .get()
                .await
                .unwrap()
                .intersect(&keys)
                .unwrap()
                .modelKeys(),
            vec![2.into(), 3.into()]
        );
        assert_eq!(
            keys.toQuery()
                .unwrap()
                .orderBy("id", "asc")
                .get()
                .await
                .unwrap()
                .modelKeys(),
            vec![2.into(), 3.into()]
        );
        assert!(Collection::<Author>::new(Vec::new()).toQuery().is_err());
        let mut outside = Author::queryOn(other).make(Attributes::new()).unwrap();
        outside.setAttribute("id", 2);
        assert!(
            Collection::new(vec![ordered().findOrFail(1).await.unwrap(), outside])
                .toQuery()
                .is_err()
        );
    })
}

fn verify_collection_reload(
    db: &Connection,
    mysql: bool,
) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let ordered = || Author::queryOn(db).orderBy("id", "desc");
        let mut missing = Author::queryOn(db)
            .create(Attributes::from([("name".into(), "Temporary".into())]))
            .await
            .unwrap();
        let missing_key = missing.getKey().unwrap().clone();
        let mut models = ordered()
            .get()
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();
        let mut dirty = ordered().findOrFail(1).await.unwrap();
        dirty.setAttribute("name", "Edited locally");
        models.push(dirty);
        missing.delete().await.unwrap();

        let mut stale = Collection::new(models);
        assert!(matches!(
            stale.loadCount("posts", &POSTS).await,
            Err(openrust::database::DatabaseError::ModelNotFound(
                "relation_authors"
            ))
        ));
        assert!(
            stale
                .iter()
                .all(|model| model.getAttribute("posts_count").is_none())
        );
        let baseline = if mysql { select_count(db).await } else { 0 };
        let mut refreshed = stale.freshWith("posts", &POSTS).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 2);
        }
        assert_eq!(stale.modelKeys()[0], missing_key);
        assert_eq!(
            stale.last().unwrap().getAttribute("name"),
            Some(&"Edited locally".into())
        );
        assert_eq!(
            refreshed.modelKeys(),
            vec![3.into(), 2.into(), 1.into(), 1.into()]
        );
        assert_eq!(
            refreshed[2].getAttribute("name"),
            refreshed[3].getAttribute("name")
        );
        assert!(refreshed.iter().all(|model| model.relationLoaded("posts")));

        let baseline = if mysql { select_count(db).await } else { 0 };
        refreshed.loadCount("posts", &POSTS).await.unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        let baseline = if mysql { select_count(db).await } else { 0 };
        refreshed
            .loadAggregates(&[
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
                .all(|model| model.getAttribute("mentees_count").is_some())
        );
        assert!(
            refreshed
                .loadAggregates(&[
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
                .all(|model| model.getAttribute("collision").is_none())
        );
        refreshed.loadAvg("posts", &POSTS, "score").await.unwrap();
        refreshed.loadMin("posts", &POSTS, "score").await.unwrap();
        refreshed
            .loadMax("posts as top_score", &POSTS, "score")
            .await
            .unwrap();
        assert_eq!(refreshed[0].getAttribute("posts_count"), Some(&0.into()));
        assert_eq!(
            refreshed[0].getAttribute("posts_exists"),
            Some(&false.into())
        );
        for attribute in [
            "posts_sum_score",
            "posts_avg_score",
            "posts_min_score",
            "top_score",
        ] {
            assert_eq!(refreshed[0].getAttribute(attribute), Some(&Value::Null));
        }
        assert_eq!(
            refreshed[2].getAttribute("posts_exists"),
            Some(&true.into())
        );
        assert_eq!(
            refreshed[2].state().getAttributes(),
            refreshed[3].state().getAttributes()
        );
        assert!(refreshed.iter().all(|model| !model.state().isDirty()));
        assert!(refreshed.loadCount("posts as id", &POSTS).await.is_err());

        let mut modified = ordered().findOrFail(1).await.unwrap();
        modified.setAttribute("name", "Pending");
        let mut modified = Collection::new(vec![modified]);
        modified.loadCount("posts", &POSTS).await.unwrap();
        assert_eq!(
            modified[0].state().getDirty(),
            Attributes::from([("name".into(), "Pending".into())])
        );

        let mut single = ordered().findOrFail(1).await.unwrap();
        single.setAttribute("name", "Pending model edit");
        let baseline = if mysql { select_count(db).await } else { 0 };
        single
            .loadAggregates(&[
                RelationAggregateQuery::new("posts", &POSTS, RelationAggregate::Count),
                RelationAggregateQuery::new("posts", &POSTS, RelationAggregate::Exists),
            ])
            .await
            .unwrap();
        if mysql {
            assert_eq!(select_count(db).await - baseline, 1);
        }
        assert_eq!(single.getAttribute("posts_exists"), Some(&true.into()));
        assert_eq!(
            single.getRawOriginal("posts_count"),
            refreshed[2].getAttribute("posts_count")
        );
        assert_eq!(
            single.state().getDirty(),
            Attributes::from([("name".into(), "Pending model edit".into())])
        );
        single.loadCount("posts", &POSTS).await.unwrap();
        single.loadExists("posts", &POSTS).await.unwrap();
        single.loadSum("posts", &POSTS, "score").await.unwrap();
        single.loadAvg("posts", &POSTS, "score").await.unwrap();
        single.loadMin("posts", &POSTS, "score").await.unwrap();
        single
            .loadMax("posts as top_score", &POSTS, "score")
            .await
            .unwrap();
        assert_eq!(
            single.getAttribute("posts_sum_score"),
            refreshed[2].getAttribute("posts_sum_score")
        );
        assert_eq!(
            single.getAttribute("posts_avg_score"),
            refreshed[2].getAttribute("posts_avg_score")
        );
        assert_eq!(
            single.getAttribute("posts_min_score"),
            refreshed[2].getAttribute("posts_min_score")
        );
        assert_eq!(
            single.getAttribute("top_score"),
            refreshed[2].getAttribute("top_score")
        );
        let mut stale_model = Author::from_state(stale[0].state().clone());
        assert!(matches!(
            stale_model.loadCount("posts", &POSTS).await,
            Err(DatabaseError::ModelNotFound("relation_authors"))
        ));
        assert!(stale_model.getAttribute("posts_count").is_none());

        let mut empty = Collection::<Author>::new(Vec::new());
        assert!(empty.fresh().await.unwrap().is_empty());
        empty.loadCount("posts", &POSTS).await.unwrap();
        let mut unsaved = ordered().make(Attributes::new()).unwrap();
        unsaved.setAttribute("id", 1);
        assert!(
            Collection::new(vec![unsaved])
                .fresh()
                .await
                .unwrap()
                .is_empty()
        );

        let mut trashed = Post::queryOn(db).onlyTrashed().get().await.unwrap();
        assert!(!trashed.is_empty());
        assert_eq!(
            trashed.fresh().await.unwrap().modelKeys(),
            trashed.modelKeys()
        );
        trashed.loadCount("comments", &POST_COMMENTS).await.unwrap();
        assert!(
            trashed
                .iter()
                .all(|post| post.getAttribute("comments_count").is_some())
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
            .map(|post| post.getKey().unwrap().clone())
            .collect::<Vec<_>>()
    };
    let selected = FEATURED_POSTS.withPivotValue("label", "selected").unwrap();
    assert!(FEATURED_POSTS.withPivotValue("label", Value::Null).is_err());
    assert!(FEATURED_POSTS.withPivotValue("AUTHOR_ID", 5).is_err());
    selected
        .attach(first, std::slice::from_ref(first_key))
        .await
        .unwrap();
    let changed = selected
        .syncWithoutDetaching(first, &[first_key.clone(), second_key.clone()])
        .await
        .unwrap();
    assert_eq!(changed.attached, vec![second_key.clone()]);
    assert_eq!(selected.getResults(first).await.unwrap().len(), 2);
    let updated = selected
        .updateExistingPivot(
            first,
            second_key,
            Attributes::from([("label".into(), "hidden".into())]),
        )
        .await
        .unwrap();
    assert_eq!(updated, 1);
    assert_eq!(
        ids(selected.getResults(first).await.unwrap()),
        vec![first_key.clone()]
    );
    assert_eq!(
        selected
            .updateExistingPivot(
                first,
                second_key,
                Attributes::from([("label".into(), "ignored".into())]),
            )
            .await
            .unwrap(),
        0
    );
    let alternatives = FEATURED_POSTS
        .wherePivotIn("label", vec!["selected".into()])
        .orWherePivotNull("label");
    assert_eq!(
        ids(alternatives.getResults(first).await.unwrap()),
        vec![first_key.clone()]
    );
    assert_eq!(
        Author::queryOn(db)
            .has(&alternatives)
            .unwrap()
            .get()
            .await
            .unwrap()
            .len(),
        2
    );
    let hidden = FEATURED_POSTS
        .wherePivotNotIn("label", vec!["selected".into()])
        .wherePivotBetween("label", ["a".into(), "m".into()])
        .wherePivotNotBetween("label", ["a".into(), "f".into()])
        .wherePivotNotNull("label");
    let owners = Author::queryOn(db)
        .with("filtered", hidden)
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(
        ids(owners[0]
            .getRelation::<RelatedCollection<Post>>("filtered")
            .unwrap()
            .clone()),
        vec![second_key.clone()]
    );
    assert!(
        owners[1]
            .getRelation::<RelatedCollection<Post>>("filtered")
            .unwrap()
            .is_empty()
    );
    let changed = selected.sync(first, &[]).await.unwrap();
    assert_eq!(changed.detached, vec![first_key.clone()]);
    assert_eq!(selected.detach(first, None).await.unwrap(), 0);
    selected
        .attachWith(
            first,
            std::slice::from_ref(first_key),
            Attributes::from([("label".into(), "explicit".into())]),
        )
        .await
        .unwrap();
    assert!(selected.getResults(first).await.unwrap().is_empty());
    let constrained = FEATURED_POSTS.wherePivot("label", "=", "selected");
    assert_eq!(constrained.detach(first, None).await.unwrap(), 0);
    assert_eq!(FEATURED_POSTS.detach(first, None).await.unwrap(), 2);
    constrained
        .attach(first, std::slice::from_ref(first_key))
        .await
        .unwrap();
    assert!(constrained.getResults(first).await.unwrap().is_empty());
    assert_eq!(FEATURED_POSTS.detach(first, None).await.unwrap(), 1);
}

async fn verify_pivot_timestamps(db: &Connection, owner: &Author, first: &Value, second: &Value) {
    const EARLIER: &str = "2000-01-01 00:00:00";
    const LATER: &str = "2001-01-01 00:00:00";
    let relation = FEATURED_POSTS
        .withTimestamps()
        .orderByPivotDesc("created_at");
    for (key, value) in [(first, EARLIER), (second, LATER)] {
        relation
            .attachWith(
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
    let posts = relation.getResults(owner).await.unwrap();
    assert_eq!(posts[0].getKey(), Some(second));
    let eager = Author::queryOn(db)
        .with(
            "ordered",
            FEATURED_POSTS
                .withTimestamps()
                .orderByPivot("relation_featured_posts.created_at", "asc"),
        )
        .findOrFail(owner.getKey().unwrap().clone())
        .await
        .unwrap();
    let posts = eager
        .getRelation::<RelatedCollection<Post>>("ordered")
        .unwrap();
    assert_eq!(posts[0].getKey(), Some(first));
    assert_eq!(
        relation
            .updateExistingPivot(owner, first, Attributes::new())
            .await
            .unwrap(),
        1
    );
    let report = relation
        .syncWithPivotAttributes(
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
    let posts = relation.getResults(owner).await.unwrap();
    for (post, expected) in posts.iter().zip([LATER, EARLIER]) {
        let pivot = post.getRelation::<Pivot>("pivot").unwrap();
        assert_eq!(pivot.getAttribute("created_at"), Some(&expected.into()));
        assert!(
            matches!(pivot.getAttribute("updated_at"), Some(Value::Text(value)) if value.as_str() > LATER)
        );
    }
    assert_eq!(relation.detach(owner, None).await.unwrap(), 2);
    relation
        .attach(owner, std::slice::from_ref(first))
        .await
        .unwrap();
    let posts = relation.getResults(owner).await.unwrap();
    let pivot = posts[0].getRelation::<Pivot>("pivot").unwrap();
    let created = pivot.getAttribute("created_at");
    assert!(matches!(created, Some(Value::Text(value)) if value.as_str() > LATER));
    assert_eq!(created, pivot.getAttribute("updated_at"));
    assert_eq!(relation.detach(owner, None).await.unwrap(), 1);

    let custom = FEATURED_POSTS
        .withTimestampColumns(None, Some("linked_at"))
        .unwrap();
    assert!(
        FEATURED_POSTS
            .withTimestampColumns(Some("author_id"), None)
            .is_err()
    );
    let report = custom
        .syncWithoutDetaching(owner, std::slice::from_ref(first))
        .await
        .unwrap();
    assert_eq!(report.attached, vec![first.clone()]);
    let posts = custom.getResults(owner).await.unwrap();
    let pivot = posts[0].getRelation::<Pivot>("pivot").unwrap();
    assert!(
        matches!(pivot.getAttribute("linked_at"), Some(Value::Text(value)) if value.as_str() > LATER)
    );
    assert_eq!(pivot.getAttribute("created_at"), None);
    assert_eq!(custom.detach(owner, None).await.unwrap(), 1);
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
    fn pivotInverseRelations() -> &'static [&'static str] {
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

    fn handleInTransaction<'a>(
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
            let label = model.getAttributeValue("label")?;
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
    let mut parent = TouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
    parent.setAttribute("name", "unsaved");
    let relation = BelongsToMany::<TouchAuthor, Post>::new(
        "relation_featured_posts",
        "author_id",
        "post_id",
        "id",
        "id",
    )
    .withTouchingParent();
    assert!(
        relation
            .toggle(&parent, &[])
            .await
            .unwrap()
            .attached
            .is_empty()
    );
    let old = TouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
    assert_eq!(old.getAttribute("updated_at"), Some(&OLD.into()));
    assert!(
        relation
            .toggleOrFail(&parent, &[2.into(), Value::Null])
            .await
            .is_err()
    );
    let result = relation
        .toggleWithPivotAttributes(
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
        .withPivot(["label"])
        .getResults(&parent)
        .await
        .unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].getKey(), Some(&1.into()));
    assert_eq!(
        posts[0]
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
        Some(&"toggle".into())
    );
    let saved = TouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
    assert_eq!(saved.getAttribute("name"), Some(&"first".into()));
    assert!(
        matches!(saved.getAttribute("updated_at"), Some(Value::Text(time)) if time.as_str() > OLD)
    );
    let relation = BelongsToMany::<TouchAuthor, Post>::new(
        "relation_featured_posts",
        "author_id",
        "post_id",
        "id",
        "id",
    )
    .withTouchingParent();
    for action in ["attach", "update", "detach", "sync"] {
        db.table("relation_authors")
            .r#where("id", "=", 1)
            .update(Attributes::from([("updated_at".into(), OLD.into())]))
            .await
            .unwrap();
        let parent = TouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
        match action {
            "attach" => relation.attach(&parent, &[2.into()]).await.unwrap(),
            "update" => assert_eq!(
                relation
                    .updateExistingPivot(
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
        let fresh = TouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
        assert!(
            matches!(fresh.getAttribute("updated_at"), Some(Value::Text(time)) if time.as_str() > OLD),
            "{action}"
        );
    }
    let other = TouchAuthor::queryOn(db).findOrFail(2).await.unwrap();
    assert_eq!(other.getAttribute("updated_at"), Some(&OLD.into()));
}

struct SaveTouchAuthor(ModelState);

impl Model for SaveTouchAuthor {
    fn metadata() -> &'static ModelMetadata {
        TouchAuthor::metadata()
    }

    fn touches() -> Touches<Self> {
        Touches::default()
            .belongsToMany(
                BelongsToMany::<Self, TouchPost>::new(
                    "relation_featured_posts",
                    "author_id",
                    "post_id",
                    "id",
                    "id",
                )
                .wherePivot("label", "=", "eligible"),
            )
            .morphToMany(MorphToMany::<Self, TouchPost>::new(
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
            .softDeletes("deleted_at");
        &METADATA
    }

    fn touches() -> Touches<Self> {
        Touches::default().belongsTo(BelongsTo::<Self, SaveTouchAuthor>::new("author_id", "id"))
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
    let parent = TouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
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

    let touching = relation().withTouchingRelated();
    for action in ["attach", "update", "detach", "sync", "toggle"] {
        db.statement("UPDATE relation_posts SET updated_at = ?", &[OLD.into()])
            .await
            .unwrap();
        match action {
            "attach" => touching.attach(&parent, &[2.into()]).await.unwrap(),
            "update" => {
                touching
                    .updateExistingPivot(
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
        .wherePivot("label", "=", "eligible")
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
    .withTouchingRelated();
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
    let mut parent = SaveTouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
    parent.save().await.unwrap();
    assert_eq!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_eq!(db.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());

    let mut tx = db.beginTransaction().await.unwrap();
    parent.setAttribute("name", "touch rolled back");
    parent.saveInTransaction(&mut tx).await.unwrap();
    assert_ne!(tx.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_ne!(tx.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());
    assert_eq!(tx.scalar(timestamp, &[3.into()]).await.unwrap(), OLD.into());
    assert_ne!(tx.scalar(author_timestamp, &[]).await.unwrap(), OLD.into());
    tx.rollBack(None).await.unwrap();
    for post in [1, 2, 3] {
        assert_eq!(
            db.scalar(timestamp, &[post.into()]).await.unwrap(),
            OLD.into()
        );
    }
    assert_eq!(db.scalar(author_timestamp, &[]).await.unwrap(), OLD.into());

    let mut parent = SaveTouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
    parent.setAttribute("name", "touch committed");
    parent.save().await.unwrap();
    assert_ne!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_ne!(db.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());
    assert_ne!(db.scalar(author_timestamp, &[]).await.unwrap(), OLD.into());
}

async fn verify_inferred_pivot_touch(db: &Connection) {
    const OLD: &str = "2000-01-01 00:00:00";
    let parent = SaveTouchAuthor::queryOn(db).findOrFail(1).await.unwrap();
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
        .wherePivot("label", "=", "unmatched")
        .attachWith(
            &parent,
            &[4.into()],
            Attributes::from([("label".into(), "unmatched".into())]),
        )
        .await
        .unwrap();
    assert_eq!(db.scalar(timestamp, &[4.into()]).await.unwrap(), OLD.into());
    relation().detach(&parent, Some(&[4.into()])).await.unwrap();

    let filtered = relation().wherePivot("label", "=", "eligible");
    assert!(
        filtered
            .attachWith(
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
        .attachWith(
            &parent,
            &[1.into()],
            Attributes::from([("label".into(), "eligible".into())]),
        )
        .await
        .unwrap();
    assert_ne!(db.scalar(timestamp, &[1.into()]).await.unwrap(), OLD.into());
    assert_eq!(db.scalar(timestamp, &[2.into()]).await.unwrap(), OLD.into());

    db.statement(reset, &[OLD.into()]).await.unwrap();
    let post = TouchPost::queryOn(db).findOrFail(1).await.unwrap();
    let inverse = BelongsToMany::<TouchPost, SaveTouchAuthor>::new(
        "relation_featured_posts",
        "post_id",
        "author_id",
        "id",
        "id",
    )
    .wherePivot("label", "=", "eligible");
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
    assert_eq!(fresh.getTable(), "relation_featured_posts");
    assert_eq!(fresh.getAttributeValue("label").unwrap(), Some(20.into()));
    refreshed.refresh().await.unwrap();
    assert_eq!(
        refreshed.getAttributeValue("label").unwrap(),
        Some(20.into())
    );
    assert!(!refreshed.state().isDirty());
}

async fn verify_first_and_update_or_create(db: &Connection) {
    let parent = Author::queryOn(db).findOrFail(1).await.unwrap();
    let relation = FEATURED_POSTS.withPivot(["label"]);
    let title = |name: &str| Attributes::from([("title".into(), name.into())]);
    let score = |value: i64| Attributes::from([("score".into(), value.into())]);
    let label = |name: &str| Attributes::from([("label".into(), name.into())]);

    let scoped = FEATURED_POSTS.constrain(move |query| query.withAttributes(score(83), true));
    let scoped_post = scoped
        .create(&parent, title("scoped-pivot"), label("scoped"))
        .await
        .unwrap();
    assert_eq!(scoped_post.getAttribute("score"), Some(&83.into()));
    let scoped_force = FEATURED_POSTS
        .constrain(move |query| query.withAttributes(score(84), false))
        .forceCreate(&parent, title("scoped-pivot-force"), label("scoped"))
        .await
        .unwrap();
    assert_eq!(scoped_force.getAttribute("score"), Some(&84.into()));
    assert_eq!(scoped.getResults(&parent).await.unwrap().len(), 1);
    scoped
        .detach(
            &parent,
            Some(&[
                scoped_post.getKey().unwrap().clone(),
                scoped_force.getKey().unwrap().clone(),
            ]),
        )
        .await
        .unwrap();

    let existing = relation
        .firstOrCreate(&parent, title("unlinked"), score(70), label("attached"))
        .await
        .unwrap();
    assert_eq!(existing.getKey(), Some(&100.into()));
    assert_eq!(existing.getAttribute("score"), Some(&90.into()));
    assert!(!existing.state().wasRecentlyCreated());

    let first = relation
        .firstOrCreate(&parent, title("unlinked"), score(71), label("ignored"))
        .await
        .unwrap();
    assert_eq!(first.getAttribute("score"), Some(&90.into()));
    assert_eq!(
        first
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
        Some(&"attached".into())
    );

    let updated = relation
        .updateOrCreate(&parent, title("unlinked"), score(72), label("ignored"))
        .await
        .unwrap();
    assert_eq!(updated.getKey(), first.getKey());
    assert_eq!(updated.getAttribute("score"), Some(&72.into()));
    assert_eq!(
        updated
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
        Some(&"attached".into())
    );

    let created = relation
        .updateOrCreate(
            &parent,
            title("upsert-created"),
            score(73),
            label("created"),
        )
        .await
        .unwrap();
    assert!(created.state().wasRecentlyCreated());
    assert_eq!(created.getAttribute("score"), Some(&73.into()));
    assert_eq!(
        Post::queryOn(db)
            .findOrFail(100)
            .await
            .unwrap()
            .getAttribute("score"),
        Some(&72.into())
    );

    let rejected = relation
        .updateOrCreate(&parent, title("unlinked"), score(73), label("ignored"))
        .await;
    assert!(matches!(rejected, Err(DatabaseError::Driver { .. })));
    assert_eq!(
        Post::queryOn(db)
            .findOrFail(100)
            .await
            .unwrap()
            .getAttribute("score"),
        Some(&72.into())
    );
}

async fn verify_restore_and_deferred_values(db: &Connection) {
    let parent = Author::queryOn(db).findOrFail(1).await.unwrap();
    let relation = FEATURED_POSTS.withPivot(["label"]);
    let title = |name: &str| Attributes::from([("title".into(), name.into())]);
    let label = |name: &str| Attributes::from([("label".into(), name.into())]);
    let never = || RelatedValues::deferred(|| panic!("values must remain deferred"));

    db.statement("INSERT INTO relation_posts (title, score, deleted_at) VALUES ('trash-attached', 200, '2025-01-01 00:00:00'), ('trash-unlinked', 201, '2025-01-01 00:00:00')", &[])
        .await.unwrap();
    let mut deleted = Post::queryOn(db).onlyTrashed();
    deleted.r#where("title", "=", "trash-attached");
    let key = deleted
        .firstOrFail()
        .await
        .unwrap()
        .getKey()
        .unwrap()
        .clone();
    relation
        .attachWith(&parent, std::slice::from_ref(&key), label("original"))
        .await
        .unwrap();

    let restored = relation
        .restoreOrCreate(&parent, title("trash-attached"), never(), label("ignored"))
        .await
        .unwrap();
    assert_eq!(restored.getKey(), Some(&key));
    assert_eq!(
        restored
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
        Some(&"original".into())
    );
    assert!(Post::queryOn(db).find(key).await.unwrap().is_some());

    let mut calls = 0;
    let restored = relation
        .createOrRestore(
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
    assert!(!restored.state().wasRecentlyCreated());
    assert!(matches!(
        restored.getAttribute("deleted_at"),
        Some(Value::Null)
    ));

    let linked = relation
        .firstOrCreate(&parent, title("trash-unlinked"), never(), label("ignored"))
        .await
        .unwrap();
    assert_eq!(linked.getKey(), restored.getKey());
    assert_eq!(
        linked
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
        Some(&"new-link".into())
    );

    let mut calls = 0;
    let created = relation
        .restoreOrCreate(
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
    assert!(created.state().wasRecentlyCreated());
    let mut calls = 0;
    let updated = relation
        .updateOrCreate(
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
    assert_eq!(updated.getKey(), created.getKey());
    assert_eq!(updated.getAttribute("score"), Some(&203.into()));

    let unsupported = BelongsToMany::<Author, Author>::new(
        "relation_featured_posts",
        "author_id",
        "post_id",
        "id",
        "id",
    );
    assert!(matches!(
        unsupported
            .restoreOrCreate(&parent, Attributes::new(), never(), Attributes::new())
            .await,
        Err(DatabaseError::InvalidQuery(_))
    ));
}

async fn verify_increment_or_create(db: &Connection, concurrent: bool) {
    let parent = Author::queryOn(db).findOrFail(1).await.unwrap();
    let relation = FEATURED_POSTS
        .withPivot(["label"])
        .withPivotValue("label", "counter")
        .unwrap();
    let attributes = || Attributes::from([("title".into(), "counter-start".into())]);
    let increment =
        |extra| relation.incrementOrCreate(&parent, attributes(), "score", 300_i64, 2_i64, extra);
    let created = increment(Attributes::new()).await.unwrap();
    assert!(created.state().wasRecentlyCreated());
    assert_eq!(created.getAttribute("score"), Some(&300.into()));

    let updated = increment(Attributes::new()).await.unwrap();
    assert!(!updated.state().wasRecentlyCreated());
    assert_eq!(updated.getKey(), created.getKey());
    assert_eq!(updated.getAttribute("score"), Some(&302.into()));
    assert_eq!(
        updated
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
        Some(&"counter".into())
    );

    let extra = Attributes::from([("title".into(), "counter-renamed".into())]);
    let updated = increment(extra).await.unwrap();
    assert_eq!(
        updated.getAttribute("title"),
        Some(&"counter-renamed".into())
    );
    assert_eq!(updated.getAttribute("score"), Some(&304.into()));
    if concurrent {
        let attributes = || Attributes::from([("title".into(), "counter-renamed".into())]);
        let increment = || {
            relation.incrementOrCreate(
                &parent,
                attributes(),
                "score",
                300_i64,
                2_i64,
                Attributes::new(),
            )
        };
        let (a, b) = tokio::join!(increment(), increment());
        let mut scores = [a.unwrap(), b.unwrap()].map(|model| match model.getAttribute("score") {
            Some(Value::Integer(score)) => *score,
            other => panic!("expected integer counter, got {other:?}"),
        });
        scores.sort_unstable();
        assert_eq!(scores, [306, 308]);
    }
    let persisted = Post::queryOn(db)
        .findOrFail(created.getKey().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(
        persisted.getAttribute("score"),
        Some(&if concurrent { 308.into() } else { 304.into() })
    );
    assert!(matches!(
        relation
            .incrementOrCreate(
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
            .softDeletes("deleted_at");
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

    fn castUsing(scale: &i64) -> DatabaseResult<Self> {
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
        relation.incrementOrCreate(
            post,
            Attributes::from([("name".into(), "scaled".into())]),
            "count",
            3_i64,
            step,
            Attributes::new(),
        )
    };
    let created = increment(2).await.unwrap();
    assert_eq!(created.getAttribute("count"), Some(&300.into()));
    let updated = increment(2).await.unwrap();
    assert_eq!(updated.getAttribute("count"), Some(&500.into()));
    assert_eq!(updated.getAttributeValue("count").unwrap(), Some(5.into()));
    assert!(matches!(
        increment(9).await,
        Err(DatabaseError::InvalidQuery("rejected logical increment"))
    ));
    let current = ScaledCounter::queryOn(db)
        .findOrFail(created.getKey().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(current.getAttribute("count"), Some(&500.into()));
    let mut current = current;
    assert_eq!(
        current
            .decrement("count", 1_i64, Attributes::new())
            .await
            .unwrap(),
        1
    );
    assert_eq!(current.getAttribute("count"), Some(&400.into()));
    assert_eq!(current.getAttributeValue("count").unwrap(), Some(4.into()));
    current
        .increment("count", 1_i64, Attributes::new())
        .await
        .unwrap();

    if mysql {
        let (a, b) = tokio::join!(increment(2), increment(2));
        let values =
            [a.unwrap(), b.unwrap()].map(|model| model.getAttributeValue("count").unwrap());
        assert!(values.contains(&Some(7.into())) && values.contains(&Some(9.into())));
    }
}

async fn verify_non_pivot_lookup(author: &Author, post: &Post) {
    let attrs = |name: &str| Attributes::from([("name".into(), name.into())]);
    let count = |value: i64| Attributes::from([("count".into(), value.into())]);
    let direct = || HasMany::<Post, RelationCounter>::new("id", "post_id");
    let pending = direct()
        .firstOrNew(post, attrs("lookup"), count(1))
        .await
        .unwrap();
    assert!(!pending.state().exists());
    assert_eq!(pending.getAttribute("post_id"), post.getKey());
    let first = direct()
        .firstOrCreate(post, attrs("lookup"), count(2))
        .await
        .unwrap();
    let matched = direct()
        .one()
        .firstOrCreate(
            post,
            attrs("lookup"),
            RelatedValues::deferred(|| panic!("lookup hit")),
        )
        .await
        .unwrap();
    assert_eq!(first.getKey(), matched.getKey());
    let updated = direct()
        .updateOrCreate(post, attrs("lookup"), RelatedValues::deferred(|| count(3)))
        .await
        .unwrap();
    assert_eq!(updated.getAttribute("count"), Some(&3.into()));
    let mut conflicting = count(8);
    conflicting.insert("owner_type".into(), "post".into());
    let existing = direct()
        .createOrFirst(post, attrs("lookup"), conflicting)
        .await
        .unwrap();
    assert_eq!(existing.getKey(), first.getKey());
    assert_eq!(existing.getAttribute("count"), Some(&3.into()));

    let morph =
        || MorphMany::<Post, RelationCounter>::new("id", "post_id", "owner_type", "lookup-morph");
    let mut created = morph()
        .one()
        .firstOrCreate(post, attrs("lookup"), count(4))
        .await
        .unwrap();
    assert_ne!(created.getKey(), first.getKey());
    assert_eq!(
        created.getAttribute("owner_type"),
        Some(&"lookup-morph".into())
    );
    created.delete().await.unwrap();
    let restored = morph()
        .restoreOrCreate(
            post,
            attrs("lookup"),
            RelatedValues::deferred(|| panic!("trashed hit")),
        )
        .await
        .unwrap();
    assert_eq!(restored.getKey(), created.getKey());
    assert_eq!(restored.getAttribute("deleted_at"), Some(&Value::Null));

    let through = || {
        HasManyThrough::<Author, Post, RelationCounter>::new("author_id", "post_id", "id", "id")
            .constrain(|mut query| {
                query.r#where("owner_type", "=", "post");
                query
            })
    };
    let pending = through()
        .firstOrNew(author, attrs("through-lookup"), count(5))
        .await
        .unwrap();
    assert!(!pending.state().exists());
    assert!(pending.getAttribute("post_id").is_none());
    let mut create = count(6);
    create.insert("post_id".into(), post.getKey().unwrap().clone());
    create.insert("owner_type".into(), "post".into());
    let mut model = through()
        .firstOrCreate(author, attrs("through-lookup"), create.clone())
        .await
        .unwrap();
    let matched = through()
        .one()
        .createOrFirst(author, attrs("through-lookup"), create.clone())
        .await
        .unwrap();
    assert_eq!(matched.getKey(), model.getKey());
    model.delete().await.unwrap();
    let restored = through()
        .one()
        .createOrRestore(author, attrs("through-lookup"), create)
        .await
        .unwrap();
    assert_eq!(restored.getKey(), model.getKey());
    let updated = through()
        .updateOrCreate(author, attrs("through-lookup"), count(7))
        .await
        .unwrap();
    assert_eq!(updated.getAttribute("count"), Some(&7.into()));
}

async fn verify_scoped_attributes(db: &Connection, author: &Author, post: &Post) {
    let name = |name: &str| Attributes::from([("name".into(), name.into())]);
    let defaults = |count: i64| Attributes::from([("count".into(), count.into())]);
    let direct = HasMany::<Post, RelationCounter>::new("id", "post_id")
        .constrain(move |query| query.withAttributes(defaults(77), true));
    let created = direct.create(post, name("scoped-create")).await.unwrap();
    assert_eq!(created.getAttribute("count"), Some(&77.into()));
    let mut attributes = name("scoped-override");
    attributes.insert("count".into(), 78.into());
    let overridden = direct.create(post, attributes).await.unwrap();
    assert_eq!(overridden.getAttribute("count"), Some(&78.into()));

    let mut existing = RelationCounter::newInstanceOn(db, name("scoped-save")).unwrap();
    assert!(direct.save(post, &mut existing).await.unwrap());
    assert_eq!(existing.getAttribute("count"), Some(&77.into()));
    let made = direct
        .firstOrNew(post, name("scoped-new"), Attributes::new())
        .await
        .unwrap();
    assert!(!made.state().exists());
    assert_eq!(made.getAttribute("count"), Some(&77.into()));
    let scoped = direct.getResults(post).await.unwrap();
    assert!(
        scoped
            .iter()
            .any(|model| model.getKey() == created.getKey())
    );
    assert!(
        !scoped
            .iter()
            .any(|model| model.getKey() == overridden.getKey())
    );

    let morph = MorphMany::<Post, RelationCounter>::new("id", "post_id", "owner_type", "scoped")
        .constrain(move |query| {
            query.withAttributes(
                Attributes::from([
                    ("count".into(), 79.into()),
                    ("owner_type".into(), "ignored".into()),
                    ("post_id".into(), 999.into()),
                ]),
                false,
            )
        });
    let forced = morph.forceCreate(post, name("scoped-force")).await.unwrap();
    assert_eq!(forced.getAttribute("count"), Some(&79.into()));
    assert_eq!(forced.getAttribute("owner_type"), Some(&"scoped".into()));
    assert_eq!(forced.getAttribute("post_id"), post.getKey());

    let through =
        HasManyThrough::<Author, Post, RelationCounter>::new("author_id", "post_id", "id", "id")
            .constrain(move |query| query.withAttributes(defaults(81), false));
    let mut input = name("scoped-through");
    input.insert("post_id".into(), post.getKey().unwrap().clone());
    let related = through
        .firstOrCreate(author, input, Attributes::new())
        .await
        .unwrap();
    assert_eq!(related.getAttribute("count"), Some(&81.into()));

    let query = RelationCounter::queryOn(db).withAttributes(defaults(82), false);
    let instance = query.make(name("scoped-builder")).unwrap();
    assert_eq!(instance.getAttribute("count"), Some(&82.into()));
    assert!(query.get().await.unwrap().len() > scoped.len());
}

async fn verify_non_pivot_counters(db: &Connection, mysql: bool) {
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY AUTOINCREMENT"
    };
    db.statement(&format!("CREATE TABLE relation_counters (id {id}, name VARCHAR(80), count BIGINT, visits BIGINT DEFAULT 10, post_id BIGINT, owner_type VARCHAR(80) DEFAULT 'post', deleted_at DATETIME, UNIQUE (name, post_id, owner_type))"), &[]).await.unwrap();
    let author = Author::queryOn(db).findOrFail(1).await.unwrap();
    let post = Post::queryOn(db).findOrFail(2).await.unwrap();
    let name = |name: &str| Attributes::from([("name".into(), name.into())]);
    let assert_count = |model: &RelationCounter, count: i64| {
        assert_eq!(model.getAttribute("count"), Some(&count.into()))
    };

    let direct = HasMany::<Post, RelationCounter>::new("id", "post_id");
    let model = direct
        .incrementOrCreate(
            &post,
            name("direct"),
            "count",
            10_i64,
            2_i64,
            Attributes::from([("post_id".into(), 999.into())]),
        )
        .await
        .unwrap();
    assert_eq!(model.getAttribute("post_id"), post.getKey());
    assert_count(&model, 10);
    let model = direct
        .one()
        .incrementOrCreate(
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
        .incrementOrCreate(
            &post,
            name("direct"),
            "count",
            20_i64,
            2_i64,
            Attributes::from([("owner_type".into(), "wrong".into())]),
        )
        .await
        .unwrap();
    assert_eq!(model.getAttribute("owner_type"), Some(&"video".into()));
    assert_count(&model, 20);
    let model = morph()
        .one()
        .incrementOrCreate(
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
        .incrementOrCreate(
            &author,
            name("through"),
            "count",
            30_i64,
            2_i64,
            Attributes::from([("post_id".into(), 2.into())]),
        )
        .await
        .unwrap();
    assert_eq!(model.getAttribute("post_id"), post.getKey());
    assert_count(&model, 30);
    let model = through()
        .one()
        .incrementOrCreate(
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
            .incrementEach(
                Attributes::from([("count".into(), 3.into()), ("visits".into(), 4.into())]),
                Attributes::from([("name".into(), "renamed".into())]),
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(model.getAttribute("count"), Some(&35.into()));
    assert_eq!(model.getAttribute("visits"), Some(&14.into()));
    assert_eq!(model.getAttribute("name"), Some(&"renamed".into()));
    assert_eq!(
        model
            .decrementEach(
                Attributes::from([("count".into(), 2.into()), ("visits".into(), 5.into())]),
                Attributes::new(),
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(model.getAttribute("count"), Some(&33.into()));
    assert_eq!(model.getAttribute("visits"), Some(&9.into()));
    assert!(model.getDirty().unwrap().is_empty());
    assert!(
        model
            .incrementEach(
                Attributes::from([
                    ("count".into(), 1.into()),
                    ("visits".into(), "invalid".into())
                ]),
                Attributes::new(),
            )
            .await
            .is_err()
    );
    let persisted = RelationCounter::queryOn(db)
        .findOrFail(model.getKey().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(persisted.getAttribute("count"), Some(&33.into()));
    assert_eq!(persisted.getAttribute("visits"), Some(&9.into()));
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

    let mut existing = Post::queryOn(db)
        .findOrFail(created.getKey().unwrap().clone())
        .await
        .unwrap();
    existing.setAttribute("score", 701);
    assert!(
        relation
            .save(parent, &mut existing, joining("duplicate"))
            .await
            .is_err()
    );
    assert_eq!(
        Post::queryOn(db)
            .findOrFail(created.getKey().unwrap().clone())
            .await
            .unwrap()
            .getAttribute("score"),
        Some(&700.into())
    );
    assert!(existing.getDirty().unwrap().contains_key("score"));

    let saved = relation
        .createManyQuietly(
            parent,
            [model("batch one", 702), model("batch two", 703)],
            &[joining("one"), joining("two")],
        )
        .await
        .unwrap();
    assert_eq!(
        saved
            .iter()
            .map(|row| row.getAttribute("score").cloned().unwrap())
            .collect::<Vec<_>>(),
        vec![702.into(), 703.into()]
    );
    let mut batch = [
        Post::newInstance(model("saved batch one", 704)).unwrap(),
        Post::newInstance(model("saved batch rejected", 705)).unwrap(),
    ];
    assert!(
        relation
            .saveMany(
                parent,
                &mut batch,
                &[joining("committed"), Attributes::new()]
            )
            .await
            .is_err()
    );
    assert!(batch[0].state().exists());
    assert!(!batch[1].state().exists());
    let mut query = Post::queryOn(db);
    query.r#where("title", "=", "saved batch rejected");
    assert!(query.first().await.unwrap().is_none());
    relation
        .saveManyQuietly(parent, &mut batch[1..], &[joining("retry")])
        .await
        .unwrap();
    assert!(batch[1].state().exists());

    let forced = relation
        .forceCreateQuietly(
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
    assert_eq!(forced.getAttribute("author_id"), Some(&44.into()));
    let posts = FEATURED_POSTS
        .withPivot(["label"])
        .getResults(parent)
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
            .find(|model| model.getAttribute("score") == Some(&score.into()))
            .unwrap();
        assert_eq!(
            model
                .getRelation::<Pivot>("pivot")
                .unwrap()
                .getAttribute("label"),
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
    let parent = Author::queryOn(db)
        .useWritePdo()
        .findOrFail(1)
        .await
        .unwrap();
    let relation = FEATURED_POSTS.withPivot(["label"]);
    let create = |title: &str, score: i64, label: Value| {
        relation.createOrFirst(
            &parent,
            Attributes::from([("title".into(), title.into())]),
            Attributes::from([("score".into(), score.into())]),
            Attributes::from([("label".into(), label)]),
        )
    };
    let first = create("new", 10, "new pivot".into()).await.unwrap();
    assert!(first.getKey().is_some());
    let existing = create("existing", 30, "existing pivot".into())
        .await
        .unwrap();
    assert_eq!(existing.getKey(), Some(&2.into()));
    let attached = create("existing", 40, "ignored".into()).await.unwrap();
    assert_eq!(attached.getKey(), Some(&2.into()));
    assert_eq!(
        attached
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
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
    let mut rolled_back = Post::queryOn(db).useWritePdo();
    rolled_back.r#where("title", "=", "rollback");
    assert!(rolled_back.first().await.unwrap().is_none());
    if mysql {
        let (a, b) = tokio::join!(
            create("race", 60, "a".into()),
            create("race", 70, "b".into())
        );
        assert_eq!(a.unwrap().getKey(), b.unwrap().getKey());
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
    plain.detachOrFail(owner, None).await.unwrap();
    plain.attachOrFail(owner, &keys, label(11)).await.unwrap();
    assert_eq!(
        plain
            .updateExistingPivotOrFail(owner, &keys[1], label(88))
            .await
            .unwrap(),
        1
    );
    let custom = FEATURED_POSTS.using::<Membership>().withPivot(["label"]);
    let count_events = || db.table("relation_membership_events").count("*");
    let baseline = count_events().await.unwrap();
    assert!(custom.detachOrFail(owner, Some(&keys)).await.is_err());
    assert!(
        custom
            .updateExistingPivotOrFail(owner, &keys[0], label(88))
            .await
            .is_err()
    );
    assert_eq!(custom.getResults(owner).await.unwrap().len(), 2);
    assert_eq!(count_events().await.unwrap(), baseline);

    assert_eq!(
        plain.detachOrFail(owner, Some(&keys[1..])).await.unwrap(),
        1
    );
    let baseline = count_events().await.unwrap();
    assert!(
        custom
            .attachOrFail(owner, &keys[1..], label(77))
            .await
            .is_err()
    );
    assert!(
        custom
            .syncWithPivotValuesOrFail(owner, &keys[1..], label(77), true)
            .await
            .is_err()
    );
    let current = custom.getResults(owner).await.unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].getKey(), Some(&keys[0]));
    assert_eq!(
        current[0]
            .getRelation::<Membership>("pivot")
            .unwrap()
            .getAttributeValue("label")
            .unwrap(),
        Some(11.into())
    );
    assert_eq!(count_events().await.unwrap(), baseline);

    let changes = custom
        .syncWithoutDetachingOrFail(owner, &keys[1..])
        .await
        .unwrap();
    assert_eq!(changes.attached, keys[1..]);
    assert!(changes.detached.is_empty());
    let changes = custom.syncOrFail(owner, &keys[1..], true).await.unwrap();
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
            if model.getAttribute("title") == Some(&"reject batch row".into()) {
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
    DirectWritePost::registerModelEvent(app, Creating, DirectWriteEvents(calls.clone())).unwrap();

    // This fixture has explicit, non-generated IDs.
    let make = |id: i64, title: &str| {
        let mut model =
            DirectWritePost::newInstance(Attributes::from([("title".into(), title.into())]))
                .unwrap();
        model.setAttribute("id", id);
        model
    };
    let attributes = |id: i64, title: &str| {
        Attributes::from([("id".into(), id.into()), ("title".into(), title.into())])
    };
    let relation = HasMany::<Author, DirectWritePost>::new("id", "author_id");
    let created = relation
        .createMany(
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
            .map(|model| model.getKey().cloned().unwrap())
            .collect::<Vec<_>>(),
        vec![101.into(), 102.into()]
    );
    let quiet = relation
        .one()
        .createManyQuietly(parent, [attributes(103, "quiet batch row")])
        .await
        .unwrap();
    assert_eq!(quiet[0].getAttribute("author_id"), parent.getKey());

    let mut rows = [
        make(104, "first saved row"),
        make(105, "reject batch row"),
        make(106, "unvisited row"),
    ];
    assert!(
        HasMany::<Author, DirectWritePost>::new("id", "author_id")
            .saveMany(parent, &mut rows)
            .await
            .is_err()
    );
    assert!(rows[0].state().exists());
    assert!(!rows[1].state().exists());
    assert!(rows[2].getAttribute("author_id").is_none());
    HasOne::<Author, DirectWritePost>::new("id", "author_id")
        .saveManyQuietly(parent, &mut rows[1..])
        .await
        .unwrap();
    assert!(
        rows.iter()
            .all(|row| row.state().exists() && row.getAttribute("author_id") == parent.getKey())
    );
    let morph = MorphMany::<Author, DirectWritePost>::new("id", "author_id", "title", "morph");
    let created = morph
        .createQuietly(parent, attributes(107, "overwritten"))
        .await
        .unwrap();
    assert_eq!(created.getAttribute("title"), Some(&"morph".into()));
    let mut model = make(108, "overwritten");
    assert!(morph.one().saveQuietly(parent, &mut model).await.unwrap());
    assert_eq!(model.getAttribute("title"), Some(&"morph".into()));
    assert_eq!(model.getAttribute("author_id"), parent.getKey());
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
    let forced = direct.forceCreate(parent, protected(109)).await.unwrap();
    assert_eq!(forced.getAttribute("score"), Some(&3700.into()));
    assert_eq!(forced.getAttribute("author_id"), parent.getKey());

    let morph =
        || MorphMany::<Author, DirectWritePost>::new("id", "author_id", "title", "forced morph");
    let forced = morph()
        .forceCreateMany(parent, [protected(110), protected(111)])
        .await
        .unwrap();
    assert_eq!(
        forced
            .iter()
            .map(|model| model.getKey().cloned().unwrap())
            .collect::<Vec<_>>(),
        vec![110.into(), 111.into()]
    );
    assert!(
        forced
            .iter()
            .all(|model| model.getAttribute("title") == Some(&"forced morph".into()))
    );
    let quiet = morph()
        .one()
        .forceCreateQuietly(parent, protected(112))
        .await
        .unwrap();
    assert_eq!(quiet.getAttribute("title"), Some(&"forced morph".into()));
    let quiet = direct
        .one()
        .forceCreateManyQuietly(parent, [protected(113)])
        .await
        .unwrap();
    assert_eq!(quiet[0].getAttribute("score"), Some(&3700.into()));
    assert_eq!(calls.load(Ordering::Relaxed), 7);
    assert_eq!(
        DirectWritePost::queryOn(db)
            .findOrFail(113)
            .await
            .unwrap()
            .getAttribute("score"),
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

    fn handleInTransaction<'a>(
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
            let score = model.getAttribute("score").unwrap().clone();
            transaction
                .statement(
                    "INSERT INTO relation_counter_events (event, score) VALUES (?, ?)",
                    &[format!("{event:?}").into(), score],
                )
                .await?;
            if event == Updated && model.getAttribute("score") == Some(&14.into()) {
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
        Post::registerModelEvent(app, event, CounterEvents).unwrap();
    }
    db.statement(
        "CREATE TABLE relation_counter_events (event VARCHAR(20), score BIGINT)",
        &[],
    )
    .await
    .unwrap();
    let relation = FEATURED_POSTS;
    let increment = || {
        relation.incrementOrCreate(
            parent,
            Attributes::from([("title".into(), "second".into())]),
            "score",
            10_i64,
            2_i64,
            Attributes::new(),
        )
    };
    assert_eq!(
        increment().await.unwrap().getAttribute("score"),
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
        .orderBy("event", "asc")
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
        Post::queryOn(db)
            .findOrFail(2)
            .await
            .unwrap()
            .getAttribute("score"),
        Some(&12.into())
    );
    let mut model = Post::queryOn(db).findOrFail(2).await.unwrap();
    assert_eq!(
        model
            .incrementQuietly("score", 2, Attributes::new())
            .await
            .unwrap(),
        1
    );
    assert_eq!(model.getAttribute("score"), Some(&14.into()));
    assert_eq!(
        model
            .decrementQuietly("score", 1, Attributes::new())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        model
            .incrementEachQuietly(
                Attributes::from([("score".into(), 3.into())]),
                Attributes::new()
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        model
            .decrementEachQuietly(
                Attributes::from([("score".into(), 2.into())]),
                Attributes::new()
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(model.getAttribute("score"), Some(&14.into()));
    assert!(model.getDirty().unwrap().is_empty());
    assert_eq!(
        Post::queryOn(db)
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
    let mut query = TouchPost::queryOn(db);
    query.whereKey(2);
    assert_eq!(
        query
            .incrementEach(
                Attributes::from([("score".into(), 2.into())]),
                Attributes::new()
            )
            .await
            .unwrap(),
        1
    );
    let updated = TouchPost::queryOn(db).findOrFail(2).await.unwrap();
    assert_eq!(updated.getAttribute("score"), Some(&17.into()));
    assert_ne!(
        updated.getAttribute("updated_at"),
        Some(&"2000-01-01 00:00:00".into())
    );

    let mut query = TouchPost::queryOn(db);
    query.whereKey(2);
    assert_eq!(
        query
            .decrementEach(
                Attributes::from([("score".into(), 2.into())]),
                Attributes::from([("updated_at".into(), "2001-01-01 00:00:00".into())])
            )
            .await
            .unwrap(),
        1
    );
    let updated = TouchPost::queryOn(db).findOrFail(2).await.unwrap();
    assert_eq!(updated.getAttribute("score"), Some(&15.into()));
    assert_eq!(
        updated.getAttribute("updated_at"),
        Some(&"2001-01-01 00:00:00".into())
    );

    let mut query = TouchPost::queryOn(db);
    query.whereKey(3);
    assert_eq!(
        query
            .decrement("score", 1, Attributes::new())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        TouchPost::queryOn(db)
            .withTrashed()
            .findOrFail(3)
            .await
            .unwrap()
            .getAttribute("score"),
        Some(&10.into())
    );
    db.statement("DROP TABLE relation_counter_events", &[])
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
        Membership::registerModelEvent(&app, event, listener).unwrap();
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
    let owner = Author::queryOn(&db).findOrFail(1).await.unwrap();
    let relation = FEATURED_POSTS
        .using::<Membership>()
        .withPivot(["label"])
        .r#as("membership");
    assert!(
        relation
            .attachWith(
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
        .attachWith(
            &owner,
            &[1.into()],
            Attributes::from([("label".into(), "12".into())]),
        )
        .await
        .unwrap();
    assert!(
        FEATURED_POSTS
            .using::<Membership>()
            .chaperoneWith(Some("invalid"), None)
            .is_err()
    );

    let baseline = if mysql { select_count(&db).await } else { 0 };
    let eager = Author::queryOn(&db)
        .with(
            "posts",
            FEATURED_POSTS
                .using::<Membership>()
                .chaperone()
                .withPivot(["label"])
                .r#as("membership"),
        )
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    for author in eager.iter() {
        let posts = author
            .getRelation::<RelatedCollection<Post>>("posts")
            .unwrap();
        for post in posts {
            let pivot = post.getRelation::<Membership>("membership").unwrap();
            assert_eq!(
                pivot
                    .getRelation::<Option<Arc<Author>>>("author")
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .getKey(),
                author.getKey()
            );
            assert_eq!(
                pivot
                    .getRelation::<Option<Arc<Post>>>("post")
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .getKey(),
                post.getKey()
            );
            assert_eq!(
                post.toArray().unwrap()["membership"]["post"]["id"],
                post.toArray().unwrap()["id"]
            );
            assert!(
                post.toArray().unwrap()["membership"]["post"]
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
        .chaperoneWith(Some("owner"), Some("item"))
        .unwrap();
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let posts = lazy.getResults(&owner).await.unwrap();
    let pivot = posts[0].getRelation::<Membership>("pivot").unwrap();
    assert!(pivot.relationLoaded("owner") && pivot.relationLoaded("item"));
    assert!(!pivot.relationLoaded("author"));
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 1);
    }
    let posts = lazy.withoutChaperone().getResults(&owner).await.unwrap();
    assert!(
        !posts[0]
            .getRelation::<Membership>("pivot")
            .unwrap()
            .relationLoaded("owner")
    );
    assert!(
        FEATURED_POSTS.chaperone().getResults(&owner).await.unwrap()[0]
            .getRelation::<Pivot>("pivot")
            .is_some()
    );
    let posts = eager[0]
        .getRelation::<RelatedCollection<Post>>("posts")
        .unwrap();
    let pivot = posts
        .iter()
        .find(|post| post.getKey() == Some(&1.into()))
        .unwrap()
        .getRelation::<Membership>("membership")
        .unwrap();
    assert_eq!(pivot.getTable(), "relation_featured_posts");
    assert!(pivot.state().exists());
    assert_eq!(pivot.getAttributeValue("label").unwrap(), Some(12.into()));
    assert_eq!(
        posts
            .iter()
            .find(|post| post.getKey() == Some(&1.into()))
            .unwrap()
            .toArray()
            .unwrap()["membership"]["label"],
        12
    );
    assert_eq!(
        relation
            .updateExistingPivot(
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
            .updateExistingPivot(
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
    .withPivot(["label"]);
    let changes = morph
        .syncWithPivotValues(
            &owner,
            &[2.into()],
            Attributes::from([("label".into(), "15".into())]),
            true,
        )
        .await
        .unwrap();
    assert_eq!(changes.attached, vec![2.into()]);
    let posts = morph.getResults(&owner).await.unwrap();
    let pivot = posts[0].getRelation::<Membership>("pivot").unwrap();
    assert!(pivot.relationLoaded("author") && pivot.relationLoaded("post"));
    assert_eq!(pivot.getAttribute("subject_type"), Some(&"featured".into()));
    assert_eq!(pivot.getAttributeValue("label").unwrap(), Some(15.into()));
    assert_eq!(
        morph.detachOrFail(&owner, Some(&[2.into()])).await.unwrap(),
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
        .toggleWithPivotAttributes(
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

    #[allow(non_snake_case, reason = "Matches Laravel's collection factory hook")]
    fn newCollection(models: Vec<Self>) -> Collection<Self> {
        Collection::fromEntries(
            models
                .into_iter()
                .enumerate()
                .map(|(index, model)| (format!("post-{index}"), model)),
        )
    }
}

async fn verify_collection_factory(db: &Connection, author: &Author) {
    let selected = KeyedPost::queryOn(db)
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    assert!(!selected.is_empty());
    assert!(selected.getByKey("post-0").is_some());

    let keyed_posts = || {
        HasMany::<Author, KeyedPost>::new("id", "author_id")
            .chaperone("author")
            .constrain(|query| {
                query.withAttributes(Attributes::from([("score".into(), 7.into())]), false)
            })
    };
    let relation = keyed_posts();
    let loaded = relation.getResults(author).await.unwrap();
    assert!(!loaded.is_empty());
    assert_eq!(
        loaded.getByKey("post-0").unwrap().getKey(),
        loaded[0].getKey()
    );
    let mut lazy = Author::from_state(author.state().clone());
    assert!(
        lazy.getRelationOrLoad("keyedLazy", &relation)
            .await
            .unwrap()
            .getByKey("post-0")
            .is_some()
    );

    let mut query = Author::queryOn(db).with("keyedEager", keyed_posts());
    query.whereKey(author.getKey().unwrap().clone());
    let eager = query.firstOrFail().await.unwrap();
    let cached = eager
        .getRelation::<RelatedCollection<KeyedPost>>("keyedEager")
        .unwrap();
    assert_eq!(
        cached.getByKey("post-0").unwrap().getKey(),
        cached[0].getKey()
    );
    assert!(cached[0].relationLoaded("author"));
    assert!(
        eager.toArray().unwrap()["keyed_eager"]
            .get("post-0")
            .is_some()
    );
    let unsaved = Author::from_state(Default::default());
    assert!(relation.getResults(&unsaved).await.unwrap().is_empty());

    let models = relation
        .makeMany(
            author,
            [
                Attributes::from([("title".into(), "first draft".into())]),
                Attributes::from([("title".into(), "second draft".into())]),
            ],
        )
        .await
        .unwrap();
    for (index, title) in ["first draft", "second draft"].into_iter().enumerate() {
        let model = models.getByKey(format!("post-{index}")).unwrap();
        assert_eq!(model.getAttribute("title"), Some(&title.into()));
        assert_eq!(model.getAttribute("score"), Some(&7.into()));
        assert_eq!(model.getAttribute("author_id"), author.getKey());
        assert!(!model.state().exists());
        assert!(model.relationLoaded("author"));
    }
    let morph = MorphOne::<Author, Comment>::new("id", "subject_id", "subject_type", "author");
    let child = morph
        .make(
            author,
            Attributes::from([("body".into(), "draft subject".into())]),
        )
        .await
        .unwrap();
    assert_eq!(child.getAttribute("subject_id"), author.getKey());
    assert_eq!(child.getAttribute("subject_type"), Some(&"author".into()));
    assert!(!child.state().exists());
    let mut query = KeyedPost::queryOn(db);
    query.whereIn("title", vec!["first draft".into(), "second draft".into()]);
    assert!(query.get().await.unwrap().is_empty());
}

async fn verify_chaperone(db: &Connection, mysql: bool, parent: &Author, post: &Post) {
    let baseline = if mysql { select_count(db).await } else { 0 };
    let mut query = Author::queryOn(db).with("posts", POSTS.chaperone("author"));
    query.whereIn("id", vec![1.into(), 2.into()]);
    let authors = query.get().await.unwrap();
    let author_relation = AUTHOR;
    let mut count = 0;
    for author in authors {
        for loaded in author
            .getRelation::<RelatedCollection<Post>>("posts")
            .unwrap()
        {
            let mut post = Post::from_state(loaded.state().clone());
            let inverse = post
                .getRelationOrLoad("author", &author_relation)
                .await
                .unwrap();
            let inverse = inverse.as_ref().unwrap();
            assert_eq!(inverse.getKey(), author.getKey());
            assert!(!inverse.relationLoaded("posts"));
            count += 1;
        }
    }
    assert!(count >= 2);
    if mysql {
        assert_eq!(select_count(db).await - baseline, 2);
    }

    let plain = POSTS
        .inverse("author")
        .withoutInverse()
        .getResults(parent)
        .await
        .unwrap();
    assert!(!plain[0].relationLoaded("author"));
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
            .getRelation::<Option<Arc<Author>>>("author")
            .unwrap()
            .as_ref()
            .unwrap()
            .getKey(),
        parent.getKey(),
    );
    db.table("relation_posts")
        .r#where("id", "=", saved.getKey().unwrap().clone())
        .delete()
        .await
        .unwrap();

    let relation = POST_COMMENTS.chaperone("subject");
    let made = relation
        .firstOrNew(
            post,
            Attributes::from([("body".into(), "unsaved inverse".into())]),
            Attributes::new(),
        )
        .await
        .unwrap();
    let owner = made
        .getRelation::<Option<openrust::database::eloquent::relations::MorphTarget>>("subject")
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(owner.alias(), "post");
    assert_eq!(
        owner.downcast_ref::<Post>().unwrap().getKey(),
        post.getKey()
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
        .getResults(post)
        .await
        .unwrap()
        .unwrap();
    let mut loaded = Comment::from_state(loaded.state().clone());
    let subject =
        MorphTo::<Comment>::new("subject_type", "subject_id").target::<Post>("post", "id");
    let loaded_owner = loaded
        .getRelationOrLoad("subject", &subject)
        .await
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(
        loaded_owner.downcast_ref::<Post>().unwrap().getKey(),
        post.getKey()
    );
    if mysql {
        assert_eq!(select_count(db).await - baseline, 1);
    }
    db.table("relation_comments")
        .r#where("id", "=", saved.getKey().unwrap().clone())
        .delete()
        .await
        .unwrap();

    let mut absent = Author::newInstanceOn(db, Attributes::new()).unwrap();
    absent.setAttribute("id", 999);
    let default = FIRST_POST
        .chaperone("author")
        .withDefault(|_, _| {})
        .getResults(&absent)
        .await
        .unwrap()
        .unwrap();
    assert!(!default.state().exists());
    assert_eq!(
        default
            .getRelation::<Option<Arc<Author>>>("author")
            .unwrap()
            .as_ref()
            .unwrap()
            .getKey(),
        absent.getKey()
    );
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
        |name: &str| Author::queryOn(&db).create(Attributes::from([("name".into(), name.into())]));
    let first = author("First").await.unwrap();
    let mut second = author("Second").await.unwrap();
    second.setAttribute("mentor_id", first.getKey().unwrap().clone());
    second.save().await.unwrap();
    let third = author("No posts").await.unwrap();
    assert_eq!(
        Author::queryOn(&db)
            .has(&MENTEES)
            .unwrap()
            .get()
            .await
            .unwrap()
            .modelKeys(),
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
    let nested = Author::queryOn(&db)
        .withWhereHas(
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
    assert_eq!(nested.modelKeys(), vec![1.into()]);
    let posts = nested[0]
        .getRelation::<RelatedCollection<Post>>("posts")
        .unwrap();
    assert_eq!(posts.len(), 1);
    let parent = posts[0]
        .getRelation::<Option<Arc<Author>>>("author")
        .unwrap();
    assert_eq!(parent.as_ref().unwrap().getKey(), first.getKey());
    assert_eq!(
        Author::queryOn(&db)
            .whereDoesntHave(&POSTS)
            .unwrap()
            .get()
            .await
            .unwrap()
            .modelKeys(),
        vec![3.into()]
    );
    let filtered = POSTS.constrain(|mut query| {
        query
            .r#where("title", "=", "First post")
            .orWhere("title", "=", "Second post");
        query.orderBy("title", "desc").limit(1)
    });
    assert_eq!(
        filtered.getResults(&first).await.unwrap()[0].getKey(),
        p1.getKey()
    );
    let filtered = Author::queryOn(&db)
        .with("posts", filtered)
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(
        filtered
            .iter()
            .map(|model| model
                .getRelation::<RelatedCollection<Post>>("posts")
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
    let latest = Author::queryOn(&db)
        .with(
            "posts",
            POSTS.constrain(|query| query.orderBy("score", "desc").limit(1).offset(1)),
        )
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    for (author, key) in latest.iter().zip([p1.getKey(), p2.getKey()]) {
        let posts = author
            .getRelation::<RelatedCollection<Post>>("posts")
            .unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].getKey(), key);
        assert!(posts[0].getAttribute("laravel_row").is_none());
    }
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let aggregates = Author::queryOn(&db)
        .withCount("posts", &POSTS)
        .unwrap()
        .withCount(
            "posts as matching_posts",
            &POSTS.constrain(|mut query| {
                query.r#where("score", ">", 10);
                query.orderBy("score", "desc")
            }),
        )
        .unwrap()
        .withExists("posts", &POSTS)
        .unwrap()
        .withSum("posts", &POSTS, "score")
        .unwrap()
        .withAvg("posts", &POSTS, "score")
        .unwrap()
        .withMin("posts", &POSTS, "score")
        .unwrap()
        .withMax("posts", &POSTS, "score")
        .unwrap()
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 1);
    }
    assert_eq!(aggregates[0].getAttribute("name"), Some(&"First".into()));
    let numeric = |attribute: &str| match aggregates[0].getAttribute(attribute) {
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
        aggregates[0].getAttribute("posts_exists"),
        Some(&true.into())
    );
    assert_eq!(
        aggregates[2].getAttribute("posts_exists"),
        Some(&false.into())
    );
    assert_eq!(
        aggregates[2].getAttribute("posts_sum_score"),
        Some(&Value::Null)
    );
    assert_eq!(aggregates[2].getAttribute("posts_count"), Some(&0.into()));
    assert_eq!(
        Author::queryOn(&db)
            .withCount("mentees", &MENTEES)
            .unwrap()
            .findOrFail(1)
            .await
            .unwrap()
            .getAttribute("mentees_count"),
        Some(&1.into())
    );
    assert_eq!(
        Author::queryOn(&db)
            .hasCount(&POSTS, ">=", 2)
            .unwrap()
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap()
            .modelKeys(),
        vec![1.into(), 2.into()]
    );
    assert_eq!(
        Author::queryOn(&db)
            .hasCount(&POSTS, "=", 0)
            .unwrap()
            .get()
            .await
            .unwrap()
            .modelKeys(),
        vec![3.into()]
    );
    assert!(Author::queryOn(&db).hasCount(&POSTS, "like", 2).is_err());

    extra_first.setAttribute("score", 10);
    extra_first.save().await.unwrap();
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let ranked = Author::queryOn(&db)
        .with("top", POSTS.one().latestOfMany(Some("score")))
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    let winner = ranked[0]
        .getRelation::<Option<Arc<Post>>>("top")
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(winner.getKey(), extra_first.getKey());
    assert!(
        ranked[2]
            .getRelation::<Option<Arc<Post>>>("top")
            .unwrap()
            .is_none()
    );
    for (relation, expected) in [
        (
            POSTS.one().oldestOfMany(Some("score")),
            extra_first.getKey(),
        ),
        (
            POSTS.one().ofManyBy("score", OneOfManyAggregate::Min),
            p1.getKey(),
        ),
        (
            POSTS.one().ofMany(&[
                ("score", OneOfManyAggregate::Max),
                ("id", OneOfManyAggregate::Min),
            ]),
            p1.getKey(),
        ),
        (
            POSTS
                .one()
                .latestOfMany(Some("score"))
                .whereCandidates(|query| {
                    query
                        .r#where("title", "=", "First post")
                        .orWhere("title", "=", "Second post")
                })
                .unwrap(),
            p1.getKey(),
        ),
        (
            POSTS
                .one()
                .latestOfMany(Some("score"))
                .constrain(|query| query.withTrashed()),
            hidden.getKey(),
        ),
    ] {
        assert_eq!(
            relation.getResults(&first).await.unwrap().unwrap().getKey(),
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
            .latestOfMany(Some("score"))
            .getResults(&third)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        Author::queryOn(&db)
            .has(&POSTS.one().latestOfMany(Some("score")))
            .unwrap()
            .orderBy("id", "asc")
            .get()
            .await
            .unwrap()
            .modelKeys(),
        vec![1.into(), 2.into()]
    );
    null_rank.forceDelete().await.unwrap();
    extra_first.forceDelete().await.unwrap();
    extra_second.forceDelete().await.unwrap();
    assert_eq!(p1.getAttribute("author_id"), first.getKey());
    assert_eq!(p2.getAttribute("author_id"), second.getKey());
    assert_eq!(
        FIRST_POST
            .getResults(&second)
            .await
            .unwrap()
            .unwrap()
            .getKey(),
        p2.getKey()
    );
    assert!(FIRST_POST.getResults(&third).await.unwrap().is_none());

    let baseline = if mysql { select_count(&db).await } else { 0 };
    let mut authors = Author::queryOn(&db)
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    for author in authors.all() {
        POSTS.getResults(author).await.unwrap();
    }
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 4);
    }
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let authors_with_posts = Author::queryOn(&db)
        .with("posts", POSTS)
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    let related = |author: &Author| {
        author
            .getRelation::<RelatedCollection<Post>>("posts")
            .unwrap()
            .len()
    };
    assert_eq!(
        authors_with_posts.iter().map(related).collect::<Vec<_>>(),
        vec![1, 1, 0]
    );

    let baseline = if mysql { select_count(&db).await } else { 0 };
    authors.load("posts", &POSTS).await.unwrap();
    authors.loadMissing("posts", &POSTS).await.unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 1);
    }
    assert!(authors.contains(first.getKey().unwrap()).unwrap());
    assert_eq!(
        authors
            .find(first.getKey().unwrap())
            .unwrap()
            .unwrap()
            .getAttribute("name"),
        Some(&"First".into())
    );
    assert_eq!(authors.modelKeys(), vec![1.into(), 2.into(), 3.into()]);
    assert_eq!(
        authors
            .only(&[2_u64.to_string().into()])
            .unwrap()
            .modelKeys(),
        vec![2.into()]
    );

    verify_collection_keys(&db, &other).await;

    Box::pin(verify_belongs_to_cache(&db, mysql, &p1, &first, &second)).await;

    p1.load("author", &AUTHOR).await.unwrap();
    let related = p1.getRelation::<Option<Arc<Author>>>("author").unwrap();
    assert_eq!(related.as_ref().unwrap().getKey(), first.getKey());
    AUTHOR.associate(&mut p1, &second).unwrap();
    assert!(!p1.relationLoaded("author"));
    assert!(p1.save().await.unwrap());
    let found = Post::queryOn(&db)
        .with("author", AUTHOR)
        .findOrFail(p1.getKey().unwrap().clone())
        .await
        .unwrap();
    assert_eq!(
        found
            .getRelation::<Option<Arc<Author>>>("author")
            .unwrap()
            .as_ref()
            .unwrap()
            .getKey(),
        second.getKey()
    );
    AUTHOR.dissociate(&mut p1);
    assert!(p1.save().await.unwrap());
    assert!(AUTHOR.getResults(&p1).await.unwrap().is_none());
    assert_eq!(POSTS.getResults(&first).await.unwrap().len(), 0);
    assert_eq!(POSTS.getResults(&second).await.unwrap().len(), 1);
    let first_key = p1.getKey().unwrap().clone();
    let second_key = p2.getKey().unwrap().clone();
    FEATURED_POSTS
        .attachWith(
            &first,
            &[first_key.clone(), hidden.getKey().unwrap().clone()],
            Attributes::from([("label".into(), "featured".into())]),
        )
        .await
        .unwrap();
    FEATURED_POSTS
        .attach(&second, std::slice::from_ref(&second_key))
        .await
        .unwrap();
    let baseline = if mysql { select_count(&db).await } else { 0 };
    let featured = Author::queryOn(&db)
        .with(
            "featured",
            FEATURED_POSTS
                .withPivot(["label"])
                .constrain(|query| query.withCasts([("id", AttributeCast::String)])),
        )
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    if mysql {
        assert_eq!(select_count(&db).await - baseline, 2);
    }
    {
        let array = featured.toArray().unwrap();
        assert_eq!(array[0]["featured"][0]["pivot"]["label"], "featured");
        assert_eq!(array[2]["featured"], json!([]));
    }
    assert!(matches!(
        featured.all()[0]
            .getRelation::<RelatedCollection<Post>>("featured")
            .unwrap()[0]
            .getAttributeValue("id")
            .unwrap(),
        Some(Value::Text(_))
    ));
    assert_eq!(
        featured
            .iter()
            .map(|author| author
                .getRelation::<RelatedCollection<Post>>("featured")
                .unwrap()
                .len())
            .collect::<Vec<_>>(),
        vec![1, 1, 0]
    );
    assert_eq!(
        FEATURED_POSTS
            .updateExistingPivot(
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
            .updateExistingPivot(
                &first,
                &first_key,
                Attributes::from([("author_id".into(), 99.into())]),
            )
            .await
            .is_err()
    );
    let pivot_rows = FEATURED_POSTS
        .withPivot(["label"])
        .r#as("membership")
        .getResults(&first)
        .await
        .unwrap();
    let pivot = pivot_rows[0].getRelation::<Pivot>("membership").unwrap();
    assert_eq!(pivot.getAttribute("label"), Some(&"updated".into()));
    assert_eq!(pivot.getAttribute("author_id"), first.getKey());
    assert_eq!(pivot_rows[0].getAttribute("author_id"), Some(&Value::Null));
    let nested_pivot = Author::queryOn(&db)
        .withWhereHas(
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
    assert_eq!(nested_pivot.modelKeys(), vec![2.into()]);
    let posts = nested_pivot[0]
        .getRelation::<RelatedCollection<Post>>("featured")
        .unwrap();
    assert_eq!(posts.len(), 1);
    assert!(posts[0].relationLoaded("author"));

    let changes = FEATURED_POSTS
        .syncWithoutDetaching(&first, std::slice::from_ref(&second_key))
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
        .syncWithPivotAttributes(&first, &records, false)
        .await
        .unwrap();
    assert_eq!(changes.updated, vec![first_key.clone(), second_key.clone()]);
    assert_eq!(
        FEATURED_POSTS
            .syncWithPivotAttributes(&first, &records, false)
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
            .syncWithPivotAttributes(&first, &invalid, true)
            .await
            .is_err()
    );
    assert_eq!(FEATURED_POSTS.getResults(&first).await.unwrap().len(), 2);
    let limited_pivots = Author::queryOn(&db)
        .with(
            "featured",
            FEATURED_POSTS
                .withPivot(["label"])
                .constrain(|query| query.orderBy("score", "asc").limit(1)),
        )
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap();
    let links = limited_pivots[0]
        .getRelation::<RelatedCollection<Post>>("featured")
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].getKey(), Some(&first_key));
    assert_eq!(
        links[0]
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
        Some(&"first".into())
    );
    let changes = FEATURED_POSTS
        .syncWithPivotValues(
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
    assert!(FEATURED_POSTS.getResults(&first).await.unwrap().is_empty());
    assert_eq!(FEATURED_POSTS.getResults(&second).await.unwrap().len(), 1);
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
    Box::pin(verify_collection_factory(&db, &first)).await;
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

    MorphMap::enforceMorphMap(&first, [MorphMapEntry::model::<Author>("writer")], false).unwrap();
    MorphMap::morphMap(&second, [MorphMapEntry::model::<Author>("member")], false).unwrap();
    let owner = Author::queryOn(&db).findOrFail(1).await.unwrap();
    let other_owner = Author::queryOn(&other).findOrFail(1).await.unwrap();
    assert_eq!(owner.getMorphClass().unwrap(), "writer");
    assert_eq!(other_owner.getMorphClass().unwrap(), "member");
    assert!(matches!(
        Post::queryOn(&db)
            .findOrFail(1)
            .await
            .unwrap()
            .getMorphClass(),
        Err(DatabaseError::InvalidQuery(
            "model requires a morph map entry"
        ))
    ));
    let registry = first.container().make::<MorphMap>().unwrap();
    assert_eq!(
        registry.getMorphedModel("writer"),
        Some(std::any::type_name::<Author>())
    );
    assert_eq!(registry.getMorphedModel("member"), None);
    assert!(MorphMap::morphMap(&first, [MorphMapEntry::model::<Post>("")], false).is_err());
    assert_eq!(owner.getMorphClass().unwrap(), "writer");

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
    assert_eq!(comment.getAttribute("subject_type"), Some(&"writer".into()));
    let inverse = MorphTo::<Comment>::new("subject_type", "subject_id")
        .targetMapped::<Author>(&db, "id")
        .unwrap();
    assert_eq!(
        inverse
            .getResults(&comment)
            .await
            .unwrap()
            .unwrap()
            .downcast_ref::<Author>()
            .unwrap()
            .getKey(),
        owner.getKey()
    );
    inverse.associateMapped(&mut comment, &owner).unwrap();
    assert_eq!(comment.getAttribute("subject_type"), Some(&"writer".into()));
    MorphMap::morphMap(&first, [MorphMapEntry::model::<Author>("author_v2")], true).unwrap();
    assert_eq!(owner.getMorphClass().unwrap(), "author_v2");
    assert_eq!(registry.getMorphAlias::<Author>(), "author_v2");
    let inverse = MorphTo::<Comment>::new("subject_type", "subject_id")
        .targetMapped::<Author>(&db, "id")
        .unwrap();
    assert_eq!(
        inverse.getResults(&comment).await.unwrap().unwrap().alias(),
        "writer"
    );
    inverse.associateMapped(&mut comment, &owner).unwrap();
    assert_eq!(
        comment.getAttribute("subject_type"),
        Some(&"author_v2".into())
    );
    assert!(registry.getMorphedModel("writer").is_some());
    db.statement("DROP TABLE relation_comments", &[])
        .await
        .unwrap();

    type Posts = HasMany<Author, Post>;
    let named = DynamicRelation::<Posts>::new("selectedPosts");
    let register = |app: &Application, title| {
        Author::resolveRelationUsing(app, "selectedPosts", move |_| {
            Posts::new("id", "author_id").constrain(move |mut query| {
                query.r#where("title", "=", title);
                query
            })
        })
        .unwrap();
    };
    register(&first, "first");
    let mut author = Author::queryOn(&db).findOrFail(1).await.unwrap();
    let mut other_author = Author::queryOn(&other).findOrFail(1).await.unwrap();
    assert!(
        other_author
            .relationResolver::<Posts>("selectedPosts")
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        other_author
            .getDynamicRelationOrLoad::<Posts>("selectedPosts")
            .await,
        Err(DatabaseError::InvalidQuery("unknown dynamic relationship"))
    ));
    assert!(!other_author.relationLoaded("selectedPosts"));
    assert!(matches!(
        Author::queryOn(&other).whereHas(&named),
        Err(DatabaseError::InvalidQuery("unknown dynamic relationship"))
    ));
    register(&second, "second");
    assert_eq!(
        other_author
            .getDynamicRelationOrLoad::<Posts>("selectedPosts")
            .await
            .unwrap()[0]
            .getKey(),
        Some(&2.into())
    );
    assert_eq!(
        author
            .getDynamicRelationOrLoad::<Posts>("selectedPosts")
            .await
            .unwrap()[0]
            .getKey(),
        Some(&1.into())
    );
    assert!(author.toArray().unwrap().contains_key("selected_posts"));
    assert!(matches!(
        author.relationResolver::<BelongsTo<Author, Post>>("selectedPosts"),
        Err(DatabaseError::InvalidQuery(
            "dynamic relationship descriptor type"
        ))
    ));

    Author::resolveRelationUsing(&second, "selectedPosts", |_| Posts::new("id", "author_id"))
        .unwrap();
    let exists = Author::queryOn(&db)
        .whereHas(&named)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(exists.len(), 1);
    assert_eq!(exists[0].getKey(), Some(&1.into()));
    let absent = Author::queryOn(&db)
        .whereDoesntHave(&named)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(absent.len(), 1);
    assert_eq!(absent[0].getKey(), Some(&2.into()));
    let counts = Author::queryOn(&other)
        .hasCount(&named, ">=", 2)
        .unwrap()
        .withCount("selectedPosts", &named)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(counts.len(), 1);
    assert_eq!(
        counts[0].getAttribute("selected_posts_count"),
        Some(&2.into())
    );
    let wrong = DynamicRelation::<BelongsTo<Author, Post>>::new("selectedPosts");
    assert!(matches!(
        Author::queryOn(&db).whereHas(&wrong),
        Err(DatabaseError::InvalidQuery(
            "dynamic relationship descriptor type"
        ))
    ));

    register(&first, "second");
    assert_eq!(
        author
            .getDynamicRelationOrLoad::<Posts>("selectedPosts")
            .await
            .unwrap()[0]
            .getKey(),
        Some(&1.into())
    );
    author.unsetRelation("selectedPosts");
    assert_eq!(
        author
            .getDynamicRelationOrLoad::<Posts>("selectedPosts")
            .await
            .unwrap()[0]
            .getKey(),
        Some(&2.into())
    );

    Author::resolveRelationUsing(&first, "selectedPosts", |parent| {
        let title = if parent.getKey() == Some(&1.into()) {
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
    let authors = Author::queryOn(&db)
        .orderBy("id", "asc")
        .with(
            "selectedPosts",
            DynamicRelation::<Posts>::new("selectedPosts"),
        )
        .get()
        .await
        .unwrap();
    for (author, id) in authors.iter().zip([1, 3]) {
        let posts = author
            .getRelation::<RelatedCollection<Post>>("selectedPosts")
            .unwrap();
        assert_eq!(posts[0].getKey(), Some(&id.into()));
    }
    let prototype_matches = Author::queryOn(&db)
        .whereHas(&named)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(prototype_matches.len(), 1);
    assert_eq!(prototype_matches[0].getKey(), Some(&2.into()));
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
    StrictAuthor::inheritRelationUsing(first, name, base, Posts::forParent).unwrap();
    assert!(matches!(
        StrictAuthor::queryOn(db).whereHas(&relation),
        Err(DatabaseError::InvalidQuery("unknown dynamic relationship"))
    ));
    Author::resolveRelationUsing(first, name, |_| {
        Posts::new("id", "author_id").constrain(|mut query| {
            query.r#where("title", "=", "third");
            query
        })
    })
    .unwrap();
    let authors = StrictAuthor::queryOn(db)
        .withWhereHas(name, relation)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(authors.len(), 1);
    assert_eq!(authors[0].getKey(), Some(&2.into()));
    assert_eq!(
        authors[0]
            .getRelation::<RelatedCollection<Post>>(name)
            .unwrap()[0]
            .getKey(),
        Some(&3.into())
    );
    let outside = StrictAuthor::queryOn(other).findOrFail(2).await.unwrap();
    assert!(
        outside
            .relationResolver::<ComposedPosts>(name)
            .unwrap()
            .is_none()
    );
    StrictAuthor::inheritRelationUsing(second, name, base, Posts::forParent).unwrap();
    assert!(
        outside
            .relationResolver::<ComposedPosts>(name)
            .unwrap()
            .is_none()
    );

    StrictAuthor::resolveRelationUsing(first, name, |_| ComposedPosts::new("id", "author_id"))
        .unwrap();
    let relation = DynamicRelation::<ComposedPosts>::new(name);
    assert_eq!(
        StrictAuthor::queryOn(db)
            .whereHas(&relation)
            .unwrap()
            .get()
            .await
            .unwrap()
            .len(),
        2
    );
    let base_relation = DynamicRelation::<Posts>::new(name);
    assert_eq!(
        Author::queryOn(db)
            .whereHas(&base_relation)
            .unwrap()
            .get()
            .await
            .unwrap()
            .len(),
        1
    );

    let cyclic = "cyclic";
    StrictAuthor::inheritRelationUsing(first, cyclic, base, Posts::forParent).unwrap();
    Author::inheritRelationUsing(
        first,
        cyclic,
        |model| StrictAuthor::from_state(model.state().clone()),
        ComposedPosts::forParent,
    )
    .unwrap();
    assert!(matches!(
        StrictAuthor::queryOn(db).whereHas(&DynamicRelation::<ComposedPosts>::new(cyclic)),
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
            .uniqueId(openrust::database::eloquent::UniqueId::Ulid);
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
        let mut parent = Author::newInstanceOn(db, Attributes::new()).unwrap();
        parent.setAttribute("id", Value::Integer(id));
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
        let mut query = UpsertChild::queryOn(db);
        query
            .r#where("owner_type", "=", owner_type)
            .r#where("owner_id", "=", id)
            .r#where("code", "=", "a");
        query.firstOrFail()
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
    assert_eq!(original.getAttribute("id"), changed.getAttribute("id"));
    assert!(matches!(changed.getAttribute("id"), Some(Value::Text(id)) if id.len() == 26));
    assert_eq!(
        original.getAttribute("created_at"),
        changed.getAttribute("created_at")
    );
    assert_eq!(
        changed.getAttribute("updated_at"),
        Some(&"explicit timestamp".into())
    );
    assert_eq!(changed.getAttribute("body"), Some(&"updated".into()));
    assert_eq!(
        lookup("direct", 2).await.unwrap().getAttribute("body"),
        Some(&"second parent".into())
    );
    assert_eq!(
        lookup("morph", 1).await.unwrap().getAttribute("body"),
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
        assert_eq!(saved.getAttribute("body"), Some(&"updated".into()));
        assert!(matches!(saved.getAttribute("id"), Some(Value::Text(id)) if id.len() == 26));
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
        lookup("through", 77).await.unwrap().getAttribute("body"),
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
    let mut child = Comment::newInstanceOn(db, Attributes::new()).unwrap();
    child.setAttribute("subject_id", 999);
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
        child.setAttribute("subject_type", alias);
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
    child.setAttribute("subject_type", "owner");
    morph
        .upsert(&child, vec![owner("updated")], &["id"], Some(&["body"]))
        .await
        .unwrap();
    let updated = UpsertOwner::queryOn(db).findOrFail(42).await.unwrap();
    assert_eq!(updated.getAttribute("body"), Some(&"updated".into()));

    child.setAttribute("subject_type", "other");
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
    assert!(matches!(other.getAttribute("id"), Some(Value::Text(id)) if id.len() == 26));
    assert_eq!(other.getAttribute("body"), Some(&"other target".into()));
    assert_eq!(child.getAttribute("subject_id"), Some(&999.into()));
    assert_eq!(child.getAttribute("subject_type"), Some(&"other".into()));
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
