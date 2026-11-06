//! 框架unit包装；仅环境输入使用既有cfg(test)快照，不造私有context/Events。
use crate as framework;
#[path = "transaction_context_cases.rs"]
mod cases;
#[tokio::test]
async fn mysql_context_provenance_order_rollback_and_cancellation() -> crate::Result<()> {
    let admin = crate::test_database_environment::database_url().map_err(crate::Error::internal)?;
    cases::run_contract(&admin).await
}
