use super::super::ownership::{OwnershipState, ResourceKind};
use super::*;

fn record() -> OwnershipRecord {
    OwnershipRecord {
        version: 1,
        resource_id: format!("openrust_resource_{}", "a".repeat(32)),
        backend: Backend::Mysql,
        server_identity: "server-123".to_owned(),
        admin_database: "mysql".to_owned(),
        database_name: "openrust_marker_test".to_owned(),
        resource_kind: ResourceKind::Target,
        token: "b".repeat(64),
        state: OwnershipState::Owned,
        marker_identity: None,
        last_error: None,
    }
}

#[test]
fn digest_is_stable_and_separates_every_identity_field() -> Result<()> {
    let original = record();
    let proof = MarkerProof::from_record(&original)?;
    assert_eq!(proof.comment, MarkerProof::from_record(&original)?.comment);
    assert_eq!(proof.comment.len(), "openrust-fixture-v1:".len() + 64);
    assert_eq!(
        proof.comment,
        "openrust-fixture-v1:9d26ba54e8a6e3de1c7e4c608c47bd17760bb6623c324086a4345308f9602ee1"
    );
    let mutations: [fn(&mut OwnershipRecord); 7] = [
        |r| r.backend = Backend::Postgres,
        |r| r.resource_kind = ResourceKind::Template,
        |r| r.server_identity.push('2'),
        |r| r.admin_database.push('2'),
        |r| r.database_name.push('2'),
        |r| r.resource_id = format!("openrust_resource_{}", "c".repeat(32)),
        |r| r.token = "c".repeat(64),
    ];
    for change in mutations {
        let mut changed = original.clone();
        change(&mut changed);
        assert_ne!(proof.comment, MarkerProof::from_record(&changed)?.comment);
    }
    let mut changed = original.clone();
    changed.state = OwnershipState::DropPending;
    assert_eq!(proof.comment, MarkerProof::from_record(&changed)?.comment);
    changed.version = 2;
    assert!(
        matches!(MarkerProof::from_record(&changed), Err(error) if error.code() == "test_database_marker_invalid")
    );
    Ok(())
}

#[test]
fn field_lengths_prevent_concatenation_collisions() -> Result<()> {
    let mut left = record();
    left.server_identity = "ab".to_owned();
    left.admin_database = "c".to_owned();
    let mut right = left.clone();
    right.server_identity = "a".to_owned();
    right.admin_database = "bc".to_owned();
    assert_ne!(
        MarkerProof::from_record(&left)?.comment,
        MarkerProof::from_record(&right)?.comment
    );
    Ok(())
}

#[test]
fn unsafe_identifiers_and_forged_records_are_rejected() {
    for name in [
        "openrust_",
        "openrust_Test",
        "openrust_库",
        "openrust_a.b",
        "openrust_a`",
        "openrust_a\"",
        "openrust_a;DROP",
        "other_db",
        &format!("openrust_{}", "x".repeat(55)),
    ] {
        let mut value = record();
        value.database_name = name.to_owned();
        assert!(
            matches!(MarkerProof::from_record(&value), Err(error) if error.code() == "test_database_marker_invalid"),
            "{name}"
        );
    }
    let mut value = record();
    value.token = "B".repeat(64);
    assert!(
        matches!(MarkerProof::from_record(&value), Err(error) if error.code() == "test_database_marker_invalid")
    );
    value = record();
    value.resource_id.push('a');
    assert!(
        matches!(MarkerProof::from_record(&value), Err(error) if error.code() == "test_database_marker_invalid")
    );
}

#[test]
fn debug_never_exposes_proof_or_object_oid() -> Result<()> {
    let value = record();
    let proof = MarkerProof::from_record(&value)?;
    let debug = format!("{proof:?}");
    assert!(!debug.contains(&value.token));
    assert!(!debug.contains(&proof.comment));
    assert!(!debug.contains(&value.server_identity));
    let identity = MarkerIdentity {
        backend: Backend::Postgres,
        database_name: value.database_name,
        postgres_oid: Some(987_654_321),
    };
    assert!(!format!("{identity:?}").contains("987_654_321"));
    Ok(())
}

#[test]
fn mysql_ddl_is_strict_non_table_and_contains_only_derived_comment() -> Result<()> {
    let value = record();
    let proof = MarkerProof::from_record(&value)?;
    let sql = install_sql(&proof);
    assert_eq!(
        sql,
        format!(
            "CREATE PROCEDURE `openrust_marker_test`.`__openrust_test_database_owner`() COMMENT '{}' SQL SECURITY INVOKER BEGIN END",
            proof.comment
        )
    );
    for forbidden in [
        "IF NOT EXISTS",
        "CALL ",
        "TABLE",
        "DROP",
        value.token.as_str(),
    ] {
        assert!(!sql.contains(forbidden));
    }
    let statement = metadata_statement(&proof);
    assert!(
        statement
            .sql
            .contains("ROUTINE_SCHEMA = ? AND ROUTINE_NAME = ? AND ROUTINE_TYPE = ?")
    );
    assert!(!statement.sql.contains(&proof.database_name));
    assert_eq!(
        statement
            .values
            .as_ref()
            .ok_or_else(|| invalid("测试缺少绑定"))?
            .0
            .len(),
        3
    );
    Ok(())
}

#[test]
fn postgres_ddl_and_metadata_use_comment_and_bound_database_name() -> Result<()> {
    let mut value = record();
    value.backend = Backend::Postgres;
    let proof = MarkerProof::from_record(&value)?;
    assert_eq!(
        install_sql(&proof),
        format!(
            "COMMENT ON DATABASE \"openrust_marker_test\" IS '{}'",
            proof.comment
        )
    );
    assert!(!install_sql(&proof).contains(&value.token));
    let statement = metadata_statement(&proof);
    assert!(statement.sql.contains("WHERE datname = $1"));
    assert!(
        statement
            .sql
            .contains("shobj_description(oid, 'pg_database')")
    );
    assert!(!statement.sql.contains(&proof.database_name));
    assert_eq!(
        statement
            .values
            .as_ref()
            .ok_or_else(|| invalid("测试缺少绑定"))?
            .0
            .len(),
        1
    );
    Ok(())
}

#[test]
fn mysql_metadata_requires_exact_schema_type_security_and_comment() -> Result<()> {
    let proof = MarkerProof::from_record(&record())?;
    assert!(mysql_matches(
        &proof,
        &proof.database_name,
        ROUTINE,
        "PROCEDURE",
        "INVOKER",
        Some(&proof.comment)
    ));
    assert!(!mysql_matches(
        &proof,
        "OPENRUST_marker_test",
        ROUTINE,
        "PROCEDURE",
        "INVOKER",
        Some(&proof.comment)
    ));
    assert!(!mysql_matches(
        &proof,
        &proof.database_name,
        "other",
        "PROCEDURE",
        "INVOKER",
        Some(&proof.comment)
    ));
    assert!(!mysql_matches(
        &proof,
        &proof.database_name,
        ROUTINE,
        "FUNCTION",
        "INVOKER",
        Some(&proof.comment)
    ));
    assert!(!mysql_matches(
        &proof,
        &proof.database_name,
        ROUTINE,
        "PROCEDURE",
        "DEFINER",
        Some(&proof.comment)
    ));
    assert!(!mysql_matches(
        &proof,
        &proof.database_name,
        ROUTINE,
        "PROCEDURE",
        "INVOKER",
        None
    ));
    assert!(!mysql_matches(
        &proof,
        &proof.database_name,
        ROUTINE,
        "PROCEDURE",
        "INVOKER",
        Some(&format!("{} ", proof.comment))
    ));
    Ok(())
}

#[test]
fn postgres_recovery_requires_exact_saved_oid_and_database() -> Result<()> {
    let mut value = record();
    value.backend = Backend::Postgres;
    let proof = MarkerProof::from_record(&value)?;
    let actual = MarkerIdentity {
        backend: value.backend,
        database_name: value.database_name,
        postgres_oid: Some(42),
    };
    assert!(!identity_matches(&proof, &actual, None));
    assert!(identity_matches(&proof, &actual, Some(&actual)));
    let mut expected = actual.clone();
    expected.postgres_oid = Some(43);
    assert!(!identity_matches(&proof, &actual, Some(&expected)));
    expected.postgres_oid = None;
    assert!(!identity_matches(&proof, &actual, Some(&expected)));
    expected = actual.clone();
    expected.database_name.push('2');
    assert!(!identity_matches(&proof, &actual, Some(&expected)));
    Ok(())
}

#[test]
fn management_database_case_and_unicode_are_not_normalized_in_proof() -> Result<()> {
    for (left, right) in [("Audit-管理", "audit-管理"), ("Café", "Cafe\u{301}")] {
        let mut original = record();
        original.admin_database = left.to_owned();
        let mut other = original.clone();
        other.admin_database = right.to_owned();
        assert_ne!(
            MarkerProof::from_record(&original)?.comment,
            MarkerProof::from_record(&other)?.comment
        );
    }
    Ok(())
}
