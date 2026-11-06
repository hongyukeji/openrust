//! 不依赖W02空表/投影类型，阻止兄弟live子查询绑定+1/-1抵消导致读写错行。
use super::*;
use crate::database::Db;
use sea_orm::{ConnectionTrait, Database};

#[tokio::test]
async fn sibling_binding_growth_and_shrinkage_cannot_cancel_in_parent_total() -> Result<()> {
    let connection = Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    connection.execute_unprepared("CREATE TABLE child_binding_items(id INTEGER,value INTEGER,label TEXT); INSERT INTO child_binding_items VALUES(1,10,'first'),(2,20,'second'),(3,30,'third')").await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    let a = db.table("child_binding_items")?.select(&["value"])?;
    let replace = db.clone();
    let b = db
        .table("child_binding_items")?
        .select(&["value"])?
        .where_eq("id", 2)?
        .before_query(move |query| {
            *query = replace.table("child_binding_items")?.select(&["value"])?;
            Ok(())
        });
    let parent = db
        .table("child_binding_items")?
        .where_compare("value", super::super::WhereOperator::Eq, &a)?
        .where_compare("value", super::super::WhereOperator::NotEq, &b)?;
    let wrapped = db.table("child_binding_items")?.where_exists(&parent)?;
    let a = a.where_eq("id", 1)?;
    b.apply_before_query_callbacks()?;
    // A当前需要1而挂接0，B当前0而挂接1；父总数恰好仍为1。
    assert!(
        matches!(parent.count().await,Err(error) if error.code()=="query_subquery_bindings_mismatch")
    );
    assert!(
        matches!(wrapped.count().await,Err(error) if error.code()=="query_subquery_bindings_mismatch")
    );
    assert!(
        matches!(parent.update(vec![("label","corrupted".into())]).await,Err(error) if error.code()=="query_subquery_bindings_mismatch")
    );
    assert!(
        matches!(parent.delete().await,Err(error) if error.code()=="query_subquery_bindings_mismatch")
    );
    assert_eq!(db.table("child_binding_items")?.count().await?, 3);
    assert_eq!(
        db.table("child_binding_items")?
            .where_eq("label", "corrupted")?
            .count()
            .await?,
        0
    );
    assert_eq!(
        db.table("child_binding_items")?
            .where_eq("value", 10)?
            .count()
            .await?,
        1
    );
    drop(a);
    drop(b);
    Ok(())
}
