use std::{
    future::Future,
    sync::{Arc, Weak},
};

use super::{Data, RuleSet, ValidationError, ValidationFuture, ValidationResult, Validator};

fn unavailable() -> ValidationError {
    ValidationError::Execution {
        source: std::io::Error::other("验证器句柄不在当前规则作用域，或已被借用").into(),
    }
}

tokio::task_local! { static ACTIVE_VALIDATOR: Arc<()>; }

/// 只在所属规则任务内访问同一验证器；结束或取消后句柄失效。
#[derive(Clone)]
pub struct ValidatorHandle {
    source: Weak<tokio::sync::Mutex<Validator>>,
    identity: Weak<()>,
}
impl ValidatorHandle {
    fn source(&self) -> ValidationResult<Arc<tokio::sync::Mutex<Validator>>> {
        let identity = self.identity.upgrade().ok_or_else(unavailable)?;
        if !ACTIVE_VALIDATOR
            .try_with(|active| Arc::ptr_eq(active, &identity))
            .unwrap_or(false)
        {
            return Err(unavailable());
        }
        self.source.upgrade().ok_or_else(unavailable)
    }
    pub fn is_active(&self) -> bool {
        self.source().is_ok()
    }
    pub fn with<T>(
        &self,
        callback: impl FnOnce(&mut Validator) -> ValidationResult<T>,
    ) -> ValidationResult<T> {
        let mut validator = self.source()?.try_lock_owned().map_err(|_| unavailable())?;
        callback(&mut validator)
    }
    pub async fn with_async<T, F>(&self, callback: F) -> ValidationResult<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a mut Validator) -> ValidationFuture<'a, ValidationResult<T>> + Send,
    {
        let mut validator = self.source()?.try_lock_owned().map_err(|_| unavailable())?;
        callback(&mut validator).await
    }
}

// 沿用请求借用的RAII模式，异步取消时也将原验证器还给调用方。
pub(super) async fn resolving<T, F: Future<Output = T>>(
    validator: &mut Validator,
    callback: impl FnOnce(ValidatorHandle) -> F,
) -> T {
    struct Restore<'a> {
        destination: &'a mut Validator,
        source: Arc<tokio::sync::Mutex<Validator>>,
    }
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let mut original = self.source.try_lock().expect("验证器借用逃逸规则作用域");
            std::mem::swap(self.destination, &mut original);
        }
    }
    let original = std::mem::replace(
        validator,
        Validator::new(
            Data::new(serde_json::json!({})).expect("空验证数据"),
            RuleSet::new(),
            None,
            None,
            Default::default(),
            Default::default(),
        ),
    );
    let source = Arc::new(tokio::sync::Mutex::new(original));
    let identity = Arc::new(());
    let handle = ValidatorHandle {
        source: Arc::downgrade(&source),
        identity: Arc::downgrade(&identity),
    };
    let restore = Restore {
        destination: validator,
        source,
    };
    let value = ACTIVE_VALIDATOR.scope(identity, callback(handle)).await;
    drop(restore);
    value
}
