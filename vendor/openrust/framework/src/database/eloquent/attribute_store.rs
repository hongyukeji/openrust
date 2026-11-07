//! 模型共享原始属性与访问器管线；对应 Laravel `HasAttributes` 的访问器优先顺序。

use super::{AttributeCast, ModelMetadata};
use crate::Error;
use crate::Result;
use crate::database::Databases;
use crate::support::{ArrayKey, BoxFuture, php_loose_string_equal, php_loose_string_value_equal};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicU8, Ordering},
    },
};

/// 同步属性读取扩展；原始值与完整快照均借自锁外的独立快照。
pub trait AttributeGetter: Send + Sync {
    /// 读取转换后值；None表示不存在，不修改持久原始属性。
    fn get(&self, raw: Option<&Value>, raw_snapshot: &Map<String, Value>) -> Result<Option<Value>>;
}

/// 异步属性写入扩展；成功返回需合并的多字段patch。
pub trait AttributeSetter: Send + Sync {
    /// 在锁外转换输入；失败不提交返回patch，重叠字段或setter发生后继变化时拒绝旧补丁。
    /// 不重叠字段仍合并，不以全局版本变化拒绝正常重入。
    fn set(
        &self,
        value: Value,
        raw_snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>>;
}

#[derive(Default)]
struct State {
    raw: Map<String, Value>,
    original: Map<String, Value>,
    changes: Map<String, Value>,
    previous: Map<String, Value>,
    original_revision: u64,
    getters: BTreeMap<String, Arc<dyn AttributeGetter>>,
    setters: BTreeMap<String, Arc<dyn AttributeSetter>>,
    revision: u64,
    fields: BTreeMap<String, u64>,
    pending_count: u64,
    owners: BTreeMap<String, Arc<MutationNode>>,
    nodes: Vec<Weak<MutationNode>>,
    active_setters: u64,
    setter_versions: BTreeMap<String, u64>,
}

/// 唯一模型原始属性存储；水合绕过访问器与 hashed cast，不向外暴露锁 guard。
///
/// [`Self::new`] 将输入同时作为当前原值和 original 基线。赋值不会自动同步基线；
/// [`Self::get_dirty`] 比较稳定原始快照，显式 [`Self::sync_changes`] 发布变化及旧原值交集。
/// 这些内存状态操作不执行 SQL、模型事件或数据库提交，也不等同完整 Eloquent `save`。
#[derive(Default)]
pub struct ModelAttributes {
    state: Arc<RwLock<State>>,
}
impl fmt::Debug for ModelAttributes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ModelAttributes([REDACTED])")
    }
}
impl ModelAttributes {
    /// 从原始数据库快照创建存储，不运行setter、cast或哈希。
    pub fn new(raw: Map<String, Value>) -> Self {
        Self {
            state: Arc::new(RwLock::new(State {
                original: raw.clone(),
                raw,
                ..State::default()
            })),
        }
    }
    /// 读取单个原始值的独立副本。
    pub fn raw_attribute(&self, name: &str) -> Result<Option<Value>> {
        Ok(self
            .state
            .read()
            .map_err(|_| state_error())?
            .raw
            .get(name)
            .cloned())
    }
    /// 读取完整原始快照；不会触发getter或cast。
    pub fn raw_attributes(&self) -> Result<Map<String, Value>> {
        Ok(self.state.read().map_err(|_| state_error())?.raw.clone())
    }
    /// 原始单字段赋值；用于水合及成功持久化回写，不运行访问器。
    pub fn set_raw_attribute(&self, name: impl Into<String>, value: Value) -> Result<()> {
        let name = name.into();
        let mut state = self.state.write().map_err(|_| state_error())?;
        let revision = next_revision(&state)?;
        state.owners.remove(&name);
        state.fields.insert(name.clone(), revision);
        state.raw.insert(name, value);
        state.revision = revision;
        Ok(())
    }
    /// 替换完整原始快照，保留已注册访问器；不运行cast或哈希。
    pub fn set_raw_attributes(&self, raw: Map<String, Value>) -> Result<()> {
        let mut state = self.state.write().map_err(|_| state_error())?;
        replace_raw(&mut state, raw)?;
        Ok(())
    }
    /// 同步原子修改原始属性，用于比较并交换；仅Ok时提交候选快照。
    ///
    /// 此显式CAS闭包在写锁内运行，必须简短且不得重入同一存储（包括捕获物析构）。
    /// 不在其中运行用户访问器、异步工作或外部I/O；Err不会保留候选的部分修改。
    /// 有未决可回滚赋值时，在调用闭包前拒绝，避免无法识别裸快照的同值写入。
    pub fn update_raw_attributes<T>(
        &self,
        update: impl FnOnce(&mut Map<String, Value>) -> Result<T>,
    ) -> Result<T> {
        // 预检拒绝时保留F至guard释放；已调用FnOnce内部的析构仍须遵守锁内CAS限制。
        let mut update = Some(update);
        let mut state = self.state.write().map_err(|_| state_error())?;
        if state.pending_count != 0 {
            drop(state);
            return Err(mutation_error("model_attribute_mutation_conflict"));
        }
        let revision = match next_revision(&state) {
            Ok(revision) => revision,
            Err(error) => {
                drop(state);
                return Err(error);
            }
        };
        let mut candidate = state.raw.clone();
        let Some(update_fn) = update.take() else {
            return Err(state_error());
        };
        let result = update_fn(&mut candidate);
        match result {
            Ok(result) => {
                replace_raw_at(&mut state, candidate, revision);
                drop(state);
                Ok(result)
            }
            Err(error) => {
                drop(state);
                Err(error)
            }
        }
    }

    /// 登记自定义getter；替换实例的析构在锁外执行。
    pub fn register_getter(
        &self,
        name: impl Into<String>,
        getter: Arc<dyn AttributeGetter>,
    ) -> Result<()> {
        let name = name.into();
        let previous = {
            let mut state = self.state.write().map_err(|_| state_error())?;
            let revision = next_revision(&state)?;
            let previous = state.getters.insert(name, getter);
            state.revision = revision;
            previous
        };
        drop(previous);
        Ok(())
    }
    /// 登记自定义setter；替换实例的析构在锁外执行。
    pub fn register_setter(
        &self,
        name: impl Into<String>,
        setter: Arc<dyn AttributeSetter>,
    ) -> Result<()> {
        let name = name.into();
        let previous = {
            let mut state = self.state.write().map_err(|_| state_error())?;
            let revision = next_revision(&state)?;
            state.setter_versions.insert(name.clone(), revision);
            let previous = state.setters.insert(name, setter);
            state.revision = revision;
            previous
        };
        drop(previous);
        Ok(())
    }
    /// 根据模型声明和实际 Schema 判断字段可否批量赋值。
    ///
    /// 显式白名单优先。默认保护规则拒绝点路径和下划线开头的字段。
    pub async fn is_fillable(
        &self,
        metadata: &ModelMetadata,
        databases: &Databases,
        name: &str,
    ) -> Result<bool> {
        self.is_fillable_for_assignment(metadata, Some(databases), name)
            .await
    }

    pub(super) async fn is_fillable_for_assignment(
        &self,
        metadata: &ModelMetadata,
        databases: Option<&Databases>,
        name: &str,
    ) -> Result<bool> {
        if super::is_unguarded()
            || metadata
                .get_fillable()
                .iter()
                .any(|field| php_loose_string_equal(name, field))
        {
            return Ok(true);
        }
        Ok(!self
            .is_guarded_with_optional_database(metadata, databases, name)
            .await?
            && metadata.get_fillable().is_empty()
            && !name.contains('.')
            && !name.starts_with('_'))
    }

    /// 判断拒绝列表是否保护此字段，必要时查找模型所选连接的真实列。
    ///
    /// 临时解除赋值保护时仍按模型声明报告。已登记 setter 允许虚拟列参与判断。
    /// 空 Schema 结果放行且不缓存，查询错误向上传递。
    pub async fn is_guarded(
        &self,
        metadata: &ModelMetadata,
        databases: &Databases,
        name: &str,
    ) -> Result<bool> {
        self.is_guarded_with_optional_database(metadata, Some(databases), name)
            .await
    }

    async fn is_guarded_with_optional_database(
        &self,
        metadata: &ModelMetadata,
        databases: Option<&Databases>,
        name: &str,
    ) -> Result<bool> {
        let guarded = metadata.get_guarded();
        if guarded.is_empty() {
            return Ok(false);
        }
        // 固定 PHP 正则使用 C locale、无 Unicode 修饰符，末尾锚点也匹配最后一个 LF 前。
        if guarded == ["*"]
            || guarded.iter().any(|field| {
                field.eq_ignore_ascii_case(name)
                    || field
                        .strip_suffix('\n')
                        .is_some_and(|field| field.eq_ignore_ascii_case(name))
            })
        {
            return Ok(true);
        }
        if self
            .state
            .read()
            .map_err(|_| state_error())?
            .setters
            .contains_key(name)
        {
            return Ok(false);
        }
        metadata.check_raw_class_cast(name)?;
        let databases = databases.ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "model_mass_assignment_database_required",
                "字段保护需要当前应用的 Schema 服务",
            )
        })?;
        let columns = databases.guardable_columns(metadata).await?;
        Ok(!columns.is_empty()
            && !columns
                .iter()
                .any(|column| php_loose_string_equal(name, column)))
    }

    /// 原子读取指定字段的稳定原始值；拒绝当前尚未提交的收据值。
    pub fn committed_raw_attribute(&self, name: &str) -> Result<Option<Value>> {
        let state = self.state.read().map_err(|_| state_error())?;
        if state
            .owners
            .get(name)
            .is_some_and(|owner| owner.status.load(Ordering::Acquire) != COMMITTED)
        {
            return Err(read_conflict("model_attribute_update_pending"));
        }
        Ok(state.raw.get(name).cloned())
    }
    /// 只读取稳定快照；锁外getter/cast执行后再次核对版本，拒绝重入或并发修改。
    ///
    /// getter可能依赖任意字段，因此快照含任意未提交字段时即拒绝。
    pub fn committed_attribute(
        &self,
        metadata: &ModelMetadata,
        name: &str,
    ) -> Result<Option<Value>> {
        let (snapshot, getter, revision) = {
            let state = self.state.read().map_err(|_| state_error())?;
            if state
                .owners
                .values()
                .any(|owner| owner.status.load(Ordering::Acquire) != COMMITTED)
            {
                return Err(read_conflict("model_attribute_update_pending"));
            }
            (
                state.raw.clone(),
                state.getters.get(name).cloned(),
                state.revision,
            )
        };
        let value = if let Some(getter) = getter {
            getter.get(snapshot.get(name), &snapshot)?
        } else {
            metadata.read_cast_attribute(name, snapshot.get(name).cloned())?
        };
        if self.state.read().map_err(|_| state_error())?.revision != revision {
            return Err(read_conflict("model_attribute_read_conflict"));
        }
        Ok(value)
    }
    /// 自定义getter优先；其余读取按共同元数据cast，不改动原始快照。
    pub fn get_attribute(&self, metadata: &ModelMetadata, name: &str) -> Result<Option<Value>> {
        let (snapshot, getter) = {
            let state = self.state.read().map_err(|_| state_error())?;
            (state.raw.clone(), state.getters.get(name).cloned())
        };
        if let Some(getter) = getter {
            return getter.get(snapshot.get(name), &snapshot);
        }
        let raw = snapshot.get(name).cloned();
        metadata.read_cast_attribute(name, raw)
    }
    /// 按给定键读取当前属性，复用普通 getter 和 cast；缺项保留为 null。
    ///
    /// 重复键仍按输入顺序读取，映射保留最后一次结果。这里不应用序列化的可见性规则。
    pub fn only(&self, metadata: &ModelMetadata, names: &[&str]) -> Result<Map<String, Value>> {
        self.project_attributes(metadata, names.iter().copied())
    }

    /// 读取当前原始键集合中的属性，按 Laravel 的宽松比较排除指定名称，再应用 getter 或 cast。
    ///
    /// 访问器在锁外运行。它新增的键不会加入本次选择，后续属性读取仍观察当时的当前值。
    pub fn except(&self, metadata: &ModelMetadata, names: &[&str]) -> Result<Map<String, Value>> {
        let keys: Vec<_> = self
            .state
            .read()
            .map_err(|_| state_error())?
            .raw
            .keys()
            .cloned()
            .collect();
        let selected = keys
            .iter()
            .map(String::as_str)
            .filter(|name| match ArrayKey::from(*name) {
                ArrayKey::Integer(key) => {
                    let key = Value::from(key);
                    !names
                        .iter()
                        .any(|excluded| php_loose_string_value_equal(excluded, &key))
                }
                ArrayKey::String(_) => !names
                    .iter()
                    .any(|excluded| php_loose_string_equal(name, excluded)),
            });
        self.project_attributes(metadata, selected)
    }

    fn project_attributes<'a>(
        &self,
        metadata: &ModelMetadata,
        names: impl Iterator<Item = &'a str>,
    ) -> Result<Map<String, Value>> {
        names
            .map(|name| {
                // Laravel getAttribute 对空名称及 "0" 直接返回 null。
                let value = match name {
                    "" | "0" => Value::Null,
                    _ => self.get_attribute(metadata, name)?.unwrap_or(Value::Null),
                };
                Ok((name.to_owned(), value))
            })
            .collect()
    }

    /// 已登记getter优先，否则使用模型自身的默认访问器；两者都在锁外执行。
    ///
    /// 默认访问器与显式getter同属属性读取层，不再叠加cast；返回null也不会回退。
    pub fn get_attribute_with_default_getter(
        &self,
        _metadata: &ModelMetadata,
        name: &str,
        default: &dyn AttributeGetter,
    ) -> Result<Option<Value>> {
        let (snapshot, getter) = {
            let state = self.state.read().map_err(|_| state_error())?;
            (state.raw.clone(), state.getters.get(name).cloned())
        };
        if let Some(getter) = getter {
            getter.get(snapshot.get(name), &snapshot)
        } else {
            default.get(snapshot.get(name), &snapshot)
        }
    }
    /// 经同一getter/cast读取标量字符串；缺失与null保持None，数组和对象明确拒绝。
    pub fn get_string_attribute(
        &self,
        metadata: &ModelMetadata,
        name: &str,
    ) -> Result<Option<String>> {
        self.get_attribute(metadata, name)?
            .filter(|value| !value.is_null())
            .as_ref()
            .map(super::attribute_cast::scalar_string)
            .transpose()
    }
    /// 不提供哈希上下文的普通赋值；非null的hashed字段须使用显式统一HashManager入口。
    pub async fn set_attribute(
        &self,
        metadata: &ModelMetadata,
        name: &str,
        value: Value,
    ) -> Result<()> {
        self.set_attribute_inner(
            metadata,
            name,
            value,
            None,
            #[cfg(feature = "hashing")]
            None,
        )
        .await
    }
    /// 使用应用唯一HashManager赋值；自定义setter仍优先于hashed cast。
    #[cfg(feature = "hashing")]
    pub async fn set_attribute_with_hashing(
        &self,
        metadata: &ModelMetadata,
        name: &str,
        value: Value,
        hashing: &crate::hashing::HashManager,
    ) -> Result<()> {
        self.set_attribute_inner(metadata, name, value, None, Some(hashing))
            .await
    }
    /// 使用调用级默认setter赋值；已登记的自定义setter仍优先，失败不回退。
    ///
    /// 两种setter均在锁外运行，返回的多字段补丁不再自动cast；默认实现自行决定转换。
    /// 默认setter仅借用于本次future，不登记到模型中；取消或冲突不发布补丁，
    /// 但不撤销setter已经产生的外部副作用。相交字段或登记setter的后继变化拒绝旧补丁。
    pub async fn set_attribute_with_default_setter(
        &self,
        metadata: &ModelMetadata,
        name: &str,
        value: Value,
        default_setter: &dyn AttributeSetter,
    ) -> Result<()> {
        self.set_attribute_inner(
            metadata,
            name,
            value,
            Some(default_setter),
            #[cfg(feature = "hashing")]
            None,
        )
        .await
    }
    pub(super) async fn set_attribute_inner(
        &self,
        metadata: &ModelMetadata,
        name: &str,
        value: Value,
        default_setter: Option<&dyn AttributeSetter>,
        #[cfg(feature = "hashing")] hashing: Option<&crate::hashing::HashManager>,
    ) -> Result<()> {
        let (snapshot, setter, watch) = {
            let mut state = self.state.write().map_err(|_| state_error())?;
            state.active_setters = state
                .active_setters
                .checked_add(1)
                .ok_or_else(state_error)?;
            let setter = state.setters.get(name).cloned();
            let watch = SetterWatch {
                state: Arc::clone(&self.state),
                fields: state.fields.clone(),
                setter: setter.clone(),
                setter_revision: state.setter_versions.get(name).copied(),
                name: name.to_owned(),
            };
            (state.raw.clone(), setter, watch)
        };
        let patch = Self::prepare_patch(
            metadata,
            name,
            value,
            snapshot,
            setter,
            default_setter,
            #[cfg(feature = "hashing")]
            hashing,
        )
        .await?;
        let mut state = self.state.write().map_err(|_| state_error())?;
        let same_setter = match (&watch.setter, state.setters.get(&watch.name)) {
            (None, None) => true,
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            _ => false,
        };
        if !same_setter
            || state.setter_versions.get(name).copied() != watch.setter_revision
            || patch
                .keys()
                .any(|key| state.fields.get(key) != watch.fields.get(key))
        {
            return Err(mutation_error("model_attribute_mutation_conflict"));
        }
        apply_patch(&mut state, patch)?;
        drop(state);
        drop(watch);
        Ok(())
    }
    async fn prepare_patch(
        metadata: &ModelMetadata,
        name: &str,
        value: Value,
        snapshot: Map<String, Value>,
        setter: Option<Arc<dyn AttributeSetter>>,
        default_setter: Option<&dyn AttributeSetter>,
        #[cfg(feature = "hashing")] hashing: Option<&crate::hashing::HashManager>,
    ) -> Result<Map<String, Value>> {
        Ok(if let Some(setter) = setter {
            setter.set(value, snapshot).await?
        } else if let Some(default_setter) = default_setter {
            default_setter.set(value, snapshot).await?
        } else {
            let value = match metadata.write_cast(name)? {
                Some(AttributeCast::Array) if !value.is_null() => {
                    Value::String(super::attribute_cast::encode_array_json(&value)?)
                }
                // 固定hashed cast先旁路null，不解析HashManager或驱动配置。
                Some(AttributeCast::Hashed) if value.is_null() => Value::Null,
                Some(AttributeCast::Hashed) => {
                    #[cfg(feature = "hashing")]
                    {
                        let hashing = hashing.ok_or_else(hashing_required)?;
                        let plain = if value.is_null() {
                            None
                        } else {
                            Some(super::attribute_cast::scalar_string(&value)?)
                        };
                        super::casts::cast_attribute_as_hashed_string(hashing, plain.as_deref())
                            .await?
                            .map_or(Value::Null, Value::String)
                    }
                    #[cfg(not(feature = "hashing"))]
                    {
                        return Err(hashing_required());
                    }
                }
                _ => value,
            };
            Map::from_iter([(name.to_owned(), value)])
        })
    }
    /// 条件成立后准备同一属性补丁并返回拥有式收据；取消准备不会修改原始值。
    ///
    /// getter、条件与setter均在锁外运行；准备期间快照或访问器变化会拒绝提交。
    pub async fn set_attribute_reversibly_if<F>(
        &self,
        metadata: &ModelMetadata,
        name: &str,
        value: Value,
        predicate: F,
    ) -> Result<Option<AttributeMutation>>
    where
        F: FnOnce(Option<Value>) -> Result<bool> + Send,
    {
        self.reversible_inner(
            metadata,
            name,
            value,
            predicate,
            #[cfg(feature = "hashing")]
            None,
        )
        .await
    }

    /// 使用明确的统一哈希上下文执行可回滚条件赋值，不解析全局服务。
    #[cfg(feature = "hashing")]
    pub async fn set_attribute_reversibly_if_with_hashing<F>(
        &self,
        metadata: &ModelMetadata,
        name: &str,
        value: Value,
        predicate: F,
        hashing: &crate::hashing::HashManager,
    ) -> Result<Option<AttributeMutation>>
    where
        F: FnOnce(Option<Value>) -> Result<bool> + Send,
    {
        self.reversible_inner(metadata, name, value, predicate, Some(hashing))
            .await
    }

    async fn reversible_inner<F>(
        &self,
        metadata: &ModelMetadata,
        name: &str,
        value: Value,
        predicate: F,
        #[cfg(feature = "hashing")] hashing: Option<&crate::hashing::HashManager>,
    ) -> Result<Option<AttributeMutation>>
    where
        F: FnOnce(Option<Value>) -> Result<bool> + Send,
    {
        let MutationPreparation {
            snapshot,
            getter,
            setter,
            revision,
            receipt,
        } = self.begin_mutation(name)?;
        let current = if let Some(getter) = getter {
            getter.get(snapshot.get(name), &snapshot)?
        } else {
            metadata.read_cast_attribute(name, snapshot.get(name).cloned())?
        };
        if !predicate(current)? {
            return Ok(None);
        }
        let patch = Self::prepare_patch(
            metadata,
            name,
            value,
            snapshot,
            setter,
            None,
            #[cfg(feature = "hashing")]
            hashing,
        )
        .await?;
        self.finish_mutation(receipt, revision, patch).map(Some)
    }

    /// 仅按原始字段条件执行同步可回滚赋值，绕过所有访问器及cast。
    ///
    /// 条件在锁外运行；缺失与null保持区别。条件拒绝不修改字段，也不占用后续收据。
    pub fn set_raw_attribute_reversibly_if<F>(
        &self,
        name: &str,
        value: Value,
        predicate: F,
    ) -> Result<Option<AttributeMutation>>
    where
        F: FnOnce(Option<Value>) -> Result<bool>,
    {
        let preparation = self.begin_mutation(name)?;
        if !predicate(preparation.snapshot.get(name).cloned())? {
            return Ok(None);
        }
        self.finish_mutation(
            preparation.receipt,
            preparation.revision,
            Map::from_iter([(name.to_owned(), value)]),
        )
        .map(Some)
    }

    fn begin_mutation(&self, name: &str) -> Result<MutationPreparation> {
        let mut state = self.state.write().map_err(|_| state_error())?;
        let pending_count = state
            .pending_count
            .checked_add(1)
            .filter(|count| state.revision.checked_add(*count).is_some())
            .ok_or_else(|| mutation_error("model_attribute_revision_exhausted"))?;
        state.pending_count = pending_count;
        Ok(MutationPreparation {
            snapshot: state.raw.clone(),
            getter: state.getters.get(name).cloned(),
            setter: state.setters.get(name).cloned(),
            revision: state.revision,
            receipt: AttributeMutation {
                state: Arc::clone(&self.state),
                nodes: BTreeMap::new(),
                status: Arc::new(AtomicU8::new(PENDING)),
                applied: false,
                armed: true,
            },
        })
    }

    fn finish_mutation(
        &self,
        mut receipt: AttributeMutation,
        revision: u64,
        patch: Map<String, Value>,
    ) -> Result<AttributeMutation> {
        {
            let mut state = self.state.write().map_err(|_| state_error())?;
            if state.revision != revision {
                return Err(mutation_error("model_attribute_mutation_conflict"));
            }
            let applied_revision = next_revision(&state)?;
            for (key, value) in patch {
                let node = Arc::new(MutationNode {
                    value: value.clone(),
                    history: Mutex::new(NodeHistory {
                        original: state.raw.get(&key).cloned(),
                        predecessor: state.owners.get(&key).cloned(),
                    }),
                    status: Arc::clone(&receipt.status),
                });
                state.raw.insert(key.clone(), value);
                state.fields.insert(key.clone(), applied_revision);
                state.owners.insert(key.clone(), Arc::clone(&node));
                state.nodes.push(Arc::downgrade(&node));
                receipt.nodes.insert(key, node);
            }
            state.revision = applied_revision;
            receipt.applied = true;
            compress_histories(&mut state);
            prune_fields(&mut state);
        }
        Ok(receipt)
    }
}
struct MutationPreparation {
    snapshot: Map<String, Value>,
    getter: Option<Arc<dyn AttributeGetter>>,
    setter: Option<Arc<dyn AttributeSetter>>,
    revision: u64,
    receipt: AttributeMutation,
}

const PENDING: u8 = 0;
const COMMITTED: u8 = 1;
const CANCELLED: u8 = 2;

struct MutationNode {
    value: Value,
    history: Mutex<NodeHistory>,
    status: Arc<AtomicU8>,
}

struct NodeHistory {
    original: Option<Value>,
    predecessor: Option<Arc<MutationNode>>,
}
impl Drop for MutationNode {
    fn drop(&mut self) {
        // 最后一个Arc沿独占前驱迭代拆链，避免Rust默认递归析构耗尽栈。
        let mut predecessor = self
            .history
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .predecessor
            .take();
        while let Some(node) = predecessor {
            match Arc::try_unwrap(node) {
                Ok(mut node) => {
                    predecessor = node
                        .history
                        .get_mut()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .predecessor
                        .take();
                }
                Err(_) => break,
            }
        }
    }
}

struct SetterWatch {
    state: Arc<RwLock<State>>,
    fields: BTreeMap<String, u64>,
    setter: Option<Arc<dyn AttributeSetter>>,
    setter_revision: Option<u64>,
    name: String,
}
impl Drop for SetterWatch {
    fn drop(&mut self) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.active_setters -= 1;
        prune_fields(&mut state);
    }
}

/// 属性补丁的拥有式回滚收据；不撤销后继写入或setter的外部副作用。
///
/// 并存收据按每字段祖先链恢复；已提交祖先为稳定边界，普通字段写入断开该字段链。
/// `Drop` 仅同步恢复原始字段，不调用访问器或启动异步任务。
pub struct AttributeMutation {
    state: Arc<RwLock<State>>,
    nodes: BTreeMap<String, Arc<MutationNode>>,
    status: Arc<AtomicU8>,
    applied: bool,
    armed: bool,
}
impl fmt::Debug for AttributeMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AttributeMutation([REDACTED])")
    }
}
impl AttributeMutation {
    /// 保留已应用值并建立提交边界；不清除后继收据的字段所有权。
    pub fn commit(mut self) -> Result<()> {
        {
            let mut state = self.state.write().map_err(|_| state_error())?;
            if self.applied {
                let revision = terminal_revision(&state)?;
                self.status.store(COMMITTED, Ordering::Release);
                for (key, node) in &self.nodes {
                    if state
                        .owners
                        .get(key)
                        .is_some_and(|owner| Arc::ptr_eq(owner, node))
                    {
                        state.owners.remove(key);
                    }
                }
                state.revision = revision;
            }
            state.pending_count -= 1;
            compress_histories(&mut state);
            prune_fields(&mut state);
            self.armed = false;
        }
        Ok(())
    }
    /// 标记取消，并恢复仍由本收据直接拥有的字段；false表示有后继字段所有者。
    ///
    /// 跳过已取消祖先；仍未决祖先恢复所有权，已提交祖先只恢复稳定值。
    /// poison仅为内部回滚读取，不清除故障，普通公共访问继续返回500。
    pub fn rollback(&mut self) -> Result<bool> {
        if !self.armed {
            return Ok(true);
        }
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut complete = true;
        if self.applied {
            let revision = terminal_revision(&state)?;
            self.status.store(CANCELLED, Ordering::Release);
            for (key, node) in &self.nodes {
                if !state
                    .owners
                    .get(key)
                    .is_some_and(|owner| Arc::ptr_eq(owner, node))
                {
                    complete = false;
                    continue;
                }
                let (value, predecessor) = rollback_target(node);
                if let Some(value) = value {
                    state.raw.insert(key.clone(), value);
                } else {
                    state.raw.remove(key);
                }
                if let Some(predecessor) = predecessor {
                    state.owners.insert(key.clone(), predecessor);
                } else {
                    state.owners.remove(key);
                }
                state.fields.insert(key.clone(), revision);
            }
            state.revision = revision;
        }
        state.pending_count -= 1;
        compress_histories(&mut state);
        prune_fields(&mut state);
        self.armed = false;
        drop(state);
        // 显式rollback后的收据可能继续由调用者持有，不再保留旧属性秘密副本。
        self.nodes.clear();
        Ok(complete)
    }
}
impl Drop for AttributeMutation {
    fn drop(&mut self) {
        let _ = self.rollback();
    }
}
fn rollback_target(node: &MutationNode) -> (Option<Value>, Option<Arc<MutationNode>>) {
    compress_node(node);
    let history = node
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(predecessor) = &history.predecessor {
        (
            Some(predecessor.value.clone()),
            Some(Arc::clone(predecessor)),
        )
    } else {
        (history.original.clone(), None)
    }
}
fn compress_node(node: &MutationNode) {
    loop {
        let predecessor = node
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .predecessor
            .clone();
        let Some(predecessor) = predecessor else {
            break;
        };
        let status = predecessor.status.load(Ordering::Acquire);
        if status == PENDING {
            break;
        }
        let (original, ancestor) = if status == COMMITTED {
            (Some(predecessor.value.clone()), None)
        } else {
            let history = predecessor
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (history.original.clone(), history.predecessor.clone())
        };
        let mut history = node
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        history.original = original;
        history.predecessor = ancestor;
    }
}
fn compress_histories(state: &mut State) {
    if state.pending_count == 0 {
        // 最后收据此刻仍强持自身节点；不能用upgrade成功判定weak表仍有用途。
        state.nodes = Vec::new();
        return;
    }
    state.nodes.retain(|weak| {
        let Some(node) = weak.upgrade() else {
            return false;
        };
        compress_node(&node);
        true
    });
}
fn prune_fields(state: &mut State) {
    if state.active_setters == 0 {
        state
            .fields
            .retain(|key, _| state.raw.contains_key(key) || state.owners.contains_key(key));
    }
}
fn terminal_revision(state: &State) -> Result<u64> {
    state
        .revision
        .checked_add(1)
        .ok_or_else(|| mutation_error("model_attribute_revision_exhausted"))
}
fn next_revision(state: &State) -> Result<u64> {
    let revision = terminal_revision(state)?;
    revision
        .checked_add(state.pending_count)
        .ok_or_else(|| mutation_error("model_attribute_revision_exhausted"))?;
    Ok(revision)
}
fn apply_patch(state: &mut State, patch: Map<String, Value>) -> Result<()> {
    let revision = next_revision(state)?;
    for (key, value) in patch {
        state.owners.remove(&key);
        state.fields.insert(key.clone(), revision);
        state.raw.insert(key, value);
    }
    state.revision = revision;
    prune_fields(state);
    Ok(())
}
fn replace_raw(state: &mut State, raw: Map<String, Value>) -> Result<()> {
    let revision = next_revision(state)?;
    replace_raw_at(state, raw, revision);
    Ok(())
}
fn replace_raw_at(state: &mut State, raw: Map<String, Value>, revision: u64) {
    for key in state.raw.keys().chain(raw.keys()) {
        state.fields.insert(key.clone(), revision);
    }
    state.owners.clear();
    state.raw = raw;
    state.revision = revision;
    prune_fields(state);
}
fn read_conflict(code: &'static str) -> Error {
    Error::custom_with_status(http::StatusCode::CONFLICT, code, "模型属性当前不能稳定读取")
}
fn mutation_error(code: &'static str) -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        code,
        "模型属性变更无法完成",
    )
}

fn state_error() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "model_attribute_state_unavailable",
        "模型属性状态不可用",
    )
}
fn hashing_required() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "model_attribute_hashing_required",
        "hashed属性赋值需要应用HashManager上下文",
    )
}

#[cfg(test)]
#[path = "attribute_store_tests.rs"]
mod tests;

#[path = "attribute_state.rs"]
mod persistence_state;

#[cfg(test)]
#[path = "default_setter_tests.rs"]
mod default_setter_tests;
