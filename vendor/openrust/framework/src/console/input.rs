use serde_json::{Map, Value};

use super::Input;
mod mutation;

impl Input {
    pub fn get_helper_set(&self) -> Option<&super::HelperSet> {
        self.helper_set.as_ref()
    }

    pub fn get_helper(
        &self,
        name: &str,
    ) -> Result<std::sync::Arc<dyn super::HelperInterface>, super::ConsoleError> {
        super::helper_set::resolve_helper(self.get_helper_set(), name)
    }

    /// 判断定义中是否存在参数，而不是判断它当前是否有值。
    pub fn has_argument(&self, name: &str) -> bool {
        self.argument_definition(name, false) || (name == "command" && self.invoked_name.is_some())
    }
    pub fn has_option(&self, name: &str) -> bool {
        self.argument_definition(name, true) && !(name == "no-ansi" && self.inherited(name))
    }
    /// 按名称读取位置参数；未知名称返回None。
    pub fn argument(&self, name: &str) -> Option<Value> {
        if name == "command" && !self.argument_definition(name, false) {
            return self.invoked_name.clone().map(Value::String);
        }
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
        let mut values = self.values(false);
        if let Some(name) = &self.invoked_name {
            values
                .entry("command")
                .or_insert_with(|| Value::String(name.clone()));
        }
        values
    }
    pub fn options(&self) -> Map<String, Value> {
        self.values(true)
    }

    fn argument_definition(&self, name: &str, option: bool) -> bool {
        self.parsing_syntax.get_arguments().any(|arg| {
            arg.get_id().as_str() == name
                && (arg.get_long().is_some() || arg.get_short().is_some()) == option
        })
    }
    fn values(&self, option: bool) -> Map<String, Value> {
        self.parsing_syntax
            .get_arguments()
            .filter(|arg| (arg.get_long().is_some() || arg.get_short().is_some()) == option)
            .filter(|arg| {
                !(option && arg.get_id().as_str() == "no-ansi" && self.inherited("no-ansi"))
            })
            .map(|arg| {
                (
                    arg.get_id().to_string(),
                    self.value(arg.get_id().as_str()).unwrap_or(Value::Null),
                )
            })
            .collect()
    }
    fn inherited(&self, name: &str) -> bool {
        self.invoked_name.is_some()
            && !self
                .syntax
                .get_arguments()
                .any(|arg| arg.get_id().as_str() == name)
    }
    fn value(&self, name: &str) -> Option<Value> {
        if self.inherited(name) {
            match name {
                "verbose" => {
                    return Some(Value::Bool(self.raw_value(name)?.as_u64().unwrap_or(0) > 0));
                }
                "env" => {
                    return Some(
                        self.raw_value(name)?
                            .as_array()
                            .and_then(|v| v.last())
                            .cloned()
                            .unwrap_or(Value::Null),
                    );
                }
                "ansi" => {
                    return Some(if self.raw_value("ansi") == Some(Value::Bool(true)) {
                        Value::Bool(true)
                    } else if self.raw_value("no-ansi") == Some(Value::Bool(true)) {
                        Value::Bool(false)
                    } else {
                        Value::Null
                    });
                }
                "no-ansi" => return None,
                _ => {}
            }
        }
        self.raw_value(name)
    }
    fn raw_value(&self, name: &str) -> Option<Value> {
        let arg = self
            .parsing_syntax
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
            match arg.get_action() {
                clap::ArgAction::SetTrue
                | clap::ArgAction::Help
                | clap::ArgAction::HelpShort
                | clap::ArgAction::HelpLong
                | clap::ArgAction::Version => Some(Value::Bool(false)),
                clap::ArgAction::SetFalse => Some(Value::Bool(true)),
                clap::ArgAction::Count => Some(Value::from(0)),
                _ => None,
            }
        }
    }
}
