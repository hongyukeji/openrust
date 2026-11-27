use super::{
    Connection, DatabaseError, DatabaseResult, Record, TABLE, Transaction, billing, budget,
    integer, now, text,
};

pub(super) const PENDING: &str = "batch_pending";
const RECOVERING: &str = "batch_recovering";

pub(super) async fn recover(
    connection: &Connection,
    owner_column: &'static str,
    owner: &str,
    budget_column: &str,
    cutoff: i64,
    limit: u64,
) -> DatabaseResult<usize> {
    let batches = connection
        .table(TABLE)
        .use_write_pdo()
        .select(["request_id", owner_column])
        .distinct()
        .r#where(owner_column, "=", owner)
        .r#where("outcome", "=", PENDING)
        .r#where("started_at", "<=", cutoff)
        .where_not_null(budget_column)
        .where_null("finished_at")
        .order_by("request_id", "asc")
        .limit(limit)
        .get()
        .await?;
    let mut recovered = 0_usize;
    for batch in batches {
        if text(&batch, owner_column)? != owner {
            continue;
        }
        let request = text(&batch, "request_id")?;
        recovered += connection
            .transaction(
                move |transaction| {
                    let request = request.clone();
                    Box::pin(async move { close_batch(transaction, &request, cutoff).await })
                },
                3,
            )
            .await?;
        if recovered as u64 >= limit {
            break;
        }
    }
    Ok(recovered)
}

async fn close_batch(
    transaction: &mut Transaction,
    request: &str,
    cutoff: i64,
) -> DatabaseResult<usize> {
    let claimed = transaction
        .table(TABLE)
        .r#where("request_id", "=", request)
        .r#where("outcome", "=", PENDING)
        .where_null("finished_at")
        .update(Record::from([("outcome".into(), RECOVERING.into())]))
        .await?;
    if claimed == 0 {
        return Ok(0);
    }
    let rows = transaction
        .table(TABLE)
        .select([
            "id",
            "started_at",
            "dispatch_started_at",
            "usage_state",
            "input_tokens",
            "budget_id",
            "reserved_receivable_atoms",
            "account_budget_id",
            "reserved_cost_atoms",
        ])
        .r#where("request_id", "=", request)
        .r#where("outcome", "=", RECOVERING)
        .where_null("finished_at")
        .order_by("id", "asc")
        .get()
        .await?;
    for row in &rows {
        let started = integer(row, "started_at")?.ok_or(DatabaseError::InvalidScalar)?;
        if started > cutoff || integer(row, "dispatch_started_at")?.is_some_and(|at| at > cutoff) {
            transaction
                .table(TABLE)
                .r#where("request_id", "=", request)
                .r#where("outcome", "=", RECOVERING)
                .update(Record::from([("outcome".into(), PENDING.into())]))
                .await?;
            return Ok(0);
        }
    }
    for row in &rows {
        let id = text(row, "id")?;
        let dispatched = integer(row, "dispatch_started_at")?.is_some();
        let tokens = if text(row, "usage_state")? == "final" {
            integer(row, "input_tokens")?
        } else {
            None
        };
        let outcome = if dispatched {
            "batch_aborted"
        } else {
            "expired_before_dispatch"
        };
        if transaction
            .table(TABLE)
            .r#where("id", "=", id.as_str())
            .r#where("outcome", "=", RECOVERING)
            .where_null("finished_at")
            .update(Record::from([
                ("outcome".into(), outcome.into()),
                ("finished_at".into(), now().into()),
            ]))
            .await?
            != 1
        {
            return Err(DatabaseError::InvalidQuery(
                "AI 网关批量恢复遇到并发状态变更",
            ));
        }
        let cost =
            billing::close_interrupted_embedding(transaction, &id, dispatched, tokens).await?;
        if let Some(reserved) = integer(row, "reserved_receivable_atoms")? {
            budget::release_reservation(transaction, &text(row, "budget_id")?, reserved, 0).await?;
        }
        if let Some(reserved) = integer(row, "reserved_cost_atoms")?
            && !budget::settlements::applied(
                transaction,
                &id,
                billing::adjustments::Direction::Cost,
            )
            .await?
        {
            budget::settle_reserved(
                transaction,
                &text(row, "account_budget_id")?,
                reserved,
                cost,
            )
            .await?;
        }
    }
    Ok(rows.len())
}
