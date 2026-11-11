use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Builder, Collection, EagerLoads, Model, ModelMetadata, ModelState,
            RelationAggregate, RelationAggregateQuery,
            casts::AttributeCast,
            relations::{
                BelongsTo, BelongsToMany, HasMany, HasManyThrough, HasOne, HasOneThrough,
                MorphMany, MorphOne, MorphPivot, MorphTarget, MorphTo, MorphToMany,
                OneOfManyAggregate, Pivot, Relation,
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

async fn verify_through_and_morph(
    db: &Connection,
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
            .getRelation::<Vec<Arc<Comment>>>("recentComments")
            .unwrap()[0]
            .getAttributeValue("id")
            .unwrap(),
        Some(Value::Text(_))
    ));
    assert_eq!(
        authors
            .iter()
            .map(|model| model
                .getRelation::<Vec<Arc<Comment>>>("recentComments")
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
        .morphWithCount::<Post>("post", "id", "comments", POST_COMMENTS);
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

    assert!(subject.associate(&mut profile, "post", author).is_err());
    subject.associate(&mut profile, "post", post).unwrap();
    profile.save().await.unwrap();
    assert_eq!(POST_COMMENTS.getResults(post).await.unwrap().len(), 2);
    subject.dissociate(&mut profile);
    profile.save().await.unwrap();
    assert!(subject.getResults(&profile).await.unwrap().is_none());
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
    let tags = post_tags
        .withPivot(["label"])
        .getResults(post)
        .await
        .unwrap();
    let pivot = tags[0].getRelation::<MorphPivot>("pivot").unwrap();
    assert_eq!(pivot.getAttribute("subject_type"), Some(&"post".into()));
    assert_eq!(pivot.getAttribute("label"), Some(&"post link".into()));
    let post_tags = MorphToMany::<Post, Author>::new(
        "relation_subject_tags",
        "subject_id",
        "tag_id",
        "id",
        "id",
        "subject_type",
        "post",
    );
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

        let by_name = BelongsTo::<Comment, Author>::new("body", "name");
        let matched = Comment::queryOn(db)
            .whereBelongsTo(&by_name, &[author])
            .unwrap()
            .get()
            .await
            .unwrap();
        assert_eq!(bodies(&matched), vec!["First".into()]);
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
        assert!(group.getRelationOrLoad(1, "posts", &broken).await.is_err());
        assert!(group[0].relationLoaded("posts"));
        assert!(!group[1].relationLoaded("posts"));
        assert!(!group[2].relationLoaded("posts"));

        let baseline = if mysql { select_count(db).await } else { 0 };
        group.getRelationOrLoad(1, "posts", &POSTS).await.unwrap();
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

        let mut other = ordered().get().await.unwrap();
        assert!(other.iter().all(|author| !author.relationLoaded("posts")));
        other.getRelationOrLoad(2, "posts", &POSTS).await.unwrap();
        assert!(!other[0].relationLoaded("posts"));
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
                .getRelation::<Vec<Arc<Post>>>("posts")
                .unwrap()
                .is_empty()
        );
        assert!(
            models[2]
                .getRelation::<Vec<Arc<Post>>>("posts")
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
                .getRelation::<Vec<Arc<Post>>>("posts")
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
                    .getRelation::<Vec<Arc<Post>>>("posts")
                    .unwrap()
                    .is_empty()
        }));

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

fn verify_collection_keys(db: &Connection) -> openrust::database::DatabaseFuture<'_, ()> {
    Box::pin(async move {
        let ordered = || Author::queryOn(db).orderBy("id", "asc");
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
        let distinct = Collection::new(duplicate).unique().unwrap();
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
        let other = connect(json!({"driver":"sqlite","database":":memory:"}));
        let mut outside = Author::queryOn(&other).make(Attributes::new()).unwrap();
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

async fn verify(db: Connection, mysql: bool) {
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
    db.statement("CREATE TABLE relation_featured_posts (author_id BIGINT NOT NULL, post_id BIGINT NOT NULL, label VARCHAR(80), PRIMARY KEY (author_id, post_id), FOREIGN KEY (author_id) REFERENCES relation_authors(id), FOREIGN KEY (post_id) REFERENCES relation_posts(id))", &[]).await.unwrap();
    db.statement(&format!("CREATE TABLE relation_comments (id {id}, post_id BIGINT, subject_id BIGINT, subject_type VARCHAR(80), body VARCHAR(80), deleted_at DATETIME)"), &[]).await.unwrap();
    db.statement("CREATE TABLE relation_subject_tags (subject_type VARCHAR(80) NOT NULL, subject_id BIGINT NOT NULL, tag_id BIGINT NOT NULL, label VARCHAR(80), PRIMARY KEY (subject_type, subject_id, tag_id), FOREIGN KEY (tag_id) REFERENCES relation_authors(id))", &[]).await.unwrap();
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
    let posts = nested[0].getRelation::<Vec<Arc<Post>>>("posts").unwrap();
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
            .map(|model| model.getRelation::<Vec<Arc<Post>>>("posts").unwrap().len())
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
        let posts = author.getRelation::<Vec<Arc<Post>>>("posts").unwrap();
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
    let related = |author: &Author| author.getRelation::<Vec<Arc<Post>>>("posts").unwrap().len();
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

    verify_collection_keys(&db).await;

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
            .getRelation::<Vec<Arc<Post>>>("featured")
            .unwrap()[0]
            .getAttributeValue("id")
            .unwrap(),
        Some(Value::Text(_))
    ));
    assert_eq!(
        featured
            .iter()
            .map(|author| author
                .getRelation::<Vec<Arc<Post>>>("featured")
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
        .getRelation::<Vec<Arc<Post>>>("featured")
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
        .getRelation::<Vec<Arc<Post>>>("featured")
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
    AUTHOR.associate(&mut p1, &first).unwrap();
    p1.save().await.unwrap();
    verify_through_and_morph(&db, mysql, &first, &p1, &hidden).await;
    verify_one_through(&db, mysql, &first).await;
    verify_collection_reload(&db, mysql).await;
    verify_relation_autoload(&db, mysql).await;
    verify_relation_chain(&db, mysql).await;
    verify_morph_batches(&db, mysql).await;
    verify_relation_identity(&db).await;
    verify_lazy_loading_policy(&db, mysql).await;
    verify_default_models(&db, mysql).await;
    verify_default_eager_loads(&db, mysql).await;
    verify_morph_wildcard(&db, mysql).await;
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

#[tokio::test]
async fn relationship_keys_and_eager_loading_on_sqlite() {
    verify(
        connect(json!({"driver":"sqlite","database":":memory:","max_connections":1,"foreign_key_constraints":true})),
        false,
    )
    .await;
}

#[tokio::test]
async fn relationship_keys_and_query_counts_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_RELATION_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_RELATION_MYSQL").is_none());
    if let Some(url) = url {
        let db = connect(json!({"url":url,"max_connections":1}));
        // Keep the large integration fixture off the default debug test stack.
        Box::pin(verify(db, true)).await;
    }
}
