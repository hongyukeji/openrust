use serde_json::{Map, Value};

use super::Input;

impl Input {
    /// 按名称读取位置参数；未知名称返回None。
    pub fn argument(&self, name: &str) -> Option<Value> {
        self.argument_definition(name, false)
            .then(|| self.value(name))
            .flatten()
    }

    /// 按名称读取选项；未知名称返回None。
    pub fn option(&self, name: &str) -> Option<Value> {
        self.argument_definition(name, true)
            .then(|| self.value(name))
            .flatten()
    }

    pub fn arguments(&self) -> Map<String, Value> {
        self.values(false)
    }
    pub fn options(&self) -> Map<String, Value> {
        self.values(true)
    }

    fn argument_definition(&self, name: &str, option: bool) -> bool {
        self.syntax.get_arguments().any(|arg| {
            arg.get_id().as_str() == name
                && (arg.get_long().is_some() || arg.get_short().is_some()) == option
        })
    }
    fn values(&self, option: bool) -> Map<String, Value> {
        self.syntax
            .get_arguments()
            .filter(|arg| (arg.get_long().is_some() || arg.get_short().is_some()) == option)
            .map(|arg| {
                (
                    arg.get_id().to_string(),
                    self.value(arg.get_id().as_str()).unwrap_or(Value::Null),
                )
            })
            .collect()
    }
    fn value(&self, name: &str) -> Option<Value> {
        let arg = self
            .syntax
            .get_arguments()
            .find(|arg| arg.get_id().as_str() == name)?;
        let array = matches!(arg.get_action(), clap::ArgAction::Append)
            || arg
                .get_num_args()
                .is_some_and(|range| range.max_values() > 1);
        // Clap保留无值选项的空出现组，无需插入魔法字符串充当null。
        if array
            && (arg.get_long().is_some() || arg.get_short().is_some())
            && arg
                .get_num_args()
                .is_some_and(|range| range.min_values() == 0)
            && let Ok(Some(groups)) = self.matches.try_get_occurrences::<String>(name)
        {
            let values = groups
                .flat_map(|group| {
                    let mut values: Vec<Value> = group.cloned().map(Value::from).collect();
                    if values.is_empty() {
                        values.push(Value::Null);
                    }
                    values
                })
                .collect();
            return Some(Value::Array(values));
        }
        macro_rules! values {
            ($type:ty) => {
                if let Ok(Some(values)) = self.matches.try_get_many::<$type>(name) {
                    let mut values: Vec<Value> = values.cloned().map(Value::from).collect();
                    return Some(if array {
                        Value::Array(values)
                    } else {
                        values.pop().unwrap_or(Value::Null)
                    });
                }
            };
        }
        values!(String);
        values!(bool);
        values!(u8);
        values!(u16);
        values!(u32);
        values!(u64);
        values!(i8);
        values!(i16);
        values!(i32);
        values!(i64);
        values!(f64);
        if let Ok(Some(values)) = self.matches.try_get_many::<std::ffi::OsString>(name) {
            let mut values: Vec<Value> = values
                .map(|value| {
                    value
                        .to_str()
                        .map_or(Value::Null, |text| Value::String(text.into()))
                })
                .collect();
            return Some(if array {
                Value::Array(values)
            } else {
                values.pop().unwrap_or(Value::Null)
            });
        }
        if array {
            Some(Value::Array(Vec::new()))
        } else {
            None
        }
    }
}
