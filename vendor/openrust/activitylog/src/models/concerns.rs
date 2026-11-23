//! 模型的日志能力与属性提取；监听和持久化调度由独立模块处理。
use crate::model_observer::LogsActivityListener;
use crate::{Activity, ActivityQuery, ActivityRepository, ChangeDetector, LogOptions, Result};
use openrust::{
    database::{
        DatabaseResult,
        eloquent::{Model, concerns::ModelEvent},
    },
    foundation::Application,
};
use serde_json::{Map, Value};
use std::{collections::BTreeSet, sync::Arc};

/// Implement on a model, then register `boot_logs_activity` in the application's Provider.
pub trait LogsActivity: Model + Clone {
    fn activitylog_state(&self) -> &super::LoggingState;
    fn disable_logging(&mut self) -> &mut Self {
        self.activitylog_state()
            .0
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self
    }
    fn enable_logging(&mut self) -> &mut Self {
        self.activitylog_state()
            .0
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self
    }
    fn get_activitylog_options(&self) -> LogOptions {
        LogOptions::defaults()
    }
    fn enable_logging_models_events(&self) -> bool {
        self.activitylog_state()
            .0
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    fn events_to_be_recorded() -> &'static [ModelEvent] {
        &[
            ModelEvent::Created,
            ModelEvent::Updated,
            ModelEvent::Deleted,
            ModelEvent::Restored,
        ]
    }
    fn do_not_record_events() -> &'static [ModelEvent] {
        &[]
    }
    fn boot_logs_activity(
        app: &Application,
        repository: Arc<ActivityRepository>,
    ) -> DatabaseResult<()> {
        for event in Self::events_to_be_recorded() {
            if !Self::do_not_record_events().contains(event) {
                Self::register_model_event(
                    app,
                    *event,
                    LogsActivityListener {
                        repository: repository.clone(),
                    },
                )?;
            }
        }
        Ok(())
    }
    fn attributes_to_be_logged(&self) -> Vec<String> {
        self.attributes_to_be_logged_with(&[])
    }
    fn attributes_to_be_logged_with(&self, defaults: &[String]) -> Vec<String> {
        let options = self.get_activitylog_options();
        let mut attributes = BTreeSet::new();
        if options.log_fillable {
            attributes.extend(self.get_fillable().iter().map(|name| (*name).to_owned()));
        }
        if options.log_unguarded {
            let guarded = self.get_guarded();
            if Self::is_unguarded() || !guarded.contains(&"*") {
                attributes.extend(
                    self.state()
                        .get_attributes()
                        .keys()
                        .filter(|name| Self::is_unguarded() || !guarded.contains(&name.as_str()))
                        .cloned(),
                );
            }
        }
        if options.log_attributes.iter().any(|name| name == "*") {
            attributes.extend(self.state().get_attributes().keys().cloned());
        }
        attributes.extend(
            options
                .log_attributes
                .iter()
                .filter(|name| name.as_str() != "*")
                .cloned(),
        );
        attributes.retain(|name| {
            !options.log_except_attributes.contains(name) && !defaults.contains(name)
        });
        attributes.into_iter().collect()
    }
    fn get_description_for_event(&self, event: &str) -> String {
        self.get_activitylog_options()
            .description_for_event
            .map_or_else(|| event.into(), |callback| callback(event))
    }
    fn before_activity_logged(&self, _activity: &mut Activity, _event: &str) -> Result<()> {
        Ok(())
    }
    fn activities_as_subject(&self, repository: &ActivityRepository) -> Result<ActivityQuery> {
        Ok(repository.query().for_subject(&super::identity(self)?))
    }
    fn extract_changes(&self) -> Result<Map<String, Value>> {
        self.extract_changes_with_defaults(&[])
    }
    fn extract_changes_with_defaults(&self, defaults: &[String]) -> Result<Map<String, Value>> {
        let options = self.get_activitylog_options();
        let mut values = Map::new();
        for attribute in self.attributes_to_be_logged_with(defaults) {
            if attribute.contains('.') {
                let snapshot = Value::Object(self.to_array()?);
                let mut value = &snapshot;
                for key in attribute.split('.') {
                    value = value.get(key).unwrap_or(&Value::Null);
                }
                values.insert(attribute, value.clone());
            } else if attribute.contains("->") {
                let base = attribute.split("->").next().unwrap();
                let value = self
                    .get_attribute_value(base)?
                    .as_ref()
                    .map(ChangeDetector::json)
                    .transpose()?
                    .unwrap_or(Value::Null);
                let root = Map::from_iter([(base.into(), value)]);
                let value = ChangeDetector::resolve_json_attribute(&root, &attribute);
                set_path(
                    &mut values,
                    &attribute.split("->").collect::<Vec<_>>(),
                    value,
                );
            } else {
                let raw = if options.attribute_raw_values.contains(&attribute) {
                    self.get_attribute(&attribute).cloned()
                } else {
                    self.get_attribute_value(&attribute)?
                };
                values.insert(
                    attribute,
                    raw.as_ref()
                        .map(ChangeDetector::json)
                        .transpose()?
                        .unwrap_or(Value::Null),
                );
            }
        }
        Ok(values)
    }
}
fn set_path(values: &mut Map<String, Value>, path: &[&str], value: Value) {
    if path.len() == 1 {
        values.insert(path[0].into(), value);
        return;
    }
    let child = values
        .entry(path[0])
        .or_insert_with(|| Value::Object(Map::new()));
    if !child.is_object() {
        *child = Value::Object(Map::new());
    }
    set_path(child.as_object_mut().unwrap(), &path[1..], value);
}
