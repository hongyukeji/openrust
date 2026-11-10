//! Variadic constructor selection and inline primitive contextual values.
use super::{Binding, Container, ParameterValue, RegisteredBinding, ServiceKey};
use crate::contracts::container::{BindingResolutionError, Container as _};
use std::{
    any::{TypeId, type_name},
    sync::Arc,
};
type Result<T> = std::result::Result<T, BindingResolutionError>;
impl Container {
    fn contextual_argument(&self, key: &ServiceKey) -> Result<Option<RegisteredBinding>> {
        let Some(consumer) = self.consumer() else {
            return Ok(None);
        };
        let key = self.getAlias(key.clone())?;
        Ok(self
            .registry
            .read()
            .expect("container lock poisoned")
            .contextual_binding(self, &consumer, &key))
    }
    fn inline_argument<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        entry: &RegisteredBinding,
    ) -> Result<Arc<T>> {
        match entry.value.downcast_ref::<Binding<T>>() {
            Some(Binding::Factory(factory)) => factory(self),
            Some(Binding::AsyncFactory(_)) => {
                Err(BindingResolutionError::AsyncResolutionRequired { service: key })
            }
            _ => Err(BindingResolutionError::TypeMismatch {
                service: key,
                expected: type_name::<T>(),
                actual: entry.output,
            }),
        }
    }
    async fn inline_argument_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        entry: &RegisteredBinding,
    ) -> Result<Arc<T>> {
        match entry.value.downcast_ref::<Binding<T>>() {
            Some(Binding::Factory(factory)) => factory(self),
            Some(Binding::AsyncFactory(factory)) => factory(self).await,
            _ => Err(BindingResolutionError::TypeMismatch {
                service: key,
                expected: type_name::<T>(),
                actual: entry.output,
            }),
        }
    }
    pub(super) fn contextual_services<T: ?Sized + Send + Sync + 'static>(
        &self,
        consumer: ServiceKey,
        need: ServiceKey,
        services: Vec<ServiceKey>,
    ) -> Result<()> {
        let need = self.getAlias(need)?;
        let services = Arc::new(services);
        let keys = services.clone();
        let mut entry = Self::entry(Binding::Factory(Arc::new(move |c: &Container| {
            keys.iter()
                .map(|key| c.get::<T>(key.clone()))
                .collect::<Result<Vec<_>>>()
                .map(Arc::new)
        })));
        entry.services = Some(services);
        let retired = self
            .registry
            .write()
            .expect("container lock poisoned")
            .contextual
            .entry(consumer)
            .or_default()
            .insert(need, entry);
        drop(retired);
        Ok(())
    }
}
pub(super) fn primitive<T: Send + Sync + 'static>(
    c: &Container,
    key: ServiceKey,
) -> Result<ParameterValue> {
    let entry = c
        .contextual_argument(&key)?
        .ok_or_else(|| BindingResolutionError::NotBound {
            service: key.clone(),
        })?;
    if TypeId::of::<T>() != TypeId::of::<ParameterValue>()
        && entry.value.is::<Binding<ParameterValue>>()
    {
        return c
            .inline_argument::<ParameterValue>(key, &entry)
            .map(|value| (*value).clone());
    }
    c.inline_argument::<T>(key, &entry)
        .map(ParameterValue::from_arc)
}
pub(super) async fn primitive_async<T: Send + Sync + 'static>(
    c: &Container,
    key: ServiceKey,
) -> Result<ParameterValue> {
    let entry = c
        .contextual_argument(&key)?
        .ok_or_else(|| BindingResolutionError::NotBound {
            service: key.clone(),
        })?;
    if TypeId::of::<T>() != TypeId::of::<ParameterValue>()
        && entry.value.is::<Binding<ParameterValue>>()
    {
        return c
            .inline_argument_async::<ParameterValue>(key, &entry)
            .await
            .map(|value| (*value).clone());
    }
    c.inline_argument_async::<T>(key, &entry)
        .await
        .map(ParameterValue::from_arc)
}
fn scalar_carriers<T: 'static>(values: &[ParameterValue]) -> Vec<ParameterValue> {
    if TypeId::of::<T>() == TypeId::of::<ParameterValue>() {
        values.iter().cloned().map(ParameterValue::new).collect()
    } else {
        values.to_vec()
    }
}
pub(super) fn scalars<T: Send + Sync + 'static>(
    c: &Container,
    key: ServiceKey,
) -> Result<Vec<ParameterValue>> {
    let Some(entry) = c.contextual_argument(&key)? else {
        return Ok(Vec::new());
    };
    if entry.value.is::<Binding<Vec<ParameterValue>>>() {
        return c
            .inline_argument::<Vec<ParameterValue>>(key, &entry)
            .map(|values| scalar_carriers::<T>(&values));
    }
    c.inline_argument::<Vec<Arc<T>>>(key, &entry).map(|values| {
        values
            .iter()
            .cloned()
            .map(ParameterValue::from_arc)
            .collect()
    })
}
pub(super) async fn scalars_async<T: Send + Sync + 'static>(
    c: &Container,
    key: ServiceKey,
) -> Result<Vec<ParameterValue>> {
    let Some(entry) = c.contextual_argument(&key)? else {
        return Ok(Vec::new());
    };
    if entry.value.is::<Binding<Vec<ParameterValue>>>() {
        return c
            .inline_argument_async::<Vec<ParameterValue>>(key, &entry)
            .await
            .map(|values| scalar_carriers::<T>(&values));
    }
    c.inline_argument_async::<Vec<Arc<T>>>(key, &entry)
        .await
        .map(|values| {
            values
                .iter()
                .cloned()
                .map(ParameterValue::from_arc)
                .collect()
        })
}
fn absent(error: &BindingResolutionError) -> bool {
    matches!(
        error,
        BindingResolutionError::NotBound { .. }
            | BindingResolutionError::UnresolvableParameter { .. }
    )
}
fn recover(result: Result<Vec<ParameterValue>>) -> Result<Vec<ParameterValue>> {
    match result {
        Err(error) if absent(&error) => Ok(Vec::new()),
        other => other,
    }
}
pub(super) fn classes<T: ?Sized + Send + Sync + 'static>(
    c: &Container,
    key: ServiceKey,
) -> Result<Vec<ParameterValue>> {
    recover((|| {
        let direct = c.contextual_argument(&key)?;
        if let Some(keys) = direct.as_ref().and_then(|entry| entry.services.as_ref()) {
            return keys
                .iter()
                .map(|key| c.get::<T>(key.clone()).map(ParameterValue::new))
                .collect();
        }
        if direct
            .as_ref()
            .is_some_and(|entry| entry.value.is::<Binding<Vec<Arc<T>>>>())
        {
            return c
                .get::<Vec<Arc<T>>>(key)
                .map(|values| values.iter().cloned().map(ParameterValue::new).collect());
        }
        if direct
            .as_ref()
            .is_some_and(|entry| entry.value.is::<Binding<Vec<ParameterValue>>>())
        {
            return c
                .get::<Vec<ParameterValue>>(key)
                .map(|values| (*values).clone());
        }
        let collection = ServiceKey::typed::<Vec<Arc<T>>>();
        if direct.is_none() && c.contextual_argument(&collection)?.is_some() {
            return c
                .get::<Vec<Arc<T>>>(collection)
                .map(|values| values.iter().cloned().map(ParameterValue::new).collect());
        }
        c.get::<T>(key)
            .map(|value| vec![ParameterValue::new(value)])
    })())
}
pub(super) async fn classes_async<T: ?Sized + Send + Sync + 'static>(
    c: &Container,
    key: ServiceKey,
) -> Result<Vec<ParameterValue>> {
    recover(
        async {
            let direct = c.contextual_argument(&key)?;
            if let Some(keys) = direct.as_ref().and_then(|entry| entry.services.as_ref()) {
                let mut values = Vec::with_capacity(keys.len());
                for key in keys.iter() {
                    values.push(ParameterValue::new(c.getAsync::<T>(key.clone()).await?));
                }
                return Ok(values);
            }
            if direct
                .as_ref()
                .is_some_and(|entry| entry.value.is::<Binding<Vec<Arc<T>>>>())
            {
                return c
                    .getAsync::<Vec<Arc<T>>>(key)
                    .await
                    .map(|values| values.iter().cloned().map(ParameterValue::new).collect());
            }
            if direct
                .as_ref()
                .is_some_and(|entry| entry.value.is::<Binding<Vec<ParameterValue>>>())
            {
                return c
                    .getAsync::<Vec<ParameterValue>>(key)
                    .await
                    .map(|values| (*values).clone());
            }
            let collection = ServiceKey::typed::<Vec<Arc<T>>>();
            if direct.is_none() && c.contextual_argument(&collection)?.is_some() {
                return c
                    .getAsync::<Vec<Arc<T>>>(collection)
                    .await
                    .map(|values| values.iter().cloned().map(ParameterValue::new).collect());
            }
            c.getAsync::<T>(key)
                .await
                .map(|value| vec![ParameterValue::new(value)])
        }
        .await,
    )
}
