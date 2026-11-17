use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseResult, Row, Transaction, Value, query::grammars::Grammar,
};

const RELATIONS: &str = "
    select n.nspname::text as schema, c.relname::text as name
    from pg_catalog.pg_class c join pg_catalog.pg_namespace n on n.oid = c.relnamespace
    where n.nspname = any(pg_catalog.current_schemas(false))
      and n.nspname !~ '^pg_' and n.nspname <> 'information_schema'
      and c.relkind::text in (?, ?)
    order by n.nspname, c.relname";

pub(super) const TYPES: &str = "
    select n.nspname::text as schema, t.typname::text as name,
           t.typtype::text as type_code, t.typcategory::text as category_code,
           ((t.typinput = 'pg_catalog.array_in'::pg_catalog.regproc
             and t.typoutput = 'pg_catalog.array_out'::pg_catalog.regproc)
             or t.typtype = 'm') as implicit
    from pg_catalog.pg_type t join pg_catalog.pg_namespace n on n.oid = t.typnamespace
    left join pg_catalog.pg_class c on c.oid = t.typrelid
    left join pg_catalog.pg_type el on el.oid = t.typelem
    left join pg_catalog.pg_class ce on ce.oid = el.typrelid
    where ((t.typrelid = 0 and (ce.relkind = 'c' or ce.relkind is null)) or c.relkind = 'c')
      and not exists (
          select 1 from pg_catalog.pg_depend d
          where d.classid = 'pg_catalog.pg_type'::pg_catalog.regclass
            and d.objid in (t.oid, t.typelem) and d.deptype = 'e'
      )";

// PostgreSQL 10 and 11 lack pg_attribute.attgenerated. Reading the optional
// field from the row keeps a single query valid across supported versions.
pub(super) const COLUMNS: &str = "
    select a.attname::text as name, t.typname::text as type_name,
           pg_catalog.format_type(a.atttypid, a.atttypmod) as full_type,
           not a.attnotnull as nullable,
           case when gen.kind = '' then expression.value end as default_value,
           (a.attidentity <> '' or coalesce(expression.value like 'nextval(%', false)) as auto_increment,
           exists (select 1 from pg_catalog.pg_constraint p
                   where p.conrelid = c.oid and p.contype = 'p' and a.attnum = any(p.conkey)) as primary_key,
           pg_catalog.col_description(c.oid, a.attnum) as comment,
           coll.collcollate as collation,
           case gen.kind when 's' then 'stored' when 'v' then 'virtual' end as generation_kind,
           case when gen.kind <> '' then expression.value end as generation_expression
    from pg_catalog.pg_attribute a
    join pg_catalog.pg_class c on c.oid = a.attrelid
    join pg_catalog.pg_namespace n on n.oid = c.relnamespace
    join pg_catalog.pg_type t on t.oid = a.atttypid
    left join pg_catalog.pg_attrdef d on d.adrelid = a.attrelid and d.adnum = a.attnum
    left join pg_catalog.pg_collation coll on coll.oid = a.attcollation
    cross join lateral (select coalesce(pg_catalog.to_jsonb(a)->>'attgenerated', '') as kind) gen
    cross join lateral (select pg_catalog.pg_get_expr(d.adbin, d.adrelid) as value) expression
    where n.nspname::text = coalesce(nullif(?, ''), pg_catalog.current_schema()::text)
      and c.relname::text = ? and a.attnum > 0 and not a.attisdropped
    order by a.attnum";

pub(super) const FOREIGN_KEYS: &str = "
    select con.conname::text as key_id, source_col.attname::text as column_name,
           target_ns.nspname::text as foreign_schema, target.relname::text as foreign_table,
           target_col.attname::text as foreign_column,
           case con.confdeltype when 'a' then 'no action' when 'r' then 'restrict'
                when 'c' then 'cascade' when 'n' then 'set null' when 'd' then 'set default' end as on_delete,
           case con.confupdtype when 'a' then 'no action' when 'r' then 'restrict'
                when 'c' then 'cascade' when 'n' then 'set null' when 'd' then 'set default' end as on_update
    from pg_catalog.pg_constraint con
    join pg_catalog.pg_class source on source.oid = con.conrelid
    join pg_catalog.pg_namespace source_ns on source_ns.oid = source.relnamespace
    join pg_catalog.pg_class target on target.oid = con.confrelid
    join pg_catalog.pg_namespace target_ns on target_ns.oid = target.relnamespace
    cross join lateral unnest(con.conkey, con.confkey) with ordinality as pair(source_num, target_num, position)
    join pg_catalog.pg_attribute source_col on source_col.attrelid = source.oid and source_col.attnum = pair.source_num
    join pg_catalog.pg_attribute target_col on target_col.attrelid = target.oid and target_col.attnum = pair.target_num
    where con.contype = 'f'
      and source_ns.nspname::text = coalesce(nullif(?, ''), pg_catalog.current_schema()::text)
      and source.relname::text = ?
    order by con.conname, pair.position";

pub(super) async fn drop_schema_objects(
    connection: &Connection,
    drop_tables: bool,
    drop_views: bool,
    drop_types: bool,
) -> DatabaseResult<()> {
    let grammar = connection.grammar.clone();
    let excluded = Arc::clone(&connection.postgres_dont_drop);
    connection
        .transaction(
            move |transaction| {
                let grammar = grammar.clone();
                let excluded = Arc::clone(&excluded);
                Box::pin(async move {
                    if drop_views {
                        let views = transaction
                            .select(RELATIONS, &["v".into(), "v".into()])
                            .await?;
                        drop_group(transaction, &grammar, "view", &views, &[]).await?;
                    }
                    if drop_tables {
                        let tables = transaction
                            .select(RELATIONS, &["r".into(), "p".into()])
                            .await?;
                        drop_group(transaction, &grammar, "table", &tables, &excluded).await?;
                    }
                    if drop_types {
                        let sql = format!(
                            "select * from ({TYPES}) as type_catalog
                             where schema = any(pg_catalog.current_schemas(false)::text[])
                               and schema !~ '^pg_' and schema <> 'information_schema'
                               and not implicit order by schema, name"
                        );
                        let (domains, types): (Vec<_>, Vec<_>) = transaction
                            .select(&sql, &[])
                            .await?
                            .into_iter()
                            .partition(|row| matches!(row.get("type_code"), Some(Value::Text(kind)) if kind == "d"));
                        drop_group(transaction, &grammar, "type", &types, &[]).await?;
                        drop_group(transaction, &grammar, "domain", &domains, &[]).await?;
                    }
                    Ok(())
                })
            },
            1,
        )
        .await
}

async fn drop_group(
    transaction: &mut Transaction,
    grammar: &Grammar,
    kind: &str,
    rows: &[Row],
    excluded: &[String],
) -> DatabaseResult<()> {
    if let Some(sql) = drop_statement(grammar, kind, rows, excluded)? {
        transaction.statement(&sql, &[]).await?;
    }
    Ok(())
}

fn drop_statement(
    grammar: &Grammar,
    kind: &str,
    rows: &[Row],
    excluded: &[String],
) -> DatabaseResult<Option<String>> {
    let mut targets = Vec::new();
    for row in rows {
        let (Some(Value::Text(schema)), Some(Value::Text(name))) =
            (row.get("schema"), row.get("name"))
        else {
            return Err(DatabaseError::InvalidSchema(
                "PostgreSQL catalog object name",
            ));
        };
        if excluded
            .iter()
            .any(|excluded| excluded == name || excluded == &format!("{schema}.{name}"))
        {
            continue;
        }
        targets.push(format!(
            "{}.{}",
            grammar.quote(schema)?,
            grammar.quote(name)?
        ));
    }
    Ok((!targets.is_empty())
        .then(|| format!("drop {kind} if exists {} cascade", targets.join(", "))))
}

#[cfg(test)]
#[path = "../../../../tests/unit/postgres_schema_wipe.rs"]
mod tests;
