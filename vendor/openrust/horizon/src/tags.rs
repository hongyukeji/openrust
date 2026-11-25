use crate::{Error, Result};
use openrust::{
    container::ServiceKey,
    contracts::container::BindingResolutionError,
    database::{Value, eloquent::Model},
    queue::{JobType, Payload},
    support::facades::Facade,
};
use std::{borrow::Cow, collections::HashMap, sync::RwLock};

pub type TagCallback = fn(&Payload) -> Vec<String>;
pub type ModelTagCallback = fn(&Payload) -> Result<Vec<String>>;

#[derive(Clone, Copy, Default)]
struct TargetCallbacks {
    tags: Option<TagCallback>,
    models: Option<ModelTagCallback>,
}

#[derive(Default)]
pub(crate) struct TagManager {
    callbacks: RwLock<Vec<TagCallback>>,
    targets: RwLock<HashMap<&'static str, TargetCallbacks>>,
}

impl TagManager {
    fn callbacks_for(&self, target: &str) -> TargetCallbacks {
        self.targets
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(target)
            .copied()
            .unwrap_or_default()
    }
}
struct TagStore;
impl Facade for TagStore {
    type Service = TagManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<TagManager>()
    }
}
/// 在当前应用作用域解析任务及底层目标的标签。
pub struct Tags;
impl Tags {
    pub fn for_payload(payload: &Payload) -> Result<Vec<String>> {
        let manager = TagStore::get_facade_root().map_err(|_| Error::Unavailable)?;
        let mut tags = if payload.job_type() == JobType::Event {
            Self::listener_tags(&manager, payload)?
        } else {
            Self::resolve_target(manager.callbacks_for(Self::subject(payload)), payload)?
        };
        let callbacks = manager
            .callbacks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for callback in callbacks {
            append_unique(
                &mut tags,
                callback(payload).into_iter().filter(|tag| !tag.is_empty()),
            );
        }
        Ok(tags)
    }

    /// 返回底层标签目标。排队监听器的目标为传递给监听器的事件。
    pub fn target_for(payload: &Payload) -> Option<&str> {
        if payload.job_type() == JobType::Event {
            payload.event_type()
        } else {
            Some(Self::subject(payload))
        }
    }

    /// 提取底层目标的显式标签。排队监听器合并监听器与事件的解析结果。
    pub fn extract_explicit_tags(payload: &Payload) -> Result<Vec<String>> {
        let manager = TagStore::get_facade_root().map_err(|_| Error::Unavailable)?;
        if payload.job_type() == JobType::Event {
            Self::listener_tags(&manager, payload)
        } else {
            let callbacks = manager.callbacks_for(Self::subject(payload));
            Ok(Self::explicit_tags(callbacks, payload))
        }
    }

    /// 读取底层目标注册的模型标签。排队监听器以事件作为底层目标。
    pub fn model_tags_for(payload: &Payload) -> Result<Vec<String>> {
        let manager = TagStore::get_facade_root().map_err(|_| Error::Unavailable)?;
        let Some(target) = Self::target_for(payload) else {
            return Ok(Vec::new());
        };
        Self::model_tags(manager.callbacks_for(target), payload)
    }

    fn subject(payload: &Payload) -> &str {
        payload.subject_type().unwrap_or(payload.name())
    }

    fn listener_tags(manager: &TagManager, payload: &Payload) -> Result<Vec<String>> {
        let mut tags = Vec::new();
        for target in std::iter::once(Self::subject(payload)).chain(Self::target_for(payload)) {
            let resolved = Self::resolve_target(manager.callbacks_for(target), payload)?;
            append_unique(&mut tags, resolved);
        }
        Ok(tags)
    }

    fn resolve_target(callbacks: TargetCallbacks, payload: &Payload) -> Result<Vec<String>> {
        let tags = Self::explicit_tags(callbacks, payload);
        if tags.is_empty() {
            Self::model_tags(callbacks, payload)
        } else {
            Ok(tags)
        }
    }

    fn explicit_tags(callbacks: TargetCallbacks, payload: &Payload) -> Vec<String> {
        let mut tags = Vec::new();
        if let Some(callback) = callbacks.tags {
            append_unique(&mut tags, callback(payload));
        }
        tags
    }

    fn model_tags(callbacks: TargetCallbacks, payload: &Payload) -> Result<Vec<String>> {
        let mut tags = Vec::new();
        if let Some(callback) = callbacks.models {
            append_unique(&mut tags, callback(payload)?);
        }
        Ok(tags)
    }

    pub fn for_model<M: Model>(model: &M) -> Result<String> {
        let key: Cow<'_, str> = match model.get_key() {
            None | Some(Value::Null | Value::Bool(false)) => "".into(),
            Some(Value::Bool(true)) => "1".into(),
            Some(Value::Text(value)) => value.as_str().into(),
            Some(Value::Integer(value)) => value.to_string().into(),
            Some(Value::Unsigned(value)) => value.to_string().into(),
            Some(Value::Real(value)) => openrust::support::php_float_string(*value).into(),
            Some(Value::Bytes(value)) => std::str::from_utf8(value)
                .map_err(|_| Error::InvalidRecord)?
                .into(),
            _ => return Err(Error::InvalidRecord),
        };
        Ok(format!("{}:{}", std::any::type_name::<M>(), key))
    }

    /// 依模型出现顺序生成标签并去重。
    pub fn for_models<'a, M: Model + 'a>(
        models: impl IntoIterator<Item = &'a M>,
    ) -> Result<Vec<String>> {
        let mut tags = Vec::new();
        for model in models {
            append_unique(&mut tags, [Self::for_model(model)?]);
        }
        Ok(tags)
    }
}

fn append_unique(tags: &mut Vec<String>, values: impl IntoIterator<Item = String>) {
    for tag in values {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
}

impl crate::Horizon {
    pub fn tag(callback: TagCallback) -> std::result::Result<(), BindingResolutionError> {
        TagStore::get_facade_root()?
            .callbacks
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(callback);
        Ok(())
    }

    /// 为一个底层任务、事件或监听器类型登记显式标签。
    pub fn tag_for<T>(callback: TagCallback) -> std::result::Result<(), BindingResolutionError> {
        TagStore::get_facade_root()?
            .targets
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(std::any::type_name::<T>())
            .or_default()
            .tags = Some(callback);
        Ok(())
    }

    /// 登记显式标签为空时使用的模型标签回调。
    pub fn tag_models_for<T>(
        callback: ModelTagCallback,
    ) -> std::result::Result<(), BindingResolutionError> {
        TagStore::get_facade_root()?
            .targets
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(std::any::type_name::<T>())
            .or_default()
            .models = Some(callback);
        Ok(())
    }
}
