//! 应用仓储的公共入口。实现 `Repository` 即可沿用默认 CRUD 并按需覆写。

#[allow(unused_imports, reason = "默认实现供应用开发者直接使用")]
pub(crate) use openrust::database::eloquent::EloquentRepository;
pub(crate) use openrust::database::eloquent::Repository;
