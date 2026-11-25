use openrust::database::{
    Value,
    query::{Expression, UpdateValue},
};

use crate::{
    DeliveryStore, Result,
    delivery_store::{DELIVERIES, check_id},
    publication::MESSAGES,
};

#[derive(Clone, Copy, Debug)]
pub enum RetentionRecord {
    Delivery,
    Publication,
}

impl DeliveryStore {
    /// 对范围内的一条记录设置或解除保留。返回标记是否发生变化。
    pub async fn set_retention_hold(
        &self,
        record: RetentionRecord,
        id: &str,
        hold: bool,
    ) -> Result<bool> {
        check_id(id)?;
        let table = match record {
            RetentionRecord::Delivery => DELIVERIES,
            RetentionRecord::Publication => MESSAGES,
        };
        let row = self
            .connection
            .table(table)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("id", "=", id);
        let (row, value) = if hold {
            (
                row.where_null("retention_hold_at"),
                UpdateValue::Operand(Expression::new(self.clock()).into()),
            )
        } else {
            (
                row.where_not_null("retention_hold_at"),
                UpdateValue::Operand(Value::Null.into()),
            )
        };
        Ok(row
            .update_with([("retention_hold_at".into(), value)].into_iter().collect())
            .await?
            == 1)
    }
}
