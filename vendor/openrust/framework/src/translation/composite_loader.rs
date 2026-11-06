//! 来源组合只覆盖同一语言资源；语言fallback由唯一翻译入口统一处理。

use super::loader::{
    MaterializedSession, TranslationLines, TranslationLoadSession, TranslationLoader,
    TranslationResource, invalid_resource, validate_lines,
};
use crate::{Result, support::BoxFuture};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// 按声明顺序组合Loader；同一资源同键以后声明者覆盖，空字符串同样覆盖。
pub struct CompositeTranslationLoader {
    sources: Vec<Arc<dyn TranslationLoader>>,
    owner: Arc<()>,
}
impl CompositeTranslationLoader {
    /// 组合显式来源；空列表生成空快照，不偷偷要求文件或数据库。
    pub fn new(sources: Vec<Arc<dyn TranslationLoader>>) -> Self {
        Self {
            sources,
            owner: Arc::new(()),
        }
    }
}
impl TranslationLoader for CompositeTranslationLoader {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async move {
            let mut all: BTreeMap<TranslationResource, TranslationLines> = BTreeMap::new();
            let mut children = Vec::new();
            for source in &self.sources {
                let session = source.begin_snapshot().await?;
                let mut own = BTreeMap::new();
                let mut seen = BTreeSet::new();
                for resource in session.resources() {
                    if !seen.insert(resource.clone()) {
                        return Err(invalid_resource());
                    }
                    let lines = session.load(
                        &resource.locale,
                        &resource.group,
                        resource.namespace.as_deref(),
                    )?;
                    validate_lines(resource, &lines)?;
                    own.insert(resource.clone(), lines);
                }
                // 子来源先独立全量通过校验，再允许更高优先级的合法来源覆盖。
                for (resource, lines) in own {
                    all.entry(resource).or_default().extend(lines);
                }
                children.push(session);
            }
            Ok(Arc::new(CompositeSession {
                merged: MaterializedSession::new(all)?,
                children,
                owner: Arc::clone(&self.owner),
            }) as Arc<dyn TranslationLoadSession>)
        })
    }
    fn needs_refresh_since<'a>(
        &'a self,
        published: &'a dyn TranslationLoadSession,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let session = (published as &dyn std::any::Any)
                .downcast_ref::<CompositeSession>()
                .ok_or_else(invalid_resource)?;
            if !Arc::ptr_eq(&self.owner, &session.owner)
                || session.children.len() != self.sources.len()
            {
                return Err(invalid_resource());
            }
            for (source, child) in self.sources.iter().zip(&session.children) {
                if source.needs_refresh_since(child.as_ref()).await? {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }
    fn needs_refresh(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            for source in &self.sources {
                if source.needs_refresh().await? {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }
}

struct CompositeSession {
    merged: MaterializedSession,
    children: Vec<Arc<dyn TranslationLoadSession>>,
    owner: Arc<()>,
}
impl TranslationLoadSession for CompositeSession {
    fn resources(&self) -> &[TranslationResource] {
        self.merged.resources()
    }
    fn load(
        &self,
        locale: &super::Locale,
        group: &str,
        namespace: Option<&str>,
    ) -> Result<TranslationLines> {
        self.merged.load(locale, group, namespace)
    }
}

#[cfg(test)]
#[path = "composite_loader_tests.rs"]
mod tests;
