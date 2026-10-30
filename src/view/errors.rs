//! `MiniJinja` 请求私有对象；只借由公开 `MessageBagContract` 分派方法。

use std::{
    fmt,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use minijinja::{Error, ErrorKind, State, Value, value::Object};

use crate::support::{ArrayKey, MessageBagContract, MessageKeys, MessageLookup, ViewErrorBag};

// 与 Collection / support::data 既有单次字段预算一致，不扩大模板参数的输入上限。
const MESSAGE_KEY_LIMIT: usize = 1024;

type SharedBag = Arc<dyn MessageBagContract>;
pub(crate) type PreparedPersistence = Box<dyn FnOnce() -> crate::Result<()> + Send>;
type Persistence = dyn Fn(
        &RequestViewErrors,
        &ArrayKey,
        &dyn MessageBagContract,
        bool,
    ) -> crate::Result<PreparedPersistence>
    + Send
    + Sync;

pub(crate) struct RequestViewErrors {
    bags: RwLock<Vec<(ArrayKey, SharedBag)>>,
    busy: AtomicBool,
    #[cfg(feature = "session")]
    identity: Arc<()>,
    persist: Option<Box<Persistence>>,
}

struct Access<'a>(&'a AtomicBool);
impl Drop for Access<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl RequestViewErrors {
    pub(crate) fn empty() -> Self {
        Self::from_bag(ViewErrorBag::new())
    }

    pub(crate) fn from_bag(bag: ViewErrorBag) -> Self {
        Self {
            bags: RwLock::new(
                bag.into_bags()
                    .into_iter()
                    .map(|(name, handle)| (name, Arc::from(handle.into_bag())))
                    .collect(),
            ),
            busy: AtomicBool::new(false),
            #[cfg(feature = "session")]
            identity: Arc::new(()),
            persist: None,
        }
    }

    #[cfg(feature = "session")]
    pub(crate) fn identity(&self) -> Arc<()> {
        Arc::clone(&self.identity)
    }

    #[cfg(feature = "session")]
    pub(crate) fn with_persistence(
        mut self,
        persist: impl Fn(
            &Self,
            &ArrayKey,
            &dyn MessageBagContract,
            bool,
        ) -> crate::Result<PreparedPersistence>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.persist = Some(Box::new(persist));
        self
    }

    fn enter(&self) -> crate::Result<Access<'_>> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| crate::Error::custom("view_errors_busy", "本请求错误包正在被访问"))?;
        Ok(Access(&self.busy))
    }

    pub(crate) fn put(
        &self,
        name: impl Into<ArrayKey>,
        bag: Box<dyn MessageBagContract>,
    ) -> crate::Result<()> {
        self.put_with_flash(name.into(), bag, false)
    }

    pub(crate) fn put_with_flash(
        &self,
        name: ArrayKey,
        bag: Box<dyn MessageBagContract>,
        flash: bool,
    ) -> crate::Result<()> {
        let _access = self.enter()?;
        let name = match name {
            ArrayKey::String(name) => ArrayKey::from(name),
            integer @ ArrayKey::Integer(_) => integer,
        };
        let bag: SharedBag = Arc::from(bag);
        let mut candidate = self.snapshot()?;
        if let Some((_, value)) = candidate.iter_mut().find(|(key, _)| key == &name) {
            *value = Arc::clone(&bag);
        } else {
            candidate.push((name.clone(), Arc::clone(&bag)));
        }
        // 自定义 to_array 在任何状态锁之外运行；完整候选预算失败不发布。
        let _ = crate::support::Collection::try_from_entries(
            candidate
                .iter()
                .map(|(key, value)| (key.clone(), value.to_array())),
        )?;
        // 持久化回调先编码，再锁定 Session；成功后只剩不失败的向量替换。
        let commit = self
            .persist
            .as_ref()
            .map(|persist| persist(self, &name, bag.as_ref(), flash))
            .transpose()?;
        let retired = {
            let mut current = self
                .bags
                .try_write()
                .map_err(|_| crate::Error::custom("view_errors_busy", "本请求错误包正在被访问"))?;
            if let Some(commit) = commit {
                commit()?;
            }
            std::mem::replace(&mut *current, candidate)
        };
        drop(retired);
        Ok(())
    }

    fn snapshot(&self) -> crate::Result<Vec<(ArrayKey, SharedBag)>> {
        self.bags
            .try_read()
            .map(|bags| bags.clone())
            .map_err(|_| crate::Error::custom("view_errors_busy", "本请求错误包正在被修改"))
    }

    fn bag(&self, name: impl Into<ArrayKey>) -> crate::Result<SharedBag> {
        let key = name.into();
        Ok(self
            .snapshot()?
            .into_iter()
            .find(|(name, _)| name == &key)
            .map_or_else(
                || Arc::new(crate::support::MessageBag::new()) as SharedBag,
                |(_, bag)| bag,
            ))
    }

    fn dispatch(
        &self,
        bag: &dyn MessageBagContract,
        method: &str,
        args: &[Value],
    ) -> std::result::Result<Value, Error> {
        let _access = self.enter().map_err(bag_error)?;
        bag_method(bag, method, args)
    }
}

pub(super) struct ViewErrors {
    bags: Arc<RequestViewErrors>,
}

impl ViewErrors {
    pub(super) fn new(bags: Arc<RequestViewErrors>) -> Self {
        Self { bags }
    }
}

impl fmt::Debug for ViewErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ViewErrors").finish_non_exhaustive()
    }
}

impl Object for ViewErrors {
    fn is_true(self: &Arc<Self>) -> bool {
        true
    }

    fn get_value_by_str(self: &Arc<Self>, key: &str) -> Option<Value> {
        self.named(key).ok()
    }

    fn call_method(
        self: &Arc<Self>,
        _state: &State<'_, '_>,
        method: &str,
        args: &[Value],
    ) -> std::result::Result<Value, Error> {
        match method {
            "any" => {
                no_args(args)?;
                // ViewErrorBag::any 调用自身 count，而命名 MessageBag::any 保留动态分派。
                let bag = self.bags.bag("default").map_err(bag_error)?;
                let _access = self.bags.enter().map_err(bag_error)?;
                Ok(Value::from(bag.count() > 0))
            }
            "getBag" => {
                let name = bag_name(args, None)?;
                Ok(self.named_value(self.bags.bag(name).map_err(bag_error)?))
            }
            "hasBag" => {
                let name = bag_name(args, Some("default"))?;
                Ok(Value::from(
                    self.bags
                        .snapshot()
                        .map_err(bag_error)?
                        .iter()
                        .any(|(key, _)| key == &name),
                ))
            }
            "getBags" => {
                no_args(args)?;
                let entries = self.bags.snapshot().map_err(bag_error)?;
                Ok(Value::from_iter(entries.into_iter().map(|(key, bag)| {
                    (key.to_string(), self.named_value(bag))
                })))
            }
            _ => self.bags.dispatch(
                self.bags.bag("default").map_err(bag_error)?.as_ref(),
                method,
                args,
            ),
        }
    }
}

impl ViewErrors {
    fn named(&self, name: &str) -> std::result::Result<Value, Error> {
        Ok(self.named_value(self.bags.bag(name).map_err(bag_error)?))
    }

    fn named_value(&self, bag: SharedBag) -> Value {
        Value::from_object(NamedBag {
            bags: Arc::clone(&self.bags),
            bag,
        })
    }
}

struct NamedBag {
    bags: Arc<RequestViewErrors>,
    bag: SharedBag,
}

impl fmt::Debug for NamedBag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("NamedBag").finish_non_exhaustive()
    }
}

impl Object for NamedBag {
    fn is_true(self: &Arc<Self>) -> bool {
        true
    }

    fn call_method(
        self: &Arc<Self>,
        _state: &State<'_, '_>,
        method: &str,
        args: &[Value],
    ) -> std::result::Result<Value, Error> {
        self.bags.dispatch(self.bag.as_ref(), method, args)
    }
}

fn bag_method(
    bag: &dyn MessageBagContract,
    method: &str,
    args: &[Value],
) -> std::result::Result<Value, Error> {
    match method {
        "count" => {
            no_args(args)?;
            Ok(Value::from(bag.count()))
        }
        "any" => {
            no_args(args)?;
            Ok(Value::from(bag.any()))
        }
        "first" => {
            let (key, format) = key_and_format(args, false)?;
            Ok(Value::from_serialize(
                bag.first(key, format).map_err(bag_error)?,
            ))
        }
        "get" => {
            let (key, format) = key_and_format(args, true)?;
            Ok(lookup(
                bag.get(key.ok_or_else(invalid_args)?, format)
                    .map_err(bag_error)?,
            ))
        }
        "has" => Ok(Value::from(
            bag.has(message_keys(args, true)?).map_err(bag_error)?,
        )),
        "hasAny" => Ok(Value::from(
            bag.has_any(message_keys(args, false)?).map_err(bag_error)?,
        )),
        "missing" => Ok(Value::from(
            bag.missing(message_keys(args, true)?).map_err(bag_error)?,
        )),
        "isEmpty" => {
            no_args(args)?;
            Ok(Value::from(bag.is_empty()))
        }
        "isNotEmpty" => {
            no_args(args)?;
            Ok(Value::from(bag.is_not_empty()))
        }
        "format" | "getFormat" => {
            no_args(args)?;
            Ok(Value::from(bag.format().to_owned()))
        }
        "all" => {
            let format = optional_key(args)?;
            Ok(Value::from_serialize(bag.all(format).map_err(bag_error)?))
        }
        "unique" => {
            let format = optional_key(args)?;
            Ok(Value::from_serialize(
                bag.unique(format).map_err(bag_error)?,
            ))
        }
        "keys" => {
            no_args(args)?;
            Ok(Value::from_iter(bag.keys().into_iter().map(
                |key| match key {
                    crate::support::ArrayKey::Integer(key) => Value::from(key),
                    crate::support::ArrayKey::String(key) => Value::from(key),
                },
            )))
        }
        "toArray" => {
            no_args(args)?;
            Ok(Value::from_serialize(bag.to_array()))
        }
        _ => Err(ErrorKind::UnknownMethod.into()),
    }
}

fn lookup(found: MessageLookup) -> Value {
    match found {
        MessageLookup::Exact(values) => Value::from_serialize(values),
        MessageLookup::Wildcard(values) => Value::from_serialize(values),
        MessageLookup::Missing => Value::from_serialize(Vec::<String>::new()),
    }
}

fn message_keys(args: &[Value], required: bool) -> std::result::Result<MessageKeys, Error> {
    let Some(first) = args.first() else {
        return if required {
            Err(invalid_args())
        } else {
            Ok(MessageKeys::Keys(Vec::new()))
        };
    };
    if first.is_none() {
        return Ok(MessageKeys::Any);
    }
    // 固定上游以首参数是否为数组决定来源；数组后的额外位置参数不参与选择。
    let values = match first.kind() {
        minijinja::value::ValueKind::Seq => first
            .try_iter()?
            .take(MESSAGE_KEY_LIMIT + 1)
            .collect::<Vec<_>>(),
        minijinja::value::ValueKind::Map => first
            .try_iter()?
            .take(MESSAGE_KEY_LIMIT + 1)
            .map(|key| first.get_item(&key))
            .collect::<std::result::Result<Vec<_>, _>>()?,
        _ => {
            if args.len() > MESSAGE_KEY_LIMIT {
                return Err(invalid_args());
            }
            args.to_vec()
        }
    };
    if values.len() > MESSAGE_KEY_LIMIT {
        return Err(invalid_args());
    }
    let keys = values
        .iter()
        .map(|key| {
            key.as_str()
                .map(ArrayKey::from)
                .or_else(|| key.as_i64().map(ArrayKey::from))
                .ok_or_else(invalid_args)
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(MessageKeys::Keys(keys))
}

fn nullable_string(value: &Value) -> std::result::Result<Option<&str>, Error> {
    if value.is_none() {
        Ok(None)
    } else {
        value.as_str().map(Some).ok_or_else(invalid_args)
    }
}

fn key_and_format(
    args: &[Value],
    required: bool,
) -> std::result::Result<(Option<&str>, Option<&str>), Error> {
    let (key, format) = match args {
        [] if !required => (None, None),
        [key] => (nullable_string(key)?, None),
        [key, format] => (nullable_string(key)?, nullable_string(format)?),
        _ => return Err(invalid_args()),
    };
    // 固定PHP8.5的get(null)以空字符串键查询；保留key省略报错，不模拟PHP弃用警告。
    Ok((
        if required {
            Some(key.unwrap_or(""))
        } else {
            key
        },
        format,
    ))
}

fn bag_name(args: &[Value], default: Option<&str>) -> std::result::Result<ArrayKey, Error> {
    match args {
        [] => default.map(ArrayKey::from).ok_or_else(invalid_args),
        [name] if name.is_none() => Ok(ArrayKey::from("")),
        [name] => name
            .as_str()
            .map(ArrayKey::from)
            .or_else(|| name.as_i64().map(ArrayKey::from))
            .ok_or_else(invalid_args),
        _ => Err(invalid_args()),
    }
}

fn optional_key(args: &[Value]) -> std::result::Result<Option<&str>, Error> {
    match args {
        [] => Ok(None),
        [key] => nullable_string(key),
        _ => Err(invalid_args()),
    }
}

fn no_args(args: &[Value]) -> std::result::Result<(), Error> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(invalid_args())
    }
}

fn invalid_args() -> Error {
    Error::new(ErrorKind::InvalidOperation, "视图错误包方法参数无效")
}

fn bag_error(error: crate::Error) -> Error {
    Error::new(ErrorKind::InvalidOperation, "视图错误包方法失败").with_source(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::{Collection, MessageBag, MessageValue};
    use std::sync::{
        Weak,
        atomic::{AtomicBool, Ordering},
    };

    struct CustomBag {
        inner: MessageBag,
        owner: Weak<RequestViewErrors>,
        reentry_rejected: Arc<AtomicBool>,
        #[cfg(feature = "session")]
        session: Option<crate::session::Session>,
    }

    impl CustomBag {
        fn new() -> Self {
            Self {
                inner: MessageBag::new(),
                owner: Weak::new(),
                reentry_rejected: Arc::new(AtomicBool::new(false)),
                #[cfg(feature = "session")]
                session: None,
            }
        }
    }

    impl MessageBagContract for CustomBag {
        fn keys(&self) -> Vec<ArrayKey> {
            vec!["custom-key".into()]
        }
        fn messages(&self) -> &Collection<Collection<MessageValue>> {
            self.inner.messages()
        }
        fn to_array(&self) -> &Collection<Collection<MessageValue>> {
            self.inner.to_array()
        }
        fn format(&self) -> &'static str {
            "custom-format"
        }
        fn set_format(&mut self, format: String) {
            self.inner.set_format(format);
        }
        fn add(&mut self, key: ArrayKey, message: String) -> crate::Result<()> {
            self.inner.add(key, message)
        }
        fn merge(&mut self, messages: &Collection<Collection<MessageValue>>) -> crate::Result<()> {
            self.inner.merge(messages)
        }
        fn has(&self, _: MessageKeys) -> crate::Result<bool> {
            Ok(true)
        }
        fn has_any(&self, _: MessageKeys) -> crate::Result<bool> {
            Ok(false)
        }
        fn missing(&self, _: MessageKeys) -> crate::Result<bool> {
            Ok(false)
        }
        fn first(&self, _: Option<&str>, _: Option<&str>) -> crate::Result<Option<String>> {
            Ok(Some("custom-first".into()))
        }
        fn get(&self, _: &str, _: Option<&str>) -> crate::Result<MessageLookup> {
            Ok(MessageLookup::Exact(Collection::try_from_entries([(
                0.into(),
                "custom-get".into(),
            )])?))
        }
        fn all(&self, _: Option<&str>) -> crate::Result<Collection<MessageValue>> {
            Collection::try_from_entries([(0.into(), "custom-all".into())])
        }
        fn unique(&self, _: Option<&str>) -> crate::Result<Collection<MessageValue>> {
            Collection::try_from_entries([(0.into(), "custom-unique".into())])
        }
        fn forget(&mut self, key: &ArrayKey) {
            self.inner.forget(key.clone());
        }
        fn count(&self) -> usize {
            if let Some(owner) = self.owner.upgrade() {
                self.reentry_rejected.store(
                    owner.put("reentered", Box::new(MessageBag::new())).is_err(),
                    Ordering::SeqCst,
                );
            }
            73
        }
        fn any(&self) -> bool {
            false
        }
        fn is_empty(&self) -> bool {
            false
        }
        fn is_not_empty(&self) -> bool {
            true
        }
    }

    fn render(bags: Arc<RequestViewErrors>, template: &str) -> crate::Result<String> {
        let env = minijinja::Environment::new();
        env.render_str(
            template,
            minijinja::context! { errors => Value::from_object(ViewErrors::new(bags)) },
        )
        .map_err(crate::Error::internal)
    }

    #[test]
    fn methods_dispatch_to_custom_contract_and_reentrant_mutation_is_rejected() -> crate::Result<()>
    {
        let bags = Arc::new(RequestViewErrors::empty());
        let mut custom = CustomBag::new();
        custom.inner.add("raw", "custom-array")?;
        custom.owner = Arc::downgrade(&bags);
        let rejected = Arc::clone(&custom.reentry_rejected);
        bags.put("default", Box::new(custom))?;
        bags.put("other", Box::new(CustomBag::new()))?;
        assert_eq!(
            render(
                Arc::clone(&bags),
                "{{ errors.count() }}|{{ errors.any() }}|{{ errors.first('x') }}|{{ errors.all()[0] }}|{{ errors.getBag('other').get('x')[0] }}|{{ errors.getBag('default').toArray().raw[0] }}|{{ errors.keys()[0] }}|{{ errors.format() }}|{{ errors.has('x') }}|{{ errors.hasAny('x') }}|{{ errors.missing('x') }}|{{ errors.isEmpty() }}|{{ errors.isNotEmpty() }}|{{ errors.unique()[0] }}"
            )?,
            "73|True|custom-first|custom-all|custom-get|custom-array|custom-key|custom-format|True|False|False|False|True|custom-unique"
        );
        assert!(rejected.load(Ordering::SeqCst));
        assert_eq!(
            render(
                Arc::clone(&bags),
                "{{ errors.hasBag('reentered') }}|{{ errors.getBag('missing').first('x') }}|{{ errors.getBags().other.count() }}|{{ errors.getBag('other').any() }}"
            )?,
            "False||73|False"
        );
        assert!(render(Arc::clone(&bags), "{{ errors.count('unexpected') }}").is_err());
        assert!(render(bags, "{{ errors.unknown() }}").is_err());
        Ok(())
    }

    #[test]
    fn named_only_and_failed_budget_do_not_replace_default_contract() -> crate::Result<()> {
        let bags = Arc::new(RequestViewErrors::empty());
        bags.put("other", Box::new(CustomBag::new()))?;
        assert_eq!(
            render(
                Arc::clone(&bags),
                "{{ errors.count() }}|{{ errors.any() }}|{{ errors.first('email') }}|{{ errors.hasBag('other') }}"
            )?,
            "0|False||True"
        );
        bags.put("default", Box::new(CustomBag::new()))?;
        let mut full = MessageBag::new();
        for index in 0..512 {
            full.add(index.to_string(), "message")?;
        }
        assert!(bags.put("default", Box::new(full)).is_err());
        assert_eq!(render(bags, "{{ errors.count() }}")?, "73");
        Ok(())
    }

    #[cfg(feature = "session")]
    #[test]
    fn restored_codec_remains_a_template_contract_object() -> crate::Result<()> {
        let mut codecs = crate::session::ErrorBagCodecs::new();
        codecs.register::<CustomBag>(
            "view.custom",
            1,
            |_| Ok(serde_json::json!({})),
            |_| Ok(CustomBag::new()),
        )?;
        let session = crate::session::Session::with_options_and_codecs(
            None,
            Arc::new(crate::session::MemorySessionStore::default()),
            None,
            crate::session::DEFAULT_MAX_BYTES,
            "view_session".into(),
            Arc::new(codecs),
        )?;
        session.flash_errors("default", &CustomBag::new())?;
        session.flash_errors("other", &CustomBag::new())?;
        let bags = Arc::new(RequestViewErrors::from_bag(session.read_errors()?));
        assert_eq!(
            render(
                bags,
                "{{ errors.count() }}|{{ errors.any() }}|{{ errors.all()[0] }}|{{ errors.getBag('other').first('email') }}"
            )?,
            "73|True|custom-all|custom-first"
        );
        Ok(())
    }
    #[cfg(feature = "session")]
    #[tokio::test]
    async fn session_identity_mutations_replacements_and_fallback_follow_fixed_oracle()
    -> crate::Result<()> {
        use crate::session::{MemorySessionStore, Session};
        let store = Arc::new(MemorySessionStore::default());
        let session = Session::new(None, store.clone(), None)?;
        session.start().await?;
        let mut initial = MessageBag::new();
        initial.add("email", "original")?;
        session.flash_errors("default", &initial)?;
        // 持久属性用于验证保存关联；flash寿命另有独立负例。
        session.forget(&["_flash"])?;
        let shared = session.shared_view_errors()?;
        assert!(Arc::ptr_eq(&shared, &session.shared_view_errors()?));
        let mut named = MessageBag::new();
        named.add("email", "named")?;
        shared.put("other", Box::new(named))?;
        assert!(session.read_errors()?.has_bag("other"));
        assert_eq!(
            render(
                Arc::clone(&shared),
                "{{ errors.first('email') }}|{{ errors.getBag('other').first('email') }}"
            )?,
            "original|named"
        );
        session.save().await?;
        let restored = Session::new(Some(session.id()?), store.clone(), None)?;
        restored.start().await?;
        assert!(restored.read_errors()?.has_bag("other"));

        let replacement = Session::new(None, store.clone(), None)?;
        replacement.flash_errors("replacement", &MessageBag::new())?;
        session.put("errors", replacement.get("errors")?)?;
        shared.put("old-only", Box::new(MessageBag::new()))?;
        assert!(session.read_errors()?.has_bag("replacement"));
        assert!(!session.read_errors()?.has_bag("old-only"));
        assert_eq!(
            render(
                Arc::clone(&shared),
                "{{ errors.hasBag('replacement') }}|{{ errors.hasBag('old-only') }}"
            )?,
            "False|True"
        );
        let current = session.shared_view_errors()?;
        assert!(!Arc::ptr_eq(&shared, &current));
        // 值完全相同的整包赋值仍产生新对象，旧句柄不能重新写入Session。
        let identical = session.get("errors")?;
        session.put("errors", identical)?;
        current.put("same-value-detached", Box::new(MessageBag::new()))?;
        assert!(!session.read_errors()?.has_bag("same-value-detached"));

        let missing = Session::new(None, store, None)?;
        let fallback = missing.shared_view_errors()?;
        fallback.put("fallback-only", Box::new(MessageBag::new()))?;
        assert_eq!(missing.get("errors")?, None);
        assert_eq!(
            render(
                fallback,
                "{{ errors.hasBag('fallback-only') }}|{{ errors.count() }}"
            )?,
            "True|0"
        );
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn shared_put_preserves_flash_age_and_explicit_flash_updates_current_object()
    -> crate::Result<()> {
        use crate::session::{MemorySessionStore, Session};
        let store = Arc::new(MemorySessionStore::default());
        let first = Session::new(None, store.clone(), None)?;
        first.start().await?;
        first.flash_errors("default", &MessageBag::new())?;
        first.save().await?;
        let next = Session::new(Some(first.id()?), store.clone(), None)?;
        next.start().await?;
        let shared = next.shared_view_errors()?;
        let flash_before = next.get("_flash")?;
        shared.put("ordinary-put", Box::new(MessageBag::new()))?;
        assert_eq!(next.get("_flash")?, flash_before);
        next.save().await?;
        assert_eq!(
            next.get("errors")?,
            None,
            "普通对象put不能延长已经old的flash"
        );
        let third = Session::new(Some(first.id()?), store, None)?;
        third.start().await?;
        assert!(!third.read_errors()?.has_bag("ordinary-put"));
        shared.put("after-age", Box::new(MessageBag::new()))?;
        assert_eq!(next.get("errors")?, None, "保存老化移除属性后旧对象已断开");

        let current = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
        current.flash_errors("default", &MessageBag::new())?;
        let shared = current.shared_view_errors()?;
        let mut new_bag = MessageBag::new();
        new_bag.add("email", "flashed")?;
        current.flash_errors("other", &new_bag)?;
        assert!(Arc::ptr_eq(&shared, &current.shared_view_errors()?));
        assert_eq!(
            render(shared, "{{ errors.getBag('other').first('email') }}")?,
            "flashed"
        );
        Ok(())
    }

    #[cfg(feature = "session")]
    #[test]
    fn attached_budget_and_codec_failure_preserve_both_objects() -> crate::Result<()> {
        use crate::session::{MemorySessionStore, Session};
        let session = Session::with_options(
            None,
            Arc::new(MemorySessionStore::default()),
            None,
            1024,
            "view_session".into(),
        )?;
        session.flash_errors("default", &MessageBag::new())?;
        let shared = session.shared_view_errors()?;
        let before = session.all()?;
        let mut huge = MessageBag::new();
        huge.add("email", "x".repeat(2048))?;
        let error = shared
            .put("default", Box::new(huge))
            .err()
            .ok_or_else(|| crate::Error::custom("test_missing_error", "预算必须拒绝写入"))?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(session.all()?, before);
        assert_eq!(render(Arc::clone(&shared), "{{ errors.count() }}")?, "0");
        assert!(
            shared
                .put("unregistered", Box::new(CustomBag::new()))
                .is_err()
        );
        assert_eq!(session.all()?, before);
        assert_eq!(
            render(
                shared,
                "{{ errors.hasBag('unregistered') }}|{{ errors.count() }}"
            )?,
            "False|0"
        );
        Ok(())
    }
    #[cfg(feature = "session")]
    thread_local! {
        static DECODER_SESSION: std::cell::RefCell<Option<(crate::session::Session, Arc<AtomicBool>)>> = const { std::cell::RefCell::new(None) };
    }

    #[cfg(feature = "session")]
    #[test]
    fn codec_callbacks_can_read_session_and_recursive_mutation_or_restore_fails_fast()
    -> crate::Result<()> {
        use crate::session::{ErrorBagCodecs, MemorySessionStore, Session};
        fn encode(bag: &CustomBag) -> crate::Result<serde_json::Value> {
            if let Some(session) = &bag.session {
                let _ = session.get("errors")?;
            }
            if let Some(owner) = bag.owner.upgrade() {
                bag.reentry_rejected.store(
                    owner
                        .put("recursive-encode", Box::new(MessageBag::new()))
                        .is_err(),
                    Ordering::SeqCst,
                );
            }
            Ok(serde_json::json!({}))
        }
        fn decode(_: serde_json::Value) -> crate::Result<CustomBag> {
            DECODER_SESSION.with(|slot| -> crate::Result<()> {
                if let Some((session, rejected)) = slot.borrow().as_ref() {
                    let _ = session.get("errors")?;
                    rejected.store(session.shared_view_errors().is_err(), Ordering::SeqCst);
                }
                Ok(())
            })?;
            Ok(CustomBag::new())
        }
        let mut codecs = ErrorBagCodecs::new();
        codecs.register::<CustomBag>("view.reentrant", 1, encode, decode)?;
        let session = Session::with_options_and_codecs(
            None,
            Arc::new(MemorySessionStore::default()),
            None,
            crate::session::DEFAULT_MAX_BYTES,
            "view_session".into(),
            Arc::new(codecs),
        )?;
        session.flash_errors("default", &MessageBag::new())?;
        let shared = session.shared_view_errors()?;
        let mut custom = CustomBag::new();
        custom.session = Some(session.clone());
        custom.owner = Arc::downgrade(&shared);
        let encode_rejected = Arc::clone(&custom.reentry_rejected);
        shared.put("default", Box::new(custom))?;
        assert!(encode_rejected.load(Ordering::SeqCst));
        assert!(!session.read_errors()?.has_bag("recursive-encode"));
        let raw = session.get("errors")?;
        session.put("errors", raw)?;
        let restore_rejected = Arc::new(AtomicBool::new(false));
        DECODER_SESSION.with(|slot| {
            *slot.borrow_mut() = Some((session.clone(), Arc::clone(&restore_rejected)));
        });
        let result = session.shared_view_errors();
        DECODER_SESSION.with(|slot| {
            slot.borrow_mut().take();
        });
        let restored = result?;
        assert!(restore_rejected.load(Ordering::SeqCst));
        assert_eq!(
            render(restored, "{{ errors.count() }}|{{ errors.any() }}")?,
            "73|True"
        );
        Ok(())
    }

    #[test]
    fn captured_named_bag_retains_original_contract_after_replacement() -> crate::Result<()> {
        let shared = Arc::new(RequestViewErrors::empty());
        shared.put("default", Box::new(CustomBag::new()))?;
        let object = ViewErrors::new(Arc::clone(&shared));
        let captured = object.named("default").map_err(crate::Error::internal)?;
        shared.put("default", Box::new(MessageBag::new()))?;
        let env = minijinja::Environment::new();
        let rendered = env
            .render_str(
                "{{ old.count() }}|{{ errors.count() }}",
                minijinja::context! { old => captured, errors => Value::from_object(object) },
            )
            .map_err(crate::Error::internal)?;
        assert_eq!(rendered, "73|0");
        Ok(())
    }
    #[test]
    fn key_argument_matrix_preserves_php_defaults_arrays_varargs_and_dispatch() -> crate::Result<()>
    {
        let mut one = MessageBag::new();
        one.add("a", "one")?;
        let mut many = one.clone();
        many.add("b", "dup")?;
        many.add("c", "dup")?;
        many.add("empty", "")?;
        let cases = [
            ("has", "none", [false, true, true]),
            ("has", "[]", [false, true, true]),
            ("has", "'a'", [false, true, true]),
            ("has", "['a','b']", [false, false, true]),
            ("has", "['a','missing']", [false, false, false]),
            ("has", "'a','b'", [false, false, true]),
            ("has", "['a'],'missing'", [false, true, true]),
            ("has", "none,'missing'", [false, true, true]),
            ("has", "{'alias':'a'},'missing'", [false, true, true]),
            ("has", "'empty'", [false, false, false]),
            ("hasAny", "", [false, false, false]),
            ("hasAny", "none", [false, true, true]),
            ("hasAny", "[]", [false, false, false]),
            ("hasAny", "['missing','a']", [false, true, true]),
            ("hasAny", "'missing','a'", [false, true, true]),
            ("hasAny", "['missing'],'a'", [false, false, false]),
            ("hasAny", "{'a':'missing'},'a'", [false, false, false]),
            ("missing", "none", [true, false, false]),
            ("missing", "[]", [true, true, true]),
            ("missing", "['a','missing']", [true, false, false]),
            ("missing", "'missing','a'", [true, false, false]),
            ("missing", "['missing'],'a'", [true, true, true]),
            ("missing", "{'a':'missing'}", [true, true, true]),
        ];
        for (index, bag) in [MessageBag::new(), one, many].into_iter().enumerate() {
            let shared = Arc::new(RequestViewErrors::empty());
            shared.put("default", Box::new(bag.clone()))?;
            shared.put("other", Box::new(bag))?;
            for receiver in ["errors", "errors.getBag('other')"] {
                for (method, args, expected) in &cases {
                    let template = format!(
                        "{{% if {receiver}.{method}({args}) %}}1{{% else %}}0{{% endif %}}"
                    );
                    assert_eq!(
                        render(Arc::clone(&shared), &template)?,
                        if expected[index] { "1" } else { "0" },
                        "{receiver}.{method}({args}), dataset {index}"
                    );
                }
                for method in ["has", "missing", "get"] {
                    assert!(
                        render(
                            Arc::clone(&shared),
                            &format!("{{{{ {receiver}.{method}() }}}}")
                        )
                        .is_err(),
                        "{method} 的必需首参不能省略"
                    );
                }
            }
        }
        let custom = Arc::new(RequestViewErrors::empty());
        custom.put("default", Box::new(CustomBag::new()))?;
        // 空内置包的has([])=false、missing([])=true；自定义覆盖仍是true/false。
        assert_eq!(
            render(
                Arc::clone(&custom),
                "{% if errors.has([]) %}1{% endif %}|{% if errors.missing([]) %}1{% endif %}|{{ errors.first(none,none) }}|{{ errors.get('a',none)[0] }}|{{ errors.all(none)[0] }}|{{ errors.unique(none)[0] }}"
            )?,
            "1||custom-first|custom-get|custom-all|custom-unique"
        );
        let oversized = Value::from_serialize(vec!["a"; MESSAGE_KEY_LIMIT + 1]);
        assert!(message_keys(&[oversized], true).is_err());
        assert_eq!(render(custom, "{{ errors.count() }}")?, "73");
        Ok(())
    }

    #[test]
    fn nullable_format_arguments_keep_current_format_and_unique_original_keys() -> crate::Result<()>
    {
        let mut bag = MessageBag::new();
        bag.add("a", "one")?;
        bag.add("b", "dup")?;
        bag.add("c", "dup")?;
        bag.add("empty", "")?;
        bag.set_format("<:key>:message");
        let shared = Arc::new(RequestViewErrors::empty());
        shared.put("default", Box::new(bag.clone()))?;
        let cases = [
            (
                "{{ errors.first() }}|{{ errors.first(none) }}|{{ errors.first(none,none) }}",
                "<a>one|<a>one|<a>one",
            ),
            (
                "{{ errors.first('a',none) }}|{{ errors.get('a',none)[0] }}",
                "<a>one|<a>one",
            ),
            (
                "{{ errors.get(none)|length }}|{{ errors.get(none,none)|length }}",
                "0|0",
            ),
            (
                "{% for value in errors.all(none) %}[{{ value }}]{% endfor %}",
                "[<a>one][<b>dup][<c>dup][<empty>]",
            ),
            (
                "{% for value in errors.unique(none) %}[{{ value }}]{% endfor %}",
                "[<a>one][<b>dup][<c>dup][<empty>]",
            ),
            (
                "{{ errors.format() }}|{{ errors.getFormat() }}",
                "<:key>:message|<:key>:message",
            ),
        ];
        for (template, expected) in cases {
            assert_eq!(render(Arc::clone(&shared), template)?, expected);
        }
        bag.set_format(":message");
        shared.put("default", Box::new(bag.clone()))?;
        assert_eq!(
            render(Arc::clone(&shared), "{{ errors.unique(none)|join(',') }}")?,
            "0,1,3"
        );
        assert_eq!(
            render(
                Arc::clone(&shared),
                "{% for value in errors.unique(':key=:message') %}[{{ value }}]{% endfor %}"
            )?,
            "[a=one][b=dup][c=dup][empty=]"
        );
        bag.add("", "blank-key")?;
        shared.put("default", Box::new(bag))?;
        assert_eq!(
            render(Arc::clone(&shared), "{{ errors.get(none,none)[0] }}")?,
            "blank-key"
        );
        shared.put("", Box::new(CustomBag::new()))?;
        shared.put(7, Box::new(CustomBag::new()))?;
        assert_eq!(
            render(
                shared,
                "{{ errors.getBag(none).first(none) }}|{{ errors.getBag(7).first(none) }}"
            )?,
            "custom-first|custom-first"
        );
        Ok(())
    }
}
