//! 综合示例的真实业务实体。

pub(crate) mod article;
pub(crate) mod article_event;
#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
pub(crate) mod article_import;
#[cfg(feature = "storage-local")]
pub(crate) mod attachment_cleanup;
pub(crate) mod example_record;
pub(crate) mod tenant_membership;
pub(crate) mod tenant_record;
