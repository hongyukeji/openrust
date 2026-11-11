use std::time::SystemTime;

use crate::database::Value;

pub(in crate::database::eloquent) fn timestamp() -> Value {
    let time = chrono::DateTime::<chrono::Utc>::from(SystemTime::now());
    Value::from(time.format("%Y-%m-%d %H:%M:%S").to_string())
}
