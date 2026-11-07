//! 模型实例已加载的关系值。

use super::{HydrateModel, SaveContext};
use crate::{Error, Result, database::TransactionContext};
use std::any::Any;

/// 一次批量关系加载及其子路径。
#[derive(Clone, Copy)]
pub struct RelationLoad<'a> {
    /// 当前模型的直接关系名称。
    pub name: &'a str,
    /// 在查得的子模型上继续加载的相对路径。
    pub nested: &'a [&'a str],
}

pub(super) async fn load_requested<M: HydrateModel>(
    models: &mut [M],
    paths: &[&str],
    context: &SaveContext<'_>,
    transaction: Option<&TransactionContext<'_>>,
) -> Result<()> {
    if models.is_empty() {
        return Ok(());
    }
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for &path in paths {
        if path.split('.').any(str::is_empty) {
            return Err(Error::custom(
                "model_relation_path_invalid",
                "关系路径含空名称",
            ));
        }
        let (name, nested) = path
            .split_once('.')
            .map_or((path, None), |(name, nested)| (name, Some(nested)));
        let Some((_, children)) = groups.iter_mut().find(|(candidate, _)| *candidate == name)
        else {
            groups.push((name, nested.into_iter().collect()));
            continue;
        };
        if let Some(nested) = nested
            && !children.contains(&nested)
        {
            children.push(nested);
        }
    }
    for (name, nested) in groups {
        M::load_relation(
            models,
            RelationLoad {
                name,
                nested: &nested,
            },
            context,
            transaction,
        )
        .await?;
    }
    Ok(())
}

/// 一个已经加载的关系结果。
pub enum LoadedRelation {
    /// 单值关系已加载，但没有匹配的模型。
    Null,
    /// 已加载的模型或集合，包括空集合。
    Related(Box<dyn Any + Send + Sync>),
    /// 已加载的中间表模型，包括自定义 Pivot。
    Pivot(Box<dyn Any + Send + Sync>),
}

impl LoadedRelation {
    /// 借用已加载的具体值，类型由关系声明决定。
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        match self {
            Self::Null => None,
            Self::Related(value) | Self::Pivot(value) => value.downcast_ref(),
        }
    }
}

/// 单个模型实例的关系状态，按首次加载顺序保留关系名称。
///
/// 这里仅保存已加载的结果。读取关系状态不会发起数据库查询。
#[derive(Default)]
pub struct ModelRelations {
    entries: Vec<(String, LoadedRelation)>,
}

impl ModelRelations {
    /// 返回已经加载的关系，保留已加载空值与未加载的区别。
    pub fn get_relation(&self, name: &str) -> Option<&LoadedRelation> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// 检查关系是否已加载，空值和空集合也视为已加载。
    pub fn relation_loaded(&self, name: &str) -> bool {
        self.get_relation(name).is_some()
    }

    /// 安装或替换已加载结果。替换原位保留名称顺序。
    pub fn set_relation(&mut self, name: impl Into<String>, value: LoadedRelation) {
        let name = name.into();
        if let Some((_, current)) = self.entries.iter_mut().find(|(key, _)| *key == name) {
            *current = value;
        } else {
            self.entries.push((name, value));
        }
    }

    /// 移除指定关系的加载状态。
    pub fn unset_relation(&mut self, name: &str) {
        if let Some(index) = self.entries.iter().position(|(key, _)| key == name) {
            self.entries.remove(index);
        }
    }

    /// 依次列出 refresh 应重载的关系名，保留独立的中间表模型。
    pub fn refreshable_names(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|(_, value)| !matches!(value, LoadedRelation::Pivot(_)))
            .map(|(name, _)| name.as_str())
    }
}
