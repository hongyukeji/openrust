// Compiled model identities preserve the application's morph map.
pub(crate) fn identity<M: openrust::database::eloquent::Model>(
    model: &M,
) -> crate::Result<crate::Subject> {
    use openrust::database::Value as Sql;
    let id = match model.get_key() {
        Some(Sql::Text(value)) => value.clone(),
        Some(Sql::Integer(value)) => value.to_string(),
        Some(Sql::Unsigned(value)) => value.to_string(),
        _ => return Err(crate::Error::InvalidActivity),
    };
    crate::Subject::new(model.get_morph_class()?, id)
}

/// Per-model logging switch. Clone copies the value, so two model instances remain independent.
pub struct LoggingState(std::sync::atomic::AtomicBool);
impl Default for LoggingState {
    fn default() -> Self {
        Self(std::sync::atomic::AtomicBool::new(true))
    }
}
impl Clone for LoggingState {
    fn clone(&self) -> Self {
        Self(std::sync::atomic::AtomicBool::new(
            self.0.load(std::sync::atomic::Ordering::Relaxed),
        ))
    }
}
pub mod concerns;

/// The native Eloquent activity model. Custom models keep these columns and may use another table.
#[derive(Clone)]
pub struct Activity(openrust::database::eloquent::ModelState);
impl openrust::database::eloquent::Model for Activity {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("activity_log").guarded(&[]);
        &META
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        use openrust::database::eloquent::casts::AttributeCast;
        &[
            ("properties", AttributeCast::Json),
            ("attribute_changes", AttributeCast::Json),
        ]
    }
}
impl Activity {
    pub fn subject<R: openrust::database::eloquent::Model>(
        &self,
    ) -> openrust::database::DatabaseResult<openrust::database::eloquent::relations::MorphTo<Self>>
    {
        use openrust::database::eloquent::Model;
        let relation =
            openrust::database::eloquent::relations::MorphTo::new("subject_type", "subject_id")
                .target_mapped::<R>(
                    &self.get_connection()?,
                    R::from_state(Default::default()).get_key_name(),
                )?;
        let include =
            openrust::support::facades::Config::get("activitylog.include_soft_deleted_subjects")?
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        Ok(if include {
            relation.with_trashed()
        } else {
            relation
        })
    }
    pub fn causer<R: openrust::database::eloquent::Model>(
        &self,
    ) -> openrust::database::DatabaseResult<openrust::database::eloquent::relations::MorphTo<Self>>
    {
        use openrust::database::eloquent::Model;
        openrust::database::eloquent::relations::MorphTo::new("causer_type", "causer_id")
            .target_mapped::<R>(
                &self.get_connection()?,
                R::from_state(Default::default()).get_key_name(),
            )
    }
    pub fn get_property(
        &self,
        path: &str,
        default: serde_json::Value,
    ) -> crate::Result<serde_json::Value> {
        use openrust::database::eloquent::Model;
        let value = self
            .get_attribute_value("properties")?
            .as_ref()
            .map(crate::ChangeDetector::json)
            .transpose()?
            .unwrap_or_default();
        Ok(property(&value, path).cloned().unwrap_or(default))
    }
}

// 保留既有公开导入路径，存储适配由独立模块实现。
pub use crate::model_store::ActivityModel;

pub(crate) fn property<'a>(
    mut value: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    if let Some(value) = value.get(path) {
        return Some(value);
    }
    for key in path.split('.') {
        value = match value {
            serde_json::Value::Array(array) => array.get(key.parse::<usize>().ok()?)?,
            _ => value.get(key)?,
        };
    }
    Some(value)
}
