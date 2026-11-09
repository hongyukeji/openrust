//! 与真实处理器共同登记的动作身份；不提供动态控制器实例化。

use crate::core::{Error, Result};

/// 路由动作声明，支持字符串和二元组。
///
/// 字符串查询可使用生成器根命名空间；二元组查询始终表示绝对类与方法。
/// 登记二元组同样绕过组命名空间。裸字符串可表示 invokable 身份，
/// 实际执行始终由同次登记的 Rust handler 负责。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteAction(ActionInput);

#[derive(Clone, Debug, PartialEq, Eq)]
enum ActionInput {
    String(String),
    Pair(String, String),
}

impl From<&str> for RouteAction {
    fn from(value: &str) -> Self {
        Self(ActionInput::String(value.to_owned()))
    }
}

impl From<String> for RouteAction {
    fn from(value: String) -> Self {
        Self(ActionInput::String(value))
    }
}

impl From<(&str, &str)> for RouteAction {
    fn from((class, method): (&str, &str)) -> Self {
        Self(ActionInput::Pair(class.to_owned(), method.to_owned()))
    }
}

impl From<(String, String)> for RouteAction {
    fn from((class, method): (String, String)) -> Self {
        Self(ActionInput::Pair(class, method))
    }
}

impl RouteAction {
    pub(crate) fn registered(self, namespace: Option<&str>) -> Result<String> {
        self.validate()?;
        Ok(match self.0 {
            ActionInput::String(action) => match namespace {
                Some(namespace) if !action.starts_with('\\') && !action.starts_with(namespace) => {
                    format!("{namespace}\\{action}")
                }
                _ => action,
            },
            ActionInput::Pair(class, method) => format!("{class}@{method}"),
        })
    }

    pub(crate) fn query(self, namespace: Option<&str>) -> Result<String> {
        self.validate()?;
        let action = match self.0 {
            ActionInput::String(action) => action,
            ActionInput::Pair(class, method) => format!("\\{class}@{method}"),
        };
        Ok(match namespace {
            Some(namespace) if !action.starts_with('\\') => format!("{namespace}\\{action}"),
            _ => action.trim_matches('\\').to_owned(),
        })
    }

    fn validate(&self) -> Result<()> {
        match &self.0 {
            ActionInput::String(action) => {
                validate_identity(action)?;
                if let Some((class, method)) = action.split_once('@') {
                    validate_identity(class)?;
                    validate_identity(method)?;
                }
                Ok(())
            }
            ActionInput::Pair(class, method) => {
                validate_identity(class)?;
                validate_identity(method)
            }
        }
    }
}

pub(crate) fn validate_identity(value: &str) -> Result<()> {
    if value.trim_matches('\\').is_empty() || value.chars().any(char::is_control) {
        return Err(Error::custom(
            "http_route_action_invalid",
            "动作身份成员不能为空或包含控制字符",
        ));
    }
    Ok(())
}

pub(crate) fn validate_namespace(value: &str) -> Result<()> {
    if value.chars().any(char::is_control) {
        return Err(Error::custom(
            "http_route_action_invalid",
            "动作命名空间不能包含控制字符",
        ));
    }
    Ok(())
}

pub(crate) fn group_namespace(parent: Option<&str>, child: Option<&str>) -> Result<Option<String>> {
    let Some(child) = child else {
        return Ok(parent.map(str::to_owned));
    };
    validate_namespace(child)?;
    Ok(Some(match parent {
        Some(parent) if !child.starts_with('\\') => {
            format!(
                "{}\\{}",
                parent.trim_matches('\\'),
                child.trim_matches('\\')
            )
        }
        _ => child.trim_matches('\\').to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_input_shapes_preserve_namespace_and_raw_identity() -> Result<()> {
        // 二元组不是相对字符串的语法糖；查询与登记的 namespace 责任也不同。
        for (action, registered, queried) in [
            (
                RouteAction::from("Draft@show"),
                "App\\Draft@show",
                "App\\Draft@show",
            ),
            (
                RouteAction::from("\\Draft@show"),
                "\\Draft@show",
                "Draft@show",
            ),
            (
                RouteAction::from(("Draft", "show")),
                "Draft@show",
                "Draft@show",
            ),
            (
                RouteAction::from(("Draft".to_owned(), "show".to_owned())),
                "Draft@show",
                "Draft@show",
            ),
            (
                RouteAction::from("Invokable".to_owned()),
                "App\\Invokable",
                "App\\Invokable",
            ),
            (
                RouteAction::from("Application@show"),
                "Application@show",
                "App\\Application@show",
            ),
            (
                RouteAction::from(" Whitespace@show "),
                "App\\ Whitespace@show ",
                "App\\ Whitespace@show ",
            ),
        ] {
            assert_eq!(action.clone().registered(Some("App"))?, registered);
            assert_eq!(action.query(Some("App"))?, queried);
        }
        Ok(())
    }

    #[test]
    fn nested_action_namespaces_follow_absolute_reset_and_do_not_trim_spaces() -> Result<()> {
        assert_eq!(
            group_namespace(Some("\\App\\"), Some("Admin\\"))?,
            Some("App\\Admin".into())
        );
        assert_eq!(
            group_namespace(Some("App"), Some("\\Other\\"))?,
            Some("Other".into())
        );
        assert_eq!(group_namespace(Some("App"), None)?, Some("App".into()));
        assert_eq!(
            group_namespace(Some("App"), Some(""))?,
            Some("App\\".into())
        );
        assert_eq!(group_namespace(None, Some(" App "))?, Some(" App ".into()));
        Ok(())
    }

    #[test]
    fn invalid_action_members_and_controls_fail_without_class_name_regex() -> Result<()> {
        for action in [
            RouteAction::from(""),
            RouteAction::from("\\"),
            RouteAction::from("Draft@"),
            RouteAction::from("@show"),
            RouteAction::from(("Draft", "")),
            RouteAction::from(("", "show")),
            RouteAction::from("Draft\n@show"),
        ] {
            assert_eq!(
                action
                    .clone()
                    .registered(None)
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some("http_route_action_invalid")
            );
            assert_eq!(
                action
                    .query(None)
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some("http_route_action_invalid")
            );
        }
        assert!(validate_namespace("").is_ok());
        assert_eq!(
            validate_namespace("App\0")
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_invalid")
        );
        assert_eq!(
            RouteAction::from("not-a-PHP-class/name@method").query(None)?,
            "not-a-PHP-class/name@method"
        );
        Ok(())
    }
}
