use openrust::{config::Repository, contracts::config::Repository as _};
use openrust_content_moderation::{
    Assessment, Coverage, Decision, ImageInput, Modality, ModerationDriver, ModerationError,
    ModerationFuture, ModerationManager, Operation, StoredMedia, TaskId, TextInput, VideoInput,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn inputs_and_task_ids_validate_at_public_boundaries() {
    assert!(TextInput::new(" \n ").is_err());
    let text = TextInput::new(" 保留 UTF-8 原文 🙂 ")
        .unwrap()
        .with_language("zh-Hans")
        .unwrap();
    assert_eq!(text.text(), " 保留 UTF-8 原文 🙂 ");
    assert_eq!(text.language(), Some("zh-Hans"));
    assert!(text.with_language("zh--Hans").is_err());
    assert!(ImageInput::inline(vec![]).is_err());
    let inline = ImageInput::inline(vec![1, 2, 3]).unwrap();
    assert_eq!(inline.as_bytes(), Some(&[1, 2, 3][..]));
    assert!(inline.as_stored().is_none());
    let source = StoredMedia::new("private", "uploads/媒体.mp4", "revision-2").unwrap();
    assert_eq!(
        VideoInput::new(source.clone()).source().version(),
        "revision-2"
    );
    assert_eq!(
        ImageInput::stored(source).as_stored().unwrap().disk(),
        "private"
    );
    let id = TaskId::new("任务-42").unwrap();
    assert_eq!(serde_json::to_string(&id).unwrap(), "\"任务-42\"");
    assert_eq!(
        serde_json::from_str::<TaskId>("\"任务-42\"")
            .unwrap()
            .as_str(),
        id.as_str()
    );
    assert!(serde_json::from_str::<TaskId>("\"\\u0000\"").is_err());
    assert!(TaskId::new("x".repeat(257)).is_err());
}

#[test]
fn allow_requires_every_requested_modality_to_be_complete() {
    let coverage = BTreeMap::from([
        (Modality::Visual, Coverage::Complete),
        (Modality::Speech, Coverage::Partial),
    ]);
    let clean = Assessment::new(Decision::Allow, coverage.clone(), vec![]);
    assert_eq!(clean.coverage(Modality::Ocr), Coverage::Unavailable);
    assert_eq!(clean.decision(&[Modality::Visual]), Some(Decision::Allow));
    assert_eq!(clean.decision(&[Modality::Visual, Modality::Speech]), None);
    assert_eq!(clean.decision(&[Modality::Ocr]), None);
    assert_eq!(clean.decision(&[]), None);
    let blocked = Assessment::new(Decision::Block, coverage, vec![]);
    assert_eq!(
        blocked.decision(&[Modality::Visual, Modality::Ocr]),
        Some(Decision::Block)
    );
}

struct TextOnly(Decision);

impl ModerationDriver for TextOnly {
    fn supports(&self, operation: Operation) -> bool {
        operation == Operation::Text
    }

    fn text<'a>(&'a self, _: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async {
            Ok(Assessment::new(
                self.0,
                BTreeMap::from([(Modality::Text, Coverage::Complete)]),
                vec![],
            ))
        })
    }
}

#[tokio::test]
async fn external_driver_uses_instance_scope_and_explicit_refresh() {
    let config = Arc::new(Repository::new(BTreeMap::from([(
        "moderation".into(),
        json!({
            "default":"posts", "drivers":{
                "posts":{"driver":"fixture","review":false}, "chat":{"driver":"fixture","review":true},
                "unregistered":{"driver":"absent"},
            },
        }),
    )])));
    let manager = ModerationManager::new(config.clone());
    let other = ModerationManager::new(config.clone());
    assert!(matches!(
        manager.driver("missing"),
        Err(ModerationError::ProfileNotConfigured)
    ));
    assert!(matches!(
        manager.driver("unregistered"),
        Err(ModerationError::DriverNotRegistered)
    ));
    let attempts = Arc::new(AtomicUsize::new(0));
    let calls = attempts.clone();
    manager.extend("fixture", move |config| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(ModerationError::InvalidConfiguration("initialization"));
        }
        let decision = if config["review"] == true {
            Decision::Review
        } else {
            Decision::Allow
        };
        Ok(Arc::new(TextOnly(decision)))
    });
    assert!(manager.default_driver().is_err());
    let posts = manager.default_driver().unwrap();
    assert!(Arc::ptr_eq(&posts, &manager.driver("posts").unwrap()));
    let chat = manager.driver("chat").unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    let sample = TextInput::new("样本").unwrap();
    assert_eq!(
        posts
            .text(&sample)
            .await
            .unwrap()
            .decision(&[Modality::Text]),
        Some(Decision::Allow)
    );
    assert_eq!(
        chat.text(&sample)
            .await
            .unwrap()
            .decision(&[Modality::Text]),
        Some(Decision::Review)
    );
    assert!(!posts.supports(Operation::QueryVideo));
    assert!(matches!(
        posts
            .query_video(&TaskId::new("not-supported").unwrap())
            .await,
        Err(ModerationError::Unsupported(Operation::QueryVideo))
    ));
    assert!(other.default_driver().is_err());
    manager.extend("fixture", |_| Ok(Arc::new(TextOnly(Decision::Block))));
    config.set("moderation.default", json!("chat")).unwrap();
    assert!(Arc::ptr_eq(&chat, &manager.default_driver().unwrap()));
    let new = manager.forget_drivers().default_driver().unwrap();
    assert_eq!(
        new.text(&sample).await.unwrap().decision(&[Modality::Text]),
        Some(Decision::Block)
    );
    assert_eq!(
        chat.text(&sample)
            .await
            .unwrap()
            .decision(&[Modality::Text]),
        Some(Decision::Review)
    );
}
