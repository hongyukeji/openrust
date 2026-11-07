//! 增强认证流程的秘密哈希值；计算只委派应用统一 Hasher，不持有算法配置或容量池。

use crate::{
    Error, Result,
    hashing::{HashOptions, Hasher},
};

/// 用于认证持久版本比较的已编码秘密值，有意不实现 `Debug`。
///
/// 此类型不判断存储算法；标准 `DatabaseUserProvider` 直接使用通用 Hasher。
/// 增强流程的 make/verify 额外保留非空与1024字节输入边界，并拒绝 bcrypt 截断别名。
#[derive(Clone, PartialEq, Eq)]
pub struct EncodedPasswordHash(String);

impl EncodedPasswordHash {
    /// 原样封装存储值；合法性、算法检查与计算预算由实际选用的 Hasher 决定。
    pub fn from_encoded(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 在检索账户前统一核验增强认证输入；不执行KDF，也不依据账户是否存在决定策略。
    pub fn validate_input(hasher: &dyn Hasher, password: &str) -> Result<()> {
        validate_input(password)?;
        if !hasher.accepts_exact_input(password.as_bytes())? {
            return Err(Error::custom(
                "password_bcrypt_input_ambiguous",
                "增强认证输入在当前哈希或迁移策略下会发生截断；请选择完整字节算法或安全重设",
            ));
        }
        Ok(())
    }

    /// 通过统一 Hasher 生成增强认证所需的哈希；不另建驱动或配置来源。
    pub async fn make(hasher: &dyn Hasher, password: &str) -> Result<Self> {
        Self::validate_input(hasher, password)?;
        let encoded = hasher
            .make(password.as_bytes(), &HashOptions::new())
            .await?;
        validate_representation(password, &encoded)?;
        Ok(Self(encoded))
    }

    /// 通过同一个 Hasher 核验增强认证输入，拒绝 bcrypt 的长输入与 NUL 别名。
    pub async fn verify(&self, hasher: &dyn Hasher, password: &str) -> Result<bool> {
        Self::validate_input(hasher, password)?;
        validate_representation(password, &self.0)?;
        hasher
            .check(password.as_bytes(), Some(&self.0), &HashOptions::new())
            .await
    }

    /// 根据统一 Hasher 的当前策略判断重哈希，不以随机盐或独立成本配置作决定。
    pub fn needs_rehash(&self, hasher: &dyn Hasher) -> Result<bool> {
        hasher.needs_rehash(&self.0, &HashOptions::new())
    }

    /// 返回持久化和字节级版本比较所需的原始编码。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_input(password: &str) -> Result<()> {
    if password.is_empty() || password.len() > 1024 {
        return Err(Error::BadRequest(
            "password must contain between 1 and 1024 UTF-8 bytes".into(),
        ));
    }
    Ok(())
}

fn validate_representation(password: &str, encoded: &str) -> Result<()> {
    if encoded.parse::<bcrypt::HashParts>().is_ok()
        && (password.len() > 72 || password.as_bytes().contains(&0))
    {
        return Err(Error::custom(
            "password_bcrypt_input_ambiguous",
            "增强认证的bcrypt输入不能超过72字节或包含NUL；完整字节密码请选择Argon驱动",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashing::{BcryptConfig, HashManager, HashingConfig};

    fn manager() -> Result<HashManager> {
        HashManager::new(HashingConfig {
            bcrypt: BcryptConfig {
                rounds: 4,
                verify: true,
                limit: None,
            },
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn typed_auth_value_uses_shared_bcrypt_driver_and_random_salts() -> Result<()> {
        let manager = manager()?;
        let first = EncodedPasswordHash::make(&manager, "correct horse").await?;
        let second = EncodedPasswordHash::make(&manager, "correct horse").await?;
        assert!(first.as_str().starts_with("$2y$04$"));
        assert_ne!(first.as_str(), second.as_str());
        assert!(first.verify(&manager, "correct horse").await?);
        assert!(!first.verify(&manager, "wrong horse").await?);
        assert!(!first.needs_rehash(&manager)?);
        Ok(())
    }

    #[tokio::test]
    async fn enhanced_auth_rejects_aliases_without_changing_generic_hash_semantics() -> Result<()> {
        let manager = manager()?;
        for (plain, alias) in [
            ("x".repeat(72), format!("{}tail", "x".repeat(72))),
            ("correct".to_owned(), "correct\0tail".to_owned()),
        ] {
            let hash = EncodedPasswordHash::make(&manager, &plain).await?;
            assert!(
                manager
                    .check(&alias, Some(hash.as_str()), &HashOptions::new())
                    .await?
            );
            assert!(
                matches!(hash.verify(&manager, &alias).await, Err(error) if error.code() == "password_bcrypt_input_ambiguous")
            );
        }
        assert!(
            EncodedPasswordHash::make(&manager, &"x".repeat(73))
                .await
                .is_err()
        );
        assert!(EncodedPasswordHash::make(&manager, "").await.is_err());
        assert!(
            EncodedPasswordHash::make(&manager, &"x".repeat(1025))
                .await
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn arbitrary_stored_values_are_checked_by_the_selected_driver() -> Result<()> {
        let manager = manager()?;
        let unknown = EncodedPasswordHash::from_encoded("not-a-hash");
        assert!(unknown.verify(&manager, "password").await.is_err());
        let empty = EncodedPasswordHash::from_encoded("");
        assert!(!empty.verify(&manager, "password").await?);
        Ok(())
    }
}
