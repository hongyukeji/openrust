//! Configurable actions are owned by the application/scope, with no process-global callbacks.
use crate::{Activity, Result};
use serde_json::Value;
use std::sync::{Arc, RwLock};

type BeforeLogging = Arc<dyn Fn(&mut Activity) -> Result<()> + Send + Sync>;
#[derive(Clone, Default)]
pub struct LogActivityAction {
    callbacks: Arc<RwLock<Vec<BeforeLogging>>>,
}
impl LogActivityAction {
    pub fn before_logging(
        &self,
        callback: impl Fn(&mut Activity) -> Result<()> + Send + Sync + 'static,
    ) {
        self.callbacks.write().unwrap().push(Arc::new(callback));
    }
    pub fn clear_before_logging_callbacks(&self) {
        self.callbacks.write().unwrap().clear();
    }
    pub fn execute(&self, activity: &mut Activity, description: String) -> Result<()> {
        self.execute_with(activity, description, |_| Ok(()))
    }
    pub(crate) fn execute_with(
        &self,
        activity: &mut Activity,
        description: String,
        before: impl FnOnce(&mut Activity) -> Result<()>,
    ) -> Result<()> {
        let description = activity.description.take().unwrap_or(description);
        activity.description = Some(replace_placeholders(&description, activity));
        before(activity)?;
        // Clone before invoking user callbacks: callbacks may register or clear hooks themselves.
        let callbacks = self.callbacks.read().unwrap().clone();
        for callback in callbacks {
            callback(activity)?;
        }
        Ok(())
    }
}
fn replace_placeholders(description: &str, activity: &Activity) -> String {
    let mut output = String::new();
    let mut remaining = description;
    while let Some(offset) = remaining.find(':') {
        output.push_str(&remaining[..offset]);
        remaining = &remaining[offset + 1..];
        let end = remaining
            .find(|c: char| !(c.is_ascii_alphanumeric() || "._-".contains(c)))
            .unwrap_or(remaining.len());
        let candidate = &remaining[..end];
        let token = candidate.trim_end_matches('.');
        let resolved = token.split_once('.').and_then(|(source, path)| {
            let value = match source {
                "properties" => activity.properties.clone(),
                "subject" => {
                    activity.subject.as_ref()?;
                    activity.subject_attributes.clone()
                }
                "causer" => {
                    activity.causer.as_ref()?;
                    activity.causer_attributes.clone()
                }
                _ => return None,
            };
            let mut value = &value;
            for key in path.split('.') {
                value = match value {
                    Value::Array(values) => values.get(key.parse::<usize>().ok()?)?,
                    _ => value.get(key)?,
                };
            }
            match value {
                Value::String(value) => Some(value.clone()),
                Value::Number(value) => Some(value.to_string()),
                Value::Bool(true) => Some("1".into()),
                Value::Bool(false) | Value::Null => Some(String::new()),
                _ => None,
            }
        });
        if let Some(value) = resolved {
            output.push_str(&value);
        } else {
            output.push(':');
            output.push_str(token);
        }
        output.push_str(&candidate[token.len()..]);
        remaining = &remaining[end..];
    }
    output.push_str(remaining);
    output
}

type Cleaner = Arc<
    dyn Fn(
            Arc<crate::ActivityRepository>,
            i64,
            Option<String>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send>>
        + Send
        + Sync,
>;
pub struct CleanActivityLogAction {
    repository: Arc<crate::ActivityRepository>,
    cleaner: Option<Cleaner>,
}
impl CleanActivityLogAction {
    pub fn new(repository: Arc<crate::ActivityRepository>) -> Self {
        Self {
            repository,
            cleaner: None,
        }
    }
    pub fn using<F, Work>(mut self, cleaner: F) -> Self
    where
        F: Fn(Arc<crate::ActivityRepository>, i64, Option<String>) -> Work + Send + Sync + 'static,
        Work: std::future::Future<Output = Result<u64>> + Send + 'static,
    {
        self.cleaner = Some(Arc::new(move |repository, days, name| {
            Box::pin(cleaner(repository, days, name))
        }));
        self
    }
    pub async fn execute(&self, max_age_in_days: i64, log_name: Option<&str>) -> Result<u64> {
        if let Some(cleaner) = &self.cleaner {
            return cleaner(
                self.repository.clone(),
                max_age_in_days,
                log_name.map(str::to_owned),
            )
            .await;
        }
        let cutoff = Self::get_cut_off_date(max_age_in_days)?;
        self.repository
            .delete_old_activities(cutoff, log_name)
            .await
    }
    pub fn get_cut_off_date(max_age_in_days: i64) -> Result<i64> {
        use openrust::support::{Date, DateInterval, MonthOverflow};
        if max_age_in_days < 1 {
            return Err(crate::Error::InvalidActivity);
        }
        let now = Date::now(None).map_err(|_| crate::Error::InvalidConfiguration)?;
        let cutoff = now
            .minus(
                DateInterval {
                    days: max_age_in_days,
                    ..DateInterval::default()
                },
                MonthOverflow::Carry,
            )
            .map_err(|_| crate::Error::InvalidActivity)?;
        // The upstream cleaner formats seconds, discarding fractional seconds.
        cutoff
            .as_date_time()
            .timestamp()
            .as_second()
            .checked_mul(1000)
            .ok_or(crate::Error::InvalidActivity)
    }
}
