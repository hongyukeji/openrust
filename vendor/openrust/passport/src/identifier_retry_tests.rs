//! 捕获预算重置、错误阶段重试及随机重复值被错误忽略。

use super::*;
use crate::{AccessRecord, GrantAuthorityBinding, RefreshRecord};
use GrantIdentifierKind::{Access, Code, Refresh};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn tenth_generation_is_allowed_but_eleventh_is_not() {
    for kind in [Code, Access, Refresh] {
        let mut budget = if kind == Code {
            IdentifierRetryBudget::code()
        } else {
            IdentifierRetryBudget::tokens(true)
        };
        for _ in 1..10 {
            assert_eq!(
                budget.retry(&GrantStorageError::IdentifierCollision(kind)),
                Some(kind)
            );
        }
        assert_eq!(
            budget.retry(&GrantStorageError::IdentifierCollision(kind)),
            None
        );
    }
}

#[test]
fn alternating_collisions_allow_nineteen_atomic_attempts_without_reset() {
    let mut budget = IdentifierRetryBudget::tokens(true);
    let mut attempts = 1;
    for _ in 1..10 {
        for kind in [Access, Refresh] {
            assert_eq!(
                budget.retry(&GrantStorageError::IdentifierCollision(kind)),
                Some(kind)
            );
            attempts += 1;
        }
    }
    assert_eq!(attempts, 19);
    for kind in [Access, Refresh] {
        assert_eq!(
            budget.retry(&GrantStorageError::IdentifierCollision(kind)),
            None
        );
    }
}

#[test]
fn unsupported_kinds_and_all_other_errors_never_retry() {
    let mut code = IdentifierRetryBudget::code();
    let mut access_only = IdentifierRetryBudget::tokens(false);
    for kind in [Access, Refresh] {
        assert_eq!(
            code.retry(&GrantStorageError::IdentifierCollision(kind)),
            None
        );
    }
    for kind in [Code, Refresh] {
        assert_eq!(
            access_only.retry(&GrantStorageError::IdentifierCollision(kind)),
            None
        );
    }
    for error in [
        GrantStorageError::ClientChanged,
        GrantStorageError::CodeRevoked,
        GrantStorageError::RefreshRevoked,
        GrantStorageError::Expired,
        GrantStorageError::BindingMismatch,
        GrantStorageError::AuthorityDenied,
        GrantStorageError::Unavailable(openrust::Error::custom("commit_unknown", "未知提交结果")),
    ] {
        assert_eq!(code.retry(&error), None);
        assert_eq!(access_only.retry(&error), None);
    }
}

fn tokens() -> openrust::Result<TokenRecords> {
    let access = AccessRecord {
        id: "same-access".to_owned(),
        client_id: "client".to_owned(),
        user_id: "00042".to_owned(),
        scopes: vec!["read".to_owned()],
        issued_at: 100,
        expires_at: 200,
        authority: GrantAuthorityBinding::new("verified-fixture".to_owned())?,
    };
    Ok(TokenRecords {
        refresh: Some(RefreshRecord {
            id: "same-refresh".to_owned(),
            access: access.clone(),
            expires_at: 300,
        }),
        access,
    })
}

#[test]
fn repeated_random_value_counts_and_only_colliding_identifier_is_generated() -> TestResult {
    let mut records = tokens()?;
    let mut budget = IdentifierRetryBudget::tokens(true);
    let mut generated = 0;
    for _ in 1..10 {
        let kind = budget
            .retry(&GrantStorageError::IdentifierCollision(Access))
            .ok_or("预算提前耗尽")?;
        replace_token_identifier(&mut records, kind, || {
            generated += 1;
            Ok::<_, openrust::Error>("same-access".to_owned())
        })?;
    }
    assert_eq!(generated, 9);
    assert_eq!(
        budget.retry(&GrantStorageError::IdentifierCollision(Access)),
        None
    );
    replace_token_identifier(&mut records, Refresh, || {
        Ok::<_, openrust::Error>("new-refresh".to_owned())
    })?;
    let refresh = records.refresh.as_ref().ok_or("缺少refresh")?;
    assert_eq!(refresh.id, "new-refresh");
    assert_eq!(refresh.access.id, records.access.id);
    assert_eq!(records.access.id, "same-access");
    assert_eq!(records.access.user_id, "00042");
    assert_eq!(records.access.client_id, "client");
    assert_eq!(records.access.scopes, ["read"]);
    assert_eq!(records.access.authority.as_str(), "verified-fixture");
    assert_eq!(
        (
            records.access.issued_at,
            records.access.expires_at,
            refresh.expires_at
        ),
        (100, 200, 300)
    );
    replace_token_identifier(&mut records, Access, || {
        Ok::<_, openrust::Error>("new-access".to_owned())
    })?;
    assert_eq!(records.access.id, "new-access");
    assert_eq!(
        records.refresh.as_ref().ok_or("缺少refresh")?.access.id,
        "new-access"
    );
    assert_eq!(
        records.refresh.as_ref().ok_or("缺少refresh")?.id,
        "new-refresh"
    );
    Ok(())
}
