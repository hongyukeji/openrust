//! 生命周期内的类转换对象缓存。
use super::{
    AttributeMap, AttributeValue, ClassCast, ModelAttributes, Result, apply_attribute_patch,
    next_revision, read_conflict, state_error,
};

#[derive(Clone)]
pub(super) struct ClassCacheEntry {
    pub(super) cast: ClassCast,
    pub(super) value: AttributeValue,
}

impl ModelAttributes {
    pub(super) fn flush_casts(&self, only: Option<&str>) -> Result<()> {
        self.flush_class_casts(only)?;
        self.flush_marked(only)
    }

    fn flush_class_casts(&self, only: Option<&str>) -> Result<()> {
        let (pending, mut snapshot, revision) = {
            let state = self.state.read().map_err(|_| state_error())?;
            let pending = state
                .class_cache
                .iter()
                .filter(|(name, _)| only.is_none_or(|key| key == name.as_str()))
                .map(|(name, entry)| (name.clone(), entry.clone()))
                .collect::<Vec<_>>();
            if pending.is_empty() {
                return Ok(());
            }
            if state.pending_count != 0 || state.active_setters != 0 {
                return Err(read_conflict("model_attribute_update_pending"));
            }
            (pending, state.raw.clone(), state.revision)
        };
        let versions = pending
            .iter()
            .map(|(_, entry)| entry.value.object_revision())
            .collect::<Result<Vec<_>>>()?;
        let mut patch = AttributeMap::new();
        for (name, entry) in &pending {
            let _callback = self.enter_attribute_callback(name)?;
            let fields = entry
                .cast
                .resolve()?
                .set(name, entry.value.clone(), &snapshot)?
                .into_patch(name);
            snapshot.extend(fields.clone());
            patch.extend(fields);
        }
        let mut state = self.state.write().map_err(|_| state_error())?;
        if revision != state.revision || state.pending_count != 0 || state.active_setters != 0 {
            return Err(read_conflict("model_attribute_mutation_conflict"));
        }
        for ((_, entry), version) in pending.iter().zip(versions) {
            if version != entry.value.object_revision()? {
                return Err(read_conflict("model_attribute_mutation_conflict"));
            }
        }
        let retired = apply_attribute_patch(&mut state, patch, super::CacheRetention::KeepAll)?;
        drop(state);
        drop(retired);
        Ok(())
    }

    pub(super) fn read_class_cast(
        &self,
        cast: &ClassCast,
        name: &str,
        input: (Option<&AttributeValue>, &AttributeMap),
        revision: u64,
        cache: bool,
    ) -> Result<AttributeValue> {
        let _callback = self.enter_attribute_callback(name)?;
        let caster = cast.resolve()?;
        {
            let state = self.state.read().map_err(|_| state_error())?;
            if revision != state.revision {
                return Err(read_conflict("model_attribute_read_conflict"));
            }
            if let Some(entry) = state
                .class_cache
                .get(name)
                .filter(|entry| &entry.cast == cast)
            {
                return Ok(entry.value.clone());
            }
        }
        let value = cast.read(caster.as_ref(), name, input.0, input.1)?;
        if cache && cast.caches(&value) {
            let mut state = self.state.write().map_err(|_| state_error())?;
            if revision != state.revision {
                return Err(read_conflict("model_attribute_read_conflict"));
            }
            if state.pending_count == 0 && state.active_setters == 0 {
                state.revision = next_revision(&state)?;
                let retired = state.class_cache.insert(
                    name.to_owned(),
                    ClassCacheEntry {
                        cast: cast.clone(),
                        value: value.clone(),
                    },
                );
                drop(state);
                drop(retired);
            }
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::eloquent::{
            AttributeCast, ClassCastWrite, ClassCaster, ModelAttribute, ModelMetadata, ModelObject,
        },
        support::DataObject,
    };
    use serde_json::{Map, Value, json};
    use std::any::Any;

    struct Units(i64);

    impl DataObject for Units {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn json_projection(&self) -> Option<Value> {
            Some(json!(self.0))
        }
    }

    struct UnitsCast;

    impl ClassCaster for UnitsCast {
        fn get_value(
            &self,
            _: &str,
            value: Option<&AttributeValue>,
            _: &AttributeMap,
        ) -> Result<AttributeValue> {
            let units = value
                .map(AttributeValue::as_json)
                .transpose()?
                .and_then(Value::as_i64)
                .unwrap_or_default();
            Ok(ModelObject::new(Units(units)).into())
        }

        fn set(
            &self,
            name: &str,
            value: AttributeValue,
            _: &AttributeMap,
        ) -> Result<ClassCastWrite> {
            let value = value.as_object().map_or_else(
                || value.clone().try_into_json(),
                |object| {
                    object
                        .snapshot()?
                        .json_projection()
                        .ok_or_else(super::super::state_error)
                },
            )?;
            Ok(ClassCastWrite::Patch(AttributeMap::from_iter([
                (name.to_owned(), value.clone().into()),
                ("unit_copy".into(), value.into()),
            ])))
        }
    }

    fn units(attributes: &ModelAttributes, model: &ModelMetadata) -> Result<ModelObject> {
        attributes
            .get_attribute_value(model, "units")?
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(super::super::state_error)
    }

    #[tokio::test]
    async fn class_objects_cache_on_read_and_write_and_flush_before_dirty() -> Result<()> {
        let mut model = ModelMetadata::new("records", "Record");
        model.casts.insert(
            "units".into(),
            AttributeCast::Class(ClassCast::new(|| Ok(UnitsCast))),
        );
        let attributes = ModelAttributes::new(Map::from_iter([("units".into(), json!(2))]));
        let first = units(&attributes, &model)?;
        assert!(first.same_object(&units(&attributes, &model)?));
        first.modify(|value: &Units| Ok(Units(value.0 + 3)))?;
        attributes.register_attribute(
            "mirror",
            ModelAttribute::new()
                .get(|_, _| Ok(ModelObject::new(Units(0)).into()))
                .set(|_, raw| {
                    Ok(AttributeMap::from_iter([(
                        "mirror_copy".into(),
                        raw.get("unit_copy")
                            .cloned()
                            .unwrap_or_else(|| Value::Null.into()),
                    )]))
                }),
        )?;
        attributes.get_attribute_value(&model, "mirror")?;
        assert_eq!(attributes.get_dirty(&model)?["units"], json!(5));
        assert_eq!(attributes.raw_attribute("unit_copy")?, Some(json!(5)));
        assert_eq!(attributes.raw_attribute("mirror_copy")?, Some(json!(5)));
        assert_eq!(attributes.attributes_to_json(&model)?["units"], json!(5));

        let assigned = ModelObject::new(Units(9));
        attributes
            .set_attribute_value(&model, "units", assigned.clone().into())
            .await?;
        assert!(assigned.same_object(&units(&attributes, &model)?));
        assigned.modify(|_: &Units| Ok(Units(12)))?;
        attributes.sync_original()?;
        assert_eq!(attributes.get_raw_original("units")?, Some(json!(12)));

        attributes.set_raw_attribute("units", json!(15))?;
        assert!(!assigned.same_object(&units(&attributes, &model)?));
        attributes.discard_changes()?;
        assert_eq!(attributes.raw_attribute("units")?, Some(json!(12)));
        Ok(())
    }

    #[test]
    fn disabled_class_object_cache_does_not_write_back() -> Result<()> {
        let mut model = ModelMetadata::new("records", "Record");
        model.casts.insert(
            "units".into(),
            AttributeCast::Class(ClassCast::new(|| Ok(UnitsCast)).without_object_caching()),
        );
        let attributes = ModelAttributes::new(Map::from_iter([("units".into(), json!(2))]));
        let first = units(&attributes, &model)?;
        first.modify(|_: &Units| Ok(Units(20)))?;
        assert!(!first.same_object(&units(&attributes, &model)?));
        assert_eq!(attributes.raw_attribute("units")?, Some(json!(2)));
        assert_eq!(attributes.raw_attribute("unit_copy")?, None);
        Ok(())
    }

    struct OverwriteNeighbor;

    impl ClassCaster for OverwriteNeighbor {
        fn set(
            &self,
            name: &str,
            value: AttributeValue,
            _: &AttributeMap,
        ) -> Result<ClassCastWrite> {
            Ok(ClassCastWrite::Patch(AttributeMap::from_iter([
                (name.to_owned(), value),
                ("units".into(), json!(99).into()),
            ])))
        }
    }

    #[tokio::test]
    async fn class_patch_preserves_neighbor_cache_and_inbound_never_caches() -> Result<()> {
        let mut model = ModelMetadata::new("records", "Record");
        model.casts.insert(
            "units".into(),
            AttributeCast::Class(ClassCast::new(|| Ok(UnitsCast))),
        );
        model.casts.insert(
            "inbound".into(),
            AttributeCast::Class(ClassCast::inbound(|| Ok(OverwriteNeighbor))),
        );
        let attributes = ModelAttributes::new(Map::from_iter([("units".into(), json!(2))]));
        let neighbor = units(&attributes, &model)?;
        neighbor.modify(|_: &Units| Ok(Units(5)))?;
        attributes
            .set_attribute(&model, "inbound", json!(1))
            .await?;
        assert!(neighbor.same_object(&units(&attributes, &model)?));
        assert_eq!(attributes.raw_attribute("units")?, Some(json!(5)));

        model.casts.insert(
            "inbound".into(),
            AttributeCast::Class(ClassCast::inbound(|| Ok(UnitsCast))),
        );
        attributes
            .set_attribute_value(&model, "inbound", ModelObject::new(Units(7)).into())
            .await?;
        assert_eq!(attributes.get_attribute(&model, "inbound")?, Some(json!(7)));
        Ok(())
    }

    #[tokio::test]
    async fn replacing_cached_class_object_keeps_original_writeback_position() -> Result<()> {
        let mut model = ModelMetadata::new("records", "Record");
        let cast = AttributeCast::Class(ClassCast::new(|| Ok(UnitsCast)));
        model.casts.insert("units".into(), cast.clone());
        model.casts.insert("other".into(), cast);
        let attributes = ModelAttributes::new(Map::from_iter([
            ("units".into(), json!(2)),
            ("other".into(), json!(4)),
        ]));
        units(&attributes, &model)?;
        attributes.get_attribute_value(&model, "other")?;
        attributes
            .set_attribute_value(&model, "units", ModelObject::new(Units(7)).into())
            .await?;
        let raw = attributes.raw_attributes()?;
        assert_eq!(raw["units"], json!(7));
        assert_eq!(raw["unit_copy"], json!(4));
        Ok(())
    }
}
