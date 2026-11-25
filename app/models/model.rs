//! 应用模型的公共入口。模型可覆写框架 `Model` trait 的默认方法。

#[allow(unused_imports, reason = "应用模型可从这里导入框架契约")]
pub(crate) use openrust::database::eloquent::Model;
