//! 每种新标识符独立累计生成次数；不重置其它kind，也不以随机值必然不同为前提。

use crate::{GrantIdentifierKind, GrantStorageError, grant_records::TokenRecords};

pub(crate) struct IdentifierRetryBudget {
    code: u8,
    access: u8,
    refresh: u8,
}

impl IdentifierRetryBudget {
    pub(crate) fn code() -> Self {
        Self {
            code: 1,
            access: 0,
            refresh: 0,
        }
    }

    pub(crate) fn tokens(has_refresh: bool) -> Self {
        Self {
            code: 0,
            access: 1,
            refresh: u8::from(has_refresh),
        }
    }

    pub(crate) fn exhausted(&self, error: &GrantStorageError) -> bool {
        match error {
            GrantStorageError::IdentifierCollision(GrantIdentifierKind::Code) => self.code == 10,
            GrantStorageError::IdentifierCollision(GrantIdentifierKind::Access) => {
                self.access == 10
            }
            GrantStorageError::IdentifierCollision(GrantIdentifierKind::Refresh) => {
                self.refresh == 10
            }
            _ => false,
        }
    }

    pub(crate) fn retry(&mut self, error: &GrantStorageError) -> Option<GrantIdentifierKind> {
        let GrantStorageError::IdentifierCollision(kind) = error else {
            return None;
        };
        let count = match kind {
            GrantIdentifierKind::Code => &mut self.code,
            GrantIdentifierKind::Access => &mut self.access,
            GrantIdentifierKind::Refresh => &mut self.refresh,
        };
        if *count == 0 || *count >= 10 {
            return None;
        }
        *count += 1;
        Some(*kind)
    }
}

pub(crate) fn replace_token_identifier<E>(
    tokens: &mut TokenRecords,
    kind: GrantIdentifierKind,
    mut generate: impl FnMut() -> Result<String, E>,
) -> Result<(), E> {
    match kind {
        GrantIdentifierKind::Access => {
            tokens.access.id = generate()?;
            if let Some(refresh) = &mut tokens.refresh {
                refresh.access.id.clone_from(&tokens.access.id);
            }
        }
        GrantIdentifierKind::Refresh => {
            if let Some(refresh) = &mut tokens.refresh {
                refresh.id = generate()?;
            }
        }
        // 预算先拒绝错误阶段，不在此生成未受支持的记录。
        GrantIdentifierKind::Code => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "identifier_retry_tests.rs"]
mod tests;
