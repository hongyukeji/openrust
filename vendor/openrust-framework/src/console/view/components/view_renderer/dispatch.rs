// 第三方许可见 ../LICENSE.renderer。
use super::{
    StyleArgument, TerminalElement, TerminalStyles, dimensions, inheritance, properties, style,
    style_methods,
};
use crate::console::{ConsoleError, Terminal};
use serde_json::{Map, Value};
use std::borrow::Cow;

/// 动态元素调用返回原元素借用或查询取得的值。
pub enum TerminalElementResult<'a> {
    Element(&'a mut TerminalElement),
    Value(Value),
}

impl TerminalElement {
    /// 按名称调用元素样式。查询返回值，修改和未定义方法保留元素身份。
    pub fn call(
        &mut self,
        method: &str,
        arguments: &[Value],
    ) -> Result<TerminalElementResult<'_>, ConsoleError> {
        let receiver_query = method.starts_with("get") || method.starts_with("has");
        let normalized = method.to_ascii_lowercase();
        let name = method_name(&normalized);
        let value = match name {
            "getproperties" => Some(Value::Object(self.get_properties()?)),
            "getlength" => {
                let argument = arguments
                    .first()
                    .filter(|value| !value.is_null())
                    .map(|value| value.as_str().ok_or_else(|| incompatible(method)))
                    .transpose()?;
                Some(Value::from(self.get_length(argument)?))
            }
            "getinnerwidth" => Some(Value::from(self.get_inner_width()?)),
            "getparentwidth" => {
                let parent = object(method, first(method, arguments)?)?;
                let wrapped = Map::from_iter([(
                    "parentStyles".to_owned(),
                    Value::Object(parent.into_owned()),
                )]);
                let layout = inheritance::layout(&wrapped)?;
                Some(Value::from(dimensions::parent_width(
                    &layout.parent,
                    Terminal::new().get_width(),
                )?))
            }
            "hasstyle" => {
                let key = text(method, arguments)?;
                let value = self.configure(|styles| Ok(styles.has_style(key)))?;
                Some(Value::Bool(value))
            }
            "defaultstyles" => None,
            "addstyle" => {
                let classes = text(method, arguments)?;
                self.configure(|style| style.add_style(classes).map(|_| ()))?;
                None
            }
            "setproperties" | "with" => {
                let updates = object(method, first(method, arguments)?)?.into_owned();
                if name == "with" {
                    self.with(updates)?;
                } else {
                    self.set_properties(updates)?;
                }
                None
            }
            "format" => {
                self.format_text(text(method, arguments)?)?;
                None
            }
            "inheritfromstyles" => {
                let mut parent = TerminalStyles::new();
                parent.set_properties(object(method, first(method, arguments)?)?.into_owned())?;
                self.inherit_from_styles(&parent)?;
                None
            }
            "setelement" | "__construct" => {
                return Err(ConsoleError::new("终端元素关联和样式构造需要类型化参数"));
            }
            "getformatstring"
            | "getmargins"
            | "getpaddings"
            | "applywidth"
            | "applystyling"
            | "getcolorvariant"
            | "calcwidthfromfraction"
            | "trimtext" => {
                return Err(ConsoleError::new(format!(
                    "不能调用私有终端样式方法：{method}"
                )));
            }
            candidate if style_methods::is_known(candidate) => {
                let arguments = arguments
                    .iter()
                    .take(parameter_count(candidate))
                    .map(|value| {
                        if let Some(value) = value.as_str() {
                            Ok(StyleArgument::Text(value))
                        } else if let Some(value) = value.as_i64() {
                            Ok(StyleArgument::Integer(value))
                        } else {
                            Err(incompatible(method))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.configure(|styles| styles.change(candidate, &arguments).map(|_| ()))?;
                None
            }
            _ => None,
        };
        if receiver_query && let Some(value) = value {
            Ok(TerminalElementResult::Value(value))
        } else {
            Ok(TerminalElementResult::Element(self))
        }
    }

    fn format_text(&self, content: &str) -> Result<String, ConsoleError> {
        let current = self.style_snapshot()?;
        let properties = current.properties.as_deref().expect("终端元素始终拥有属性");
        let first = properties::format_first(properties)?;
        let layout = inheritance::layout(properties)?;
        style::format(
            content.to_owned(),
            &current,
            &layout.parent,
            Terminal::new().get_width(),
            first,
            layout.spacing,
        )
    }
}

fn first<'a>(method: &str, arguments: &'a [Value]) -> Result<&'a Value, ConsoleError> {
    arguments
        .first()
        .ok_or_else(|| ConsoleError::new(format!("终端样式方法缺少参数：{method}")))
}

fn text<'a>(method: &str, arguments: &'a [Value]) -> Result<&'a str, ConsoleError> {
    first(method, arguments)?
        .as_str()
        .ok_or_else(|| incompatible(method))
}

fn object<'a>(method: &str, value: &'a Value) -> Result<Cow<'a, Map<String, Value>>, ConsoleError> {
    if let Some(value) = value.as_object() {
        Ok(Cow::Borrowed(value))
    } else if value.as_array().is_some_and(Vec::is_empty) {
        Ok(Cow::Owned(Map::new()))
    } else {
        Err(incompatible(method))
    }
}

fn incompatible(method: &str) -> ConsoleError {
    ConsoleError::new(format!("终端样式方法参数类型不匹配：{method}"))
}

fn method_name(name: &str) -> &str {
    match name {
        "set_element" => "setelement",
        "default_styles" => "defaultstyles",
        "get_properties" => "getproperties",
        "set_properties" => "setproperties",
        "set_style" => "setstyle",
        "has_style" => "hasstyle",
        "add_style" => "addstyle",
        "inherit_from_styles" => "inheritfromstyles",
        "font_bold" => "fontbold",
        "font_normal" => "fontnormal",
        "space_y" => "spacey",
        "space_x" => "spacex",
        "border_t" => "bordert",
        "w_full" => "wfull",
        "w_auto" => "wauto",
        "min_w" => "minw",
        "max_w" => "maxw",
        "line_through" => "linethrough",
        "justify_between" => "justifybetween",
        "justify_around" => "justifyaround",
        "justify_evenly" => "justifyevenly",
        "justify_center" => "justifycenter",
        "content_repeat" => "contentrepeat",
        "get_format_string" => "getformatstring",
        "get_margins" => "getmargins",
        "get_paddings" => "getpaddings",
        "apply_width" => "applywidth",
        "apply_styling" => "applystyling",
        "get_length" => "getlength",
        "get_inner_width" => "getinnerwidth",
        "get_color_variant" => "getcolorvariant",
        "calc_width_from_fraction" => "calcwidthfromfraction",
        "get_parent_width" => "getparentwidth",
        "trim_text" => "trimtext",
        _ => name,
    }
}

fn parameter_count(name: &str) -> usize {
    match name {
        "bg" | "text" | "truncate" | "list" => 2,
        "setstyle" | "ml" | "mr" | "mb" | "mt" | "mx" | "my" | "m" | "pl" | "pr" | "px" | "pt"
        | "pb" | "py" | "p" | "spacey" | "spacex" | "bordert" | "w" | "minw" | "maxw"
        | "contentrepeat" | "prepend" | "append" | "href" => 1,
        _ => 0,
    }
}
