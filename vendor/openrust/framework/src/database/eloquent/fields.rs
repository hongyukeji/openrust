use super::{Model, ModelState};
use crate::database::DatabaseResult;

/// 自动生成区域携带的静态类型入口；不保存应用、连接或可变模型状态。
#[doc(hidden)]
pub struct FieldModel {
    pub(crate) module: &'static str,
    pub(crate) class: &'static str,
    pub(crate) definition: fn() -> DatabaseResult<(String, Option<String>)>,
}

inventory::collect!(FieldModel);

impl FieldModel {
    pub const fn new<M: Model>(module: &'static str, class: &'static str) -> Self {
        Self {
            module,
            class,
            definition: || {
                let model = M::from_state(ModelState::default());
                Ok((
                    model.getTable().to_owned(),
                    model.resolveConnectionName()?.map(|name| name.into_owned()),
                ))
            },
        }
    }
}

/// 从一处可选声明生成模型的列名常量。
///
/// 常量是未限定表名的 `&'static str`，直接复用现有查询接口及标识符语法器。
/// 声明独立于批量赋值白名单和属性转换；不会连接数据库或推断动态属性。
/// 编译器检查常量是否存在，不检查查询模型、值类型或数据库结构。
///
/// ```
/// use openrust::database::eloquent::model_fields;
/// struct User;
/// model_fields! {
///     impl User {
///         pub const ID = "id";
///         pub const NAME = "display_name";
///     }
/// }
/// assert_eq!(User::NAME, "display_name");
/// ```
///
/// 未声明或重命名后仍使用的常量会编译失败：
///
/// ```compile_fail,E0599
/// use openrust::database::eloquent::model_fields;
/// struct User;
/// model_fields! { impl User { pub const NAME = "name"; } }
/// let _ = User::NAEM;
/// ```
#[macro_export]
macro_rules! model_fields {
    ($model:ident { $($(#[$attribute:meta])* $field:ident = $column:literal;)* }) => {
        $crate::model_fields! {
            @model $model;
            #[allow(dead_code, reason = "数据库列集合包含应用尚未引用的字段")]
            impl $model {
                $($(#[$attribute])* pub const $field = $column;)*
            }
        }
    };
    (@model $model:ident; $(#[$implementation:meta])* impl $same:ty { $($fields:tt)* }) => {
        $crate::database::eloquent::__inventory::submit! {
            $crate::database::eloquent::__FieldModel::new::<$model>(module_path!(), stringify!($model))
        }
        $crate::model_fields! { $(#[$implementation])* impl $same { $($fields)* } }
    };
    ($(#[$implementation:meta])* impl $model:ty { $($(#[$attribute:meta])* $visibility:vis const $field:ident = $column:literal;)* }) => {
        $(#[$implementation])*
        impl $model {
            $(
                $(#[$attribute])*
                $visibility const $field: &'static str = $column;
            )*
        }
    };
}
