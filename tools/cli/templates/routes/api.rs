//! 无状态 API 路由；前缀与 middleware 组由 `bootstrap/app.rs` 装配。

use openrust::prelude::*;
use crate::app::http::controllers::home::index as root;

pub fn register(routes: Routes) -> Routes {
{{routes}}
}
{{auth_handler}}
