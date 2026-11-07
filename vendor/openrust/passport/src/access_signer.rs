//! OAuth access token 的独立 RS256 签发、公钥验证及显式配对检查。
//!
//! JWT 验证不替代持久仓储的撤销检查。OAuth access key 必须与应用 JWT、OIDC
//! ID token key 隔离；上游没有用途 claim，同 key 同形状的令牌不能凭密码学区分。

use std::fmt;

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

/// 固定上游 access token 的公开载荷，不额外添加 issuer 或用途字段。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassportAccessClaims {
    /// 客户端 audience 列表，单项在 JWT 中序列化为字符串。
    #[serde(with = "audience_wire")]
    pub aud: Vec<String>,
    /// 持久 access token identifier。
    pub jti: String,
    /// 原用户标识；无用户 grant 回退为 client identifier。
    pub sub: String,
    /// 已批准的 scope identifiers。
    pub scopes: Vec<String>,
    /// 签发时刻，Unix 秒。
    pub iat: u64,
    /// 最早可用时刻，Unix 秒。
    pub nbf: u64,
    /// 失效时刻，Unix 秒。
    pub exp: u64,
}

mod audience_wire {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer>(
        audience: &[String],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match audience {
            [client] => client.serialize(serializer),
            _ => audience.serialize(serializer),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<String>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Audience {
            Single(String),
            Multiple(Vec<String>),
        }

        Ok(match Audience::deserialize(deserializer)? {
            Audience::Single(client) => vec![client],
            Audience::Multiple(clients) => clients,
        })
    }
}

impl PassportAccessClaims {
    /// 返回唯一 audience 对应的 client；不新增非上游 `client_id` claim。
    #[must_use]
    pub fn client_id(&self) -> Option<&str> {
        match self.aud.as_slice() {
            [client] => Some(client),
            _ => None,
        }
    }
}

/// 脱敏签发与验证失败，不携带原始 PEM、JWT 或密码学库错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AccessSigningError {
    /// 私钥不能实际完成 RSA 签名。
    #[error("OAuth RSA private signing key is invalid")]
    InvalidPrivateKey,
    /// 公钥不能通过成熟 RSA 解析。
    #[error("OAuth RSA public verification key is invalid")]
    InvalidPublicKey,
    /// 显式配对检查不能证明私钥与公钥配对。
    #[error("OAuth RSA signing key pair is invalid")]
    InvalidKeyPair,
    /// access token 的必需身份或期限无效。
    #[error("OAuth access token claims are invalid")]
    InvalidClaims,
    /// 签名失败。
    #[error("OAuth access token signing failed")]
    SigningFailed,
    /// 算法、签名、audience、载荷或有效期校验失败。
    #[error("OAuth access token verification failed")]
    VerificationFailed,
}

/// 独立 OAuth access token 签发能力，只持有专用 RSA 私钥。
///
/// 授权服务器不需要持有资源服务器的公钥；需要预检配对时显式调用
/// [`validate_key_pair`]。不能传入应用 JWT 或 OIDC ID token 使用的密钥。
pub struct OAuthAccessSigner {
    signing: EncodingKey,
}

impl fmt::Debug for OAuthAccessSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthAccessSigner")
            .finish_non_exhaustive()
    }
}

impl OAuthAccessSigner {
    /// 只读取私钥，并经成熟库实际执行 RS256 签名，拒绝仅有 PEM/ASN.1 外形的材料。
    ///
    /// # Errors
    /// 私钥格式错误或不能实际签名时返回脱敏错误；不要求公钥或配对检查。
    pub fn from_pem(private_pem: &[u8]) -> Result<Self, AccessSigningError> {
        let signer = Self {
            signing: EncodingKey::from_rsa_pem(private_pem)
                .map_err(|_| AccessSigningError::InvalidPrivateKey)?,
        };
        // PEM 解码不证明 RSA 私钥可签名；encode 的实际 RSA backend 必须完成一次签名。
        signer
            .sign_at("key-signing-proof", "key-signing-proof", None, &[], 1, 2)
            .map_err(|_| AccessSigningError::InvalidPrivateKey)?;
        Ok(signer)
    }

    /// 生成包含唯一 client audience 的 access token，保留用户身份原字符串。
    ///
    /// `user_id=None` 时 sub 为 client，供无用户 grant 使用。
    ///
    /// # Errors
    /// 身份为空、期限无效或 RSA 签名失败时返回脱敏错误。
    pub fn sign_at(
        &self,
        client_id: &str,
        token_id: &str,
        user_id: Option<&str>,
        scopes: &[String],
        now: u64,
        expires_at: u64,
    ) -> Result<String, AccessSigningError> {
        let subject = user_id.unwrap_or(client_id);
        if client_id.is_empty() || token_id.is_empty() || subject.is_empty() || expires_at <= now {
            return Err(AccessSigningError::InvalidClaims);
        }
        let claims = PassportAccessClaims {
            aud: vec![client_id.to_owned()],
            jti: token_id.to_owned(),
            sub: subject.to_owned(),
            scopes: scopes.to_vec(),
            iat: now,
            nbf: now,
            exp: expires_at,
        };
        encode(&Header::new(Algorithm::RS256), &claims, &self.signing)
            .map_err(|_| AccessSigningError::SigningFailed)
    }
}

/// 独立 OAuth access token 验证能力，只持有专用 RSA 公钥。
///
/// JWT 验证不能替代持久仓储的 revoked 检查。此类型不持有签发私钥，
/// 不接受应用 JWT 或 OIDC ID token 的密钥/用途作为 OAuth 身份。
pub struct OAuthAccessVerifier {
    verification: DecodingKey,
}

impl fmt::Debug for OAuthAccessVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthAccessVerifier")
            .finish_non_exhaustive()
    }
}

impl OAuthAccessVerifier {
    /// 只读取公钥，并通过现有密码学 provider 的成熟 RSA 公钥解析验证材料。
    ///
    /// # Errors
    /// 公钥格式错误或 RSA 结构无效时返回脱敏错误；不要求私钥或配对检查。
    pub fn from_pem(public_pem: &[u8]) -> Result<Self, AccessSigningError> {
        let verification = DecodingKey::from_rsa_pem(public_pem)
            .map_err(|_| AccessSigningError::InvalidPublicKey)?;
        // from_rsa_pem 只解码 PEM/ASN.1；JWK 投影委托同一成熟 provider 解析 RSA。
        jsonwebtoken::jwk::Jwk::from_decoding_key(&verification, Some(Algorithm::RS256))
            .map_err(|_| AccessSigningError::InvalidPublicKey)?;
        Ok(Self { verification })
    }

    /// 校验算法、签名、唯一 client audience 及显式时刻的有效期。
    ///
    /// 这是显式 expected-client 绑定的诊断/回归入口，不是固定 League `ResourceServer`
    /// 的默认接受域；后者没有默认 expected-audience/issuer，`LooseValidAt` 也不要求
    /// 所有时间声明存在。本方法保留当前候选的必需字段和完整日期检查。
    /// 不增设 issuer；本结果不能替代 DB revoked 校验。
    ///
    /// # Errors
    /// 非 RS256、错误 key/client、非 access 载荷或期限无效时拒绝。
    pub fn verify_at(
        &self,
        token: &str,
        expected_client: &str,
        now: u64,
    ) -> Result<PassportAccessClaims, AccessSigningError> {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.leeway = 0;
        validation.set_required_spec_claims(&["aud", "sub", "exp", "nbf"]);
        validation.set_audience(&[expected_client]);
        // 库没有可注入时钟；仅时间判断使用显式 now，签名及 audience 仍由库验证。
        validation.validate_exp = false;
        validation.validate_nbf = false;
        let claims = decode::<PassportAccessClaims>(token, &self.verification, &validation)
            .map_err(|_| AccessSigningError::VerificationFailed)?
            .claims;
        if expected_client.is_empty()
            || claims.client_id() != Some(expected_client)
            || claims.jti.is_empty()
            || claims.sub.is_empty()
            || claims.iat > now
            || claims.nbf > now
            || claims.exp <= now
            || claims.exp <= claims.nbf
            || claims.exp <= claims.iat
        {
            return Err(AccessSigningError::VerificationFailed);
        }
        Ok(claims)
    }
}

/// 显式增强：用私钥实际签名并由给定公钥验证，证明这对 OAuth RSA 密钥配对。
///
/// 两种独立能力的构造不自动调用本检查；资源服务器无需私钥，签发服务器无需公钥。
///
/// # Errors
/// 任一材料无效、签名失败或公钥不能验证签名时返回脱敏配对错误。
pub fn validate_key_pair(private_pem: &[u8], public_pem: &[u8]) -> Result<(), AccessSigningError> {
    let check = || {
        let signer = OAuthAccessSigner::from_pem(private_pem)?;
        let verifier = OAuthAccessVerifier::from_pem(public_pem)?;
        let proof = signer.sign_at("key-pair-proof", "key-pair-proof", None, &[], 1, 2)?;
        verifier.verify_at(&proof, "key-pair-proof", 1)?;
        Ok::<(), AccessSigningError>(())
    };
    check().map_err(|_| AccessSigningError::InvalidKeyPair)
}

#[cfg(test)]
#[path = "access_signer_tests.rs"]
mod tests;
