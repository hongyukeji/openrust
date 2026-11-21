use std::any::TypeId;

use crate::{contracts::auth::Authenticatable, database::eloquent::Model};

use super::{AuthError, AuthManager, AuthResult};

/// Compiled counterpart of a PHP model class reference. No application state is shared.
#[doc(hidden)]
pub struct ModelClass {
    name: &'static str,
    type_id: fn() -> TypeId,
    rust_name: fn() -> &'static str,
    register: fn(&AuthManager, &'static str),
    tokens: bool,
}

inventory::collect!(ModelClass);

impl ModelClass {
    pub const fn new<M: Model + Authenticatable>(name: &'static str) -> Self {
        Self {
            name,
            type_id: TypeId::of::<M>,
            rust_name: std::any::type_name::<M>,
            register: |auth, name| auth.registerModel::<M>(name),
            tokens: false,
        }
    }

    #[cfg(feature = "sanctum")]
    pub const fn sanctum<M: crate::sanctum::HasApiTokens>(name: &'static str) -> Self {
        Self {
            register: |auth, name| auth.registerSanctumModel::<M>(name),
            tokens: true,
            ..Self::new::<M>(name)
        }
    }

    pub fn name(&self) -> &'static str {
        self.name.strip_prefix("crate::").unwrap_or(self.name)
    }

    pub(crate) fn named(name: &str) -> AuthResult<Option<&'static Self>> {
        let mut found: Option<&Self> = None;
        for model in inventory::iter::<Self> {
            if model.name() != name && (model.rust_name)() != name {
                continue;
            }
            if let Some(previous) = found {
                if (previous.type_id)() != (model.type_id)() {
                    return Err(AuthError::Configuration("ambiguous Eloquent model class"));
                }
                if previous.tokens {
                    continue;
                }
            }
            found = Some(model);
        }
        Ok(found)
    }

    pub(crate) fn for_type(id: TypeId) -> Option<&'static Self> {
        inventory::iter::<Self>
            .into_iter()
            .filter(|model| (model.type_id)() == id)
            .filter(|model| Self::named(model.name()).is_ok())
            .max_by_key(|model| model.tokens)
    }

    pub(crate) fn register(&self, auth: &AuthManager) {
        (self.register)(auth, self.name());
    }
}

/// References an Eloquent authentication model from configuration.
///
/// Like PHP's `User::class`, the value is a class name. The compiled factory is
/// retained by the linker even when cached configuration bypasses the loader.
/// An explicit name is useful for public aliases or models from other crates.
#[doc(hidden)]
#[macro_export]
macro_rules! __auth_model {
    ($model:path) => {
        $crate::__auth_model!(@collect $model, stringify!($model), new)
    };
    ($model:path, sanctum) => {
        $crate::__auth_model!(@collect $model, stringify!($model), sanctum)
    };
    ($model:ty, $name:literal) => {
        $crate::__auth_model!(@collect $model, $name, new)
    };
    ($model:ty, $name:literal, sanctum) => {
        $crate::__auth_model!(@collect $model, $name, sanctum)
    };
    (@collect $model:ty, $name:expr, $constructor:ident) => {{
        $crate::auth::__inventory::submit! {
            $crate::auth::__ModelClass::$constructor::<$model>($name)
        }
        $crate::auth::__ModelClass::$constructor::<$model>($name).name()
    }};
}
