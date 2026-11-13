use std::{collections::BTreeMap, sync::Arc};

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
    fn namedRelations() -> NamedRelations<Self> {
        NamedRelations::default()
            .nested("posts", || POSTS)
            .nested("details", || POSTS)
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
        for post in author.getRelation::<Vec<Arc<Post>>>("posts").unwrap() {
            assert!(post.relationLoaded("author"));
            found_comments |= !post
                .getRelation::<Vec<Arc<Comment>>>("comments")
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
        for post in author.getRelation::<Vec<Arc<Post>>>("posts").unwrap() {
            for comment in post.getRelation::<Vec<Arc<Comment>>>("comments").unwrap() {
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
                .getRelation::<Vec<Arc<Post>>>("details")
                .unwrap()
                .len()
        } else {
            target
                .downcast_ref::<Post>()
                .unwrap()
                .getRelation::<Vec<Arc<Comment>>>("details")
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
    assert!(absent.modelKeys().contains(&no_target));
    db.table("relation_comments")
        .whereIn("id", vec![no_target, additional])
        .delete()
        .await
        .unwrap();
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
        let second_posts = second.getRelation::<Vec<Arc<Post>>>("posts").unwrap();
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
        let loaded = filtered[0].getRelation::<Vec<Arc<Post>>>("posts").unwrap();
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

async fn verify_pivot_constraints(
    db: &Connection,
    first: &Author,
    first_key: &Value,
    second_key: &Value,
) {
    let ids = |posts: Vec<Arc<Post>>| {
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
            .getRelation::<Vec<Arc<Post>>>("filtered")
            .unwrap()
            .clone()),
        vec![second_key.clone()]
    );
    assert!(
        owners[1]
            .getRelation::<Vec<Arc<Post>>>("filtered")
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
    let posts = eager.getRelation::<Vec<Arc<Post>>>("ordered").unwrap();
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
                if event == Created && label == Some(77.into()) {
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
        "CREATE TABLE relation_posts (id BIGINT PRIMARY KEY, author_id BIGINT, title VARCHAR(80), deleted_at DATETIME, updated_at VARCHAR(32), CHECK (id <> 4 OR updated_at = '2000-01-01 00:00:00'))",
        "CREATE TABLE relation_featured_posts (author_id BIGINT NOT NULL, post_id BIGINT NOT NULL, label VARCHAR(80), PRIMARY KEY (author_id, post_id))",
        "CREATE TABLE relation_membership_events (event VARCHAR(30) NOT NULL)",
        "INSERT INTO relation_authors VALUES (1,'first','2000-01-01 00:00:00'),(2,'second','2000-01-01 00:00:00')",
        "INSERT INTO relation_posts VALUES (1,1,'first',NULL,'2000-01-01 00:00:00'),(2,1,'second',NULL,'2000-01-01 00:00:00')",
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
        let posts = author.getRelation::<Vec<Arc<Post>>>("posts").unwrap();
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
    let posts = eager[0].getRelation::<Vec<Arc<Post>>>("posts").unwrap();
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
    assert_eq!(morph.detach(&owner, Some(&[2.into()])).await.unwrap(), 1);
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
    db.statement("INSERT INTO relation_posts VALUES (3,1,'deleted','2000-01-01 00:00:00','2000-01-01 00:00:00'),(4,1,'reject touch',NULL,'2000-01-01 00:00:00')", &[]).await.unwrap();
    Box::pin(verify_related_touch(&db)).await;
    Box::pin(verify_save_touches_related(&db)).await;
    Box::pin(verify_inferred_pivot_touch(&db)).await;
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
    verify_through_and_morph(&db, mysql, &first, &p1, &hidden).await;
    Box::pin(verify_morph_paths(&db, mysql)).await;
    verify_one_through(&db, mysql, &first).await;
    verify_collection_reload(&db, mysql).await;
    verify_relation_autoload(&db, mysql).await;
    verify_relation_chain(&db, mysql).await;
    Box::pin(verify_named_paths(&db, mysql)).await;

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
            .getRelation::<Vec<Arc<Post>>>("selectedPosts")
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
        authors[0].getRelation::<Vec<Arc<Post>>>(name).unwrap()[0].getKey(),
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
        let db = connect(json!({"url":url,"max_connections":1}));
        // Keep the large integration fixture off the default debug test stack.
        Box::pin(verify(db, true)).await;
        Box::pin(verify_dynamic_relations(
            json!({"url":url,"max_connections":1}),
        ))
        .await;
        Box::pin(verify_custom_pivot(json!({"url":url,"max_connections":1}))).await;
    }
}
