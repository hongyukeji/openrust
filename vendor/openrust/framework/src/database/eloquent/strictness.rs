use std::{any::TypeId, collections::HashMap, sync::RwLock};

use super::Model;
use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{Connection, DatabaseError, DatabaseResult},
    foundation::Application,
};

#[derive(Clone, Copy, Default)]
pub(super) struct Policy {
    pub lazy_loading: bool,
    pub discarded_attributes: bool,
    pub missing_attributes: bool,
}

#[derive(Default)]
pub(crate) struct ModelPolicies(RwLock<HashMap<TypeId, Policy>>);

pub(super) fn configure<M: Model>(
    app: &Application,
    change: impl FnOnce(&mut Policy),
) -> DatabaseResult<()> {
    let registry = app
        .container()
        .make::<ModelPolicies>()
        .map_err(DatabaseError::Unavailable)?;
    let mut policies = registry.0.write().expect("模型策略注册表锁损坏");
    change(policies.entry(TypeId::of::<M>()).or_default());
    Ok(())
}

pub(super) fn policy<M: Model>(model: &M) -> Policy {
    let Some(container) = Connection::model_container(model.state().connection.as_ref()) else {
        return Policy::default();
    };
    let Ok(registry) = container.make::<ModelPolicies>() else {
        return Policy::default();
    };
    registry
        .0
        .read()
        .expect("模型策略注册表锁损坏")
        .get(&TypeId::of::<M>())
        .copied()
        .unwrap_or_default()
}
