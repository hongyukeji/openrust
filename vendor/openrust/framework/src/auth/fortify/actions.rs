//! 双因素账户动作；持久化走同一模型能力，事件沿用框架Events。

use std::{fmt, sync::Arc};

use chrono::Utc;
use zeroize::Zeroizing;

use crate::Error;
use crate::Result;
use crate::auth::AuthUser;
use crate::auth::FactorManagementContext;
use crate::auth::FactorReceipt;
use crate::auth::FactorVerificationMethod;
use crate::auth::UserProvider;
use crate::core::error::ValidationErrors;
use crate::database::DatabaseTransaction;
use crate::database::Db;
use crate::encryption::ApplicationEncrypter;
use crate::events::Events;

use super::authority::{FactorAuthorityMutation, FactorAuthoritySnapshot};
use super::{
    TwoFactorAuthenticatable, TwoFactorAuthenticationProvider, TwoFactorOptions, TwoFactorState,
};

macro_rules! account_event {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone)]
        pub struct $name {
            /// 同一Guard/Provider返回的账户。
            pub user: AuthUser,
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("user", &"[REDACTED]")
                    .finish()
            }
        }
    };
}
account_event!(
    TwoFactorAuthenticationEnabled,
    "双因素密钥与恢复码已存储；不等同已确认绑定。"
);

impl crate::events::Event for TwoFactorAuthenticationEnabled {
    const NAME: &'static str = r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationEnabled";
}

account_event!(TwoFactorAuthenticationConfirmed, "双因素绑定验证码已确认。");

impl crate::events::Event for TwoFactorAuthenticationConfirmed {
    const NAME: &'static str = r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationConfirmed";
}

account_event!(TwoFactorAuthenticationDisabled, "双因素三项属性已清除。");

impl crate::events::Event for TwoFactorAuthenticationDisabled {
    const NAME: &'static str = r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationDisabled";
}

account_event!(RecoveryCodesGenerated, "双因素恢复码已重新生成。");

impl crate::events::Event for RecoveryCodesGenerated {
    const NAME: &'static str = r"OpenRust\Auth\TwoFactor\Events\RecoveryCodesGenerated";
}

/// 恢复码替换事件，保留官方code字段但禁止Debug泄露。
#[derive(Clone)]
pub struct RecoveryCodeReplaced {
    /// 同一Provider的账户。
    pub user: AuthUser,
    /// 已被替换的恢复码；仅显式expose才读取。
    pub code: RecoveryCode,
}

impl crate::events::Event for RecoveryCodeReplaced {
    const NAME: &'static str = r"OpenRust\Auth\TwoFactor\Events\RecoveryCodeReplaced";
}

impl fmt::Debug for RecoveryCodeReplaced {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryCodeReplaced")
            .field("user", &"[REDACTED]")
            .field("code", &self.code)
            .finish()
    }
}

/// 可交给受信事件监听器的恢复码，调试输出始终脱敏。
#[derive(Clone)]
pub struct RecoveryCode(Zeroizing<String>);
impl RecoveryCode {
    /// 包装由受保护流程取得的恢复码。
    pub fn new(code: String) -> Self {
        Self(Zeroizing::new(code))
    }
    /// 显式读取秘密内容，不应记录到日志。
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode([REDACTED])")
    }
}

#[derive(Clone)]
pub(super) struct ManagementEvent {
    user: AuthUser,
    events: Events,
    audit: Option<Arc<dyn super::api_audit::ApiFactorManagementAuditSink>>,
}

impl ManagementEvent {
    async fn dispatch<E: crate::events::Event>(
        &self,
        event: &E,
        tx: Option<&DatabaseTransaction>,
    ) -> Result<()> {
        match tx {
            Some(tx) => self
                .events
                .dispatch_in_transaction(tx, event)
                .await
                .map(|_| ()),
            None => self.events.dispatch(event).await.map(|_| ()),
        }
    }

    async fn send(
        &self,
        mutation: FactorAuthorityMutation,
        tx: Option<&DatabaseTransaction>,
    ) -> Result<()> {
        let user = self.user.clone();
        match mutation {
            FactorAuthorityMutation::Enable => {
                self.dispatch(&TwoFactorAuthenticationEnabled { user }, tx)
                    .await
            }
            FactorAuthorityMutation::Confirm => {
                self.dispatch(&TwoFactorAuthenticationConfirmed { user }, tx)
                    .await
            }
            FactorAuthorityMutation::Regenerate => {
                self.dispatch(&RecoveryCodesGenerated { user }, tx).await
            }
            FactorAuthorityMutation::Disable => {
                self.dispatch(&TwoFactorAuthenticationDisabled { user }, tx)
                    .await
            }
        }
    }

    pub(super) async fn before_commit(
        &self,
        db: &Db,
        tx: &DatabaseTransaction,
        mutation: FactorAuthorityMutation,
        user_id: i64,
    ) -> Result<()> {
        if self.user.auth_identifier()?.as_i64() != Some(user_id) {
            return Err(Error::Unauthorized);
        }
        if let Some(audit) = &self.audit {
            audit.require_management_store(db)?;
        }
        self.send(mutation, Some(tx)).await?;
        if let Some(audit) = &self.audit {
            audit
                .record_management_in_transaction(tx, user_id, mutation)
                .await?;
        }
        Ok(())
    }
}

#[derive(Clone)]
enum FieldEncrypter {
    Instance(Arc<dyn ApplicationEncrypter>),
    Application(crate::container::Resolver),
}

/// 固定Fortify账户动作及同一公开加密器覆写优先级。
///
/// 官方动作提交后派发事件；受控 JWT 管理在因子事务内派发事件并运行应用选定的审计。
#[derive(Clone)]
pub struct TwoFactorActions {
    pub(super) provider: Arc<dyn TwoFactorAuthenticationProvider>,
    default_encrypter: FieldEncrypter,
    explicit_encrypter: Option<Arc<dyn ApplicationEncrypter>>,
    management_context: Option<(FactorManagementContext, bool)>,
    management_audit: Option<Arc<dyn super::api_audit::ApiFactorManagementAuditSink>>,
    pub(super) options: TwoFactorOptions,
    config: Option<crate::config::repository::ConfigRepository>,
    manual_feature_enabled: bool,
    pub(super) events: Events,
    pub(super) recovery_generator: Arc<dyn Fn() -> Result<String> + Send + Sync>,
}

impl fmt::Debug for TwoFactorActions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwoFactorActions")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl TwoFactorActions {
    pub(super) async fn default_encrypter(&self) -> Result<Arc<dyn ApplicationEncrypter>> {
        if let Some(encrypter) = &self.explicit_encrypter {
            return Ok(encrypter.clone());
        }
        match &self.default_encrypter {
            FieldEncrypter::Instance(encrypter) => Ok(encrypter.clone()),
            FieldEncrypter::Application(resolver) => {
                resolver.resolve::<dyn ApplicationEncrypter>().await
            }
        }
    }

    /// 创建包回退动作；发布配置需显式传入安装选项。
    pub fn new(
        provider: Arc<dyn TwoFactorAuthenticationProvider>,
        encrypter: Arc<dyn ApplicationEncrypter>,
        events: Events,
    ) -> Self {
        Self::build(provider, FieldEncrypter::Instance(encrypter), events)
    }

    pub(super) fn from_application(
        provider: Arc<dyn TwoFactorAuthenticationProvider>,
        resolver: crate::container::Resolver,
        events: Events,
    ) -> Self {
        Self::build(provider, FieldEncrypter::Application(resolver), events)
    }

    fn build(
        provider: Arc<dyn TwoFactorAuthenticationProvider>,
        default_encrypter: FieldEncrypter,
        events: Events,
    ) -> Self {
        Self {
            provider,
            default_encrypter,
            explicit_encrypter: None,
            management_context: None,
            management_audit: None,
            options: TwoFactorOptions::default(),
            config: None,
            manual_feature_enabled: true,
            events,
            recovery_generator: Arc::new(generate_recovery_code),
        }
    }

    /// 使用同一配置仓储求值后的选项。
    #[must_use]
    pub fn with_options(mut self, options: TwoFactorOptions) -> Self {
        self.options = options;
        self
    }

    /// 复用应用配置仓储；调用动作时读取选项，不固定首次解析的值。
    #[must_use]
    pub fn with_config_repository(
        mut self,
        config: crate::config::repository::ConfigRepository,
    ) -> Self {
        self.config = Some(config);
        self
    }

    /// 手工组装登录管线时选择是否执行双因素阶段；已绑定配置仓储时以官方功能清单为准。
    #[must_use]
    pub fn with_feature_enabled(mut self, enabled: bool) -> Self {
        self.manual_feature_enabled = enabled;
        self
    }

    pub(super) fn feature_enabled(&self) -> Result<bool> {
        self.config.as_ref().map_or(
            Ok(self.manual_feature_enabled),
            super::options::feature_enabled,
        )
    }

    /// 按当前官方功能清单及严格true选项判断确认要求，不解析无关密钥长度。
    pub fn confirms_two_factor_authentication(&self) -> Result<bool> {
        self.confirmation_required()
    }

    pub(super) fn configured_authority(
        &self,
        authority: &super::DatabaseFactorAuthority,
    ) -> super::DatabaseFactorAuthority {
        self.config.as_ref().map_or_else(
            || authority.clone(),
            |config| authority.clone().with_confirmation_config(config.clone()),
        )
    }

    pub(super) fn confirmation_required(&self) -> Result<bool> {
        self.config.as_ref().map_or(
            Ok(self.manual_feature_enabled && self.options.confirm),
            super::options::confirmation_required,
        )
    }

    pub(super) fn options_snapshot(&self) -> Result<TwoFactorOptions> {
        let Some(config) = &self.config else {
            return Ok(self.options.clone());
        };
        super::options::snapshot(config)
    }

    fn secret_length(&self) -> Result<usize> {
        self.config.as_ref().map_or(
            Ok(self.options.secret_length),
            super::options::secret_length,
        )
    }

    /// 对应Fortify显式加密器，优先于模型和默认Crypt。
    #[must_use]
    pub fn with_encrypter(mut self, encrypter: Arc<dyn ApplicationEncrypter>) -> Self {
        self.explicit_encrypter = Some(encrypter);
        self
    }

    /// 对应公开recoveryCodeGenerator扩展；失败时不写账户状态。
    #[must_use]
    pub fn with_recovery_code_generator(
        mut self,
        generator: Arc<dyn Fn() -> Result<String> + Send + Sync>,
    ) -> Self {
        self.recovery_generator = generator;
        self
    }

    /// 为手工装配的 JWT 管理动作选择同一数据库的事务审计。
    #[must_use]
    pub fn with_api_management_audit(
        mut self,
        audit: Arc<dyn super::api_audit::ApiFactorManagementAuditSink>,
    ) -> Self {
        self.management_audit = Some(audit);
        self
    }

    pub(crate) fn for_account(&self, model: &dyn TwoFactorAuthenticatable) -> Self {
        let mut actions = self.clone();
        if actions.explicit_encrypter.is_none() {
            actions.explicit_encrypter = model.two_factor_encrypter();
        }
        actions
    }

    fn api_management(&self) -> bool {
        self.management_context
            .as_ref()
            .is_some_and(|(context, _)| context.has_api_authorization())
    }

    fn management_event(&self, user: &AuthUser) -> ManagementEvent {
        ManagementEvent {
            user: user.clone(),
            events: self.events.clone(),
            audit: self.management_audit.clone(),
        }
    }

    async fn notify_after_write(
        &self,
        user: &AuthUser,
        mutation: FactorAuthorityMutation,
    ) -> Result<()> {
        if self.api_management() {
            return Ok(());
        }
        self.management_event(user).send(mutation, None).await
    }

    fn mutation_authority(
        &self,
        authority: &super::DatabaseFactorAuthority,
        user: &AuthUser,
    ) -> super::DatabaseFactorAuthority {
        let authority = self.configured_authority(authority);
        if self.api_management() {
            authority.with_management_event(self.management_event(user))
        } else {
            authority
        }
    }

    pub(crate) fn with_management_context(
        &self,
        context: FactorManagementContext,
        confirm: bool,
    ) -> Self {
        let mut actions = self.clone();
        actions.management_context = Some((context, confirm));
        actions
    }

    pub(super) async fn encrypter(
        &self,
        model: &dyn TwoFactorAuthenticatable,
    ) -> Result<Arc<dyn ApplicationEncrypter>> {
        if let Some(encrypter) = self
            .explicit_encrypter
            .clone()
            .or_else(|| model.two_factor_encrypter())
        {
            return Ok(encrypter);
        }
        self.default_encrypter().await
    }

    /// 有 `secret` 时默认幂等；`force` 重建密钥和恢复码，按官方保留 `confirmed_at`。
    pub async fn enable(&self, user: &AuthUser, force: bool) -> Result<()> {
        let model = capability(user)?;
        let authority = self.load_authority(model).await?;
        let before = match &authority {
            Some(snapshot) => snapshot.state.clone(),
            None => model.two_factor_state().await?,
        };
        if before
            .secret
            .as_ref()
            .is_some_and(|s| !s.is_empty() && s != "0")
            && !force
        {
            return Ok(());
        }
        let encrypter = self.encrypter(model).await?;
        let secret = Zeroizing::new(self.provider.generate_secret_key(self.secret_length()?)?);
        let replacement = TwoFactorState {
            secret: Some(encrypter.encrypt_serialized_string(secret.as_bytes())?),
            recovery_codes: Some(self.new_recovery_codes(encrypter.as_ref())?),
            confirmed_at: before.confirmed_at,
        };
        self.replace_authority(
            user,
            authority.as_ref(),
            &before,
            &replacement,
            FactorAuthorityMutation::Enable,
        )
        .await?;
        self.notify_after_write(user, FactorAuthorityMutation::Enable)
            .await
    }

    /// 验证当前secret并保存确认时间；无secret/空code仍是code字段校验失败。
    pub async fn confirm(&self, user: &AuthUser, code: &str) -> Result<()> {
        self.confirm_inner(user, code).await.map(|_| ())
    }

    pub(crate) async fn confirm_with_receipt(
        &self,
        user: &AuthUser,
        code: &str,
        guard_name: &str,
        provider_name: Option<&str>,
        provider: &dyn UserProvider,
    ) -> Result<Option<FactorReceipt>> {
        if let Some(authority) = capability(user)?.two_factor_authority() {
            authority.validate_account(user, provider).await?;
        }
        let (committed, verified_at) = self.confirm_inner(user, code).await?;
        let Some(snapshot) = committed else {
            return Ok(None);
        };
        FactorReceipt::issue(
            user.auth_identifier()?,
            guard_name.into(),
            provider_name.map(str::to_owned),
            snapshot.source,
            snapshot.generation,
            FactorVerificationMethod::Totp,
            verified_at,
        )
        .map(Some)
    }

    async fn confirm_inner(
        &self,
        user: &AuthUser,
        code: &str,
    ) -> Result<(Option<FactorAuthoritySnapshot>, u64)> {
        let model = capability(user)?;
        let authority = self.load_authority(model).await?;
        let before = match &authority {
            Some(snapshot) => snapshot.state.clone(),
            None => model.two_factor_state().await?,
        };
        if code.is_empty() || code == "0" {
            return Err(invalid_code());
        }
        let Some(ciphertext) = before
            .secret
            .as_ref()
            .filter(|s| !s.is_empty() && s.as_str() != "0")
        else {
            return Err(invalid_code());
        };
        let secret = decrypt_string(self.encrypter(model).await?.as_ref(), ciphertext)?;
        let verification = self.provider.verify_with_context(&secret, code).await?;
        if !verification.is_valid() {
            return Err(invalid_code());
        }
        let mut replacement = before.clone();
        replacement.confirmed_at = Some(Utc::now());
        let committed = match (model.two_factor_authority(), authority.as_ref()) {
            (Some(authority), Some(snapshot)) => {
                let authority = self
                    .mutation_authority(authority, user)
                    .with_provider_verification(&verification)?;
                Some(match &self.management_context {
                    Some((context, confirm)) => {
                        authority
                            .confirm_verified_with_management(
                                snapshot,
                                &replacement,
                                code,
                                context,
                                *confirm,
                            )
                            .await?
                    }
                    None => {
                        authority
                            .confirm_verified(snapshot, &replacement, code)
                            .await?
                    }
                })
            }
            (None, None) => {
                replace(model, &before, &replacement).await?;
                None
            }
            _ => return Err(Error::Conflict("因子authority来源已变化".into())),
        };
        let verified_at = u64::try_from(Utc::now().timestamp()).map_err(Error::internal)?;
        self.notify_after_write(user, FactorAuthorityMutation::Confirm)
            .await?;
        Ok((committed, verified_at))
    }

    /// 清除官方三项属性，不要求额外TOTP凭据。
    pub async fn disable(&self, user: &AuthUser) -> Result<()> {
        let model = capability(user)?;
        let authority = self.load_authority(model).await?;
        let before = match &authority {
            Some(snapshot) => snapshot.state.clone(),
            None => model.two_factor_state().await?,
        };
        if authority.is_none() && before == TwoFactorState::default() {
            return Ok(());
        }
        self.replace_authority(
            user,
            authority.as_ref(),
            &before,
            &TwoFactorState::default(),
            FactorAuthorityMutation::Disable,
        )
        .await?;
        self.notify_after_write(user, FactorAuthorityMutation::Disable)
            .await
    }

    /// 重新生成8个加密可重看的恢复码，不改变secret或确认时间。
    pub async fn generate_new_recovery_codes(&self, user: &AuthUser) -> Result<()> {
        let model = capability(user)?;
        let authority = self.load_authority(model).await?;
        let before = match &authority {
            Some(snapshot) => snapshot.state.clone(),
            None => model.two_factor_state().await?,
        };
        let mut replacement = before.clone();
        replacement.recovery_codes =
            Some(self.new_recovery_codes(self.encrypter(model).await?.as_ref())?);
        self.replace_authority(
            user,
            authority.as_ref(),
            &before,
            &replacement,
            FactorAuthorityMutation::Regenerate,
        )
        .await?;
        self.notify_after_write(user, FactorAuthorityMutation::Regenerate)
            .await
    }

    fn new_recovery_codes(&self, encrypter: &dyn ApplicationEncrypter) -> Result<String> {
        let codes = Zeroizing::new(
            (0..8)
                .map(|_| (self.recovery_generator)())
                .collect::<Result<Vec<_>>>()?,
        );
        let json = Zeroizing::new(serde_json::to_vec(&*codes).map_err(Error::internal)?);
        encrypter.encrypt_serialized_string(&json)
    }

    /// 受保护管理入口重读当前恢复码；未启用或无恢复码返回空数组。
    pub async fn recovery_codes(&self, user: &AuthUser) -> Result<Zeroizing<Vec<String>>> {
        let model = capability(user)?;
        let state = match self.load_authority(model).await? {
            Some(snapshot) => snapshot.state,
            None => model.two_factor_state().await?,
        };
        if state.secret.as_ref().is_none_or(String::is_empty) {
            return Ok(Zeroizing::new(Vec::new()));
        }
        match state.recovery_codes.as_ref().filter(|s| !s.is_empty()) {
            None => Ok(Zeroizing::new(Vec::new())),
            Some(ciphertext) => {
                let json = self
                    .encrypter(model)
                    .await?
                    .decrypt_serialized_string(ciphertext)?;
                Ok(Zeroizing::new(
                    serde_json::from_slice(&json).map_err(Error::internal)?,
                ))
            }
        }
    }

    /// 读取当前手动密钥；未启用按官方secret-key端点返回404。
    pub async fn secret_key(&self, user: &AuthUser) -> Result<Zeroizing<String>> {
        let model = capability(user)?;
        let state = match self.load_authority(model).await? {
            Some(snapshot) => snapshot.state,
            None => model.two_factor_state().await?,
        };
        let ciphertext = state.secret.as_ref().ok_or_else(|| {
            Error::NotFound("Two factor authentication has not been enabled.".into())
        })?;
        decrypt_string(self.encrypter(model).await?.as_ref(), ciphertext)
    }

    /// 生成本地二维码URI；尚无secret时返回None，由HTTP入口返回空数组。
    pub async fn qr_code_url(
        &self,
        user: &AuthUser,
        company: &str,
        username: &str,
    ) -> Result<Option<Zeroizing<String>>> {
        let model = capability(user)?;
        let state = match self.load_authority(model).await? {
            Some(snapshot) => snapshot.state,
            None => model.two_factor_state().await?,
        };
        let Some(ciphertext) = state.secret.as_ref() else {
            return Ok(None);
        };
        let secret = decrypt_string(self.encrypter(model).await?.as_ref(), ciphertext)?;
        let label = model.two_factor_account_name(username)?.unwrap_or_default();
        Ok(Some(Zeroizing::new(
            self.provider.qr_code_url(company, &label, &secret)?,
        )))
    }
}

pub(super) fn capability(user: &AuthUser) -> Result<&dyn TwoFactorAuthenticatable> {
    user.as_two_factor().ok_or_else(|| {
        Error::custom(
            "fortify_two_factor_model_unsupported",
            "当前账户模型未提供双因素能力",
        )
    })
}

impl TwoFactorActions {
    async fn load_authority(
        &self,
        model: &dyn TwoFactorAuthenticatable,
    ) -> Result<Option<FactorAuthoritySnapshot>> {
        let Some(authority) = model.two_factor_authority() else {
            if self.api_management() {
                return Err(Error::Unauthorized);
            }
            return Ok(None);
        };
        let authority = self.configured_authority(authority);
        let snapshot = authority.load().await?;
        if let Some((context, confirm)) = &self.management_context {
            authority
                .authorize_management(&snapshot, context, *confirm)
                .await?;
        }
        Ok(Some(snapshot))
    }
    async fn replace_authority(
        &self,
        user: &AuthUser,
        snapshot: Option<&FactorAuthoritySnapshot>,
        before: &TwoFactorState,
        replacement: &TwoFactorState,
        mutation: FactorAuthorityMutation,
    ) -> Result<Option<FactorAuthoritySnapshot>> {
        let model = capability(user)?;
        match (model.two_factor_authority(), snapshot) {
            (Some(authority), Some(snapshot)) => match &self.management_context {
                Some((context, confirm)) => self
                    .mutation_authority(authority, user)
                    .replace_with_management(snapshot, replacement, mutation, context, *confirm)
                    .await
                    .map(Some),
                None => authority
                    .replace(snapshot, replacement, mutation)
                    .await
                    .map(Some),
            },
            (None, None) => replace(model, before, replacement).await.map(|()| None),
            _ => Err(Error::Conflict("因子authority来源已变化".into())),
        }
    }
}

pub(super) async fn replace(
    model: &dyn TwoFactorAuthenticatable,
    before: &TwoFactorState,
    replacement: &TwoFactorState,
) -> Result<()> {
    if !model
        .compare_and_set_two_factor_state(before, replacement)
        .await?
    {
        return Err(Error::Conflict("双因素状态已变化，请重试当前操作".into()));
    }
    Ok(())
}

pub(super) fn decrypt_string(
    encrypter: &dyn ApplicationEncrypter,
    ciphertext: &str,
) -> Result<Zeroizing<String>> {
    let plaintext = encrypter.decrypt_serialized_string(ciphertext)?;
    std::str::from_utf8(&plaintext)
        .map(|value| Zeroizing::new(value.to_owned()))
        .map_err(|_| Error::custom("fortify_secret_encoding_invalid", "双因素密文内容不是UTF-8"))
}

fn invalid_code() -> Error {
    let mut errors = ValidationErrors::new();
    errors.add(
        "code",
        "The provided two factor authentication code was invalid.",
    );
    Error::validation_in_bag("confirmTwoFactorAuthentication", errors)
}

/// 按固定RecoveryCode生成两个10字符随机字母数字段，用连字符连接。
pub fn generate_recovery_code() -> Result<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut output = String::with_capacity(21);
    while output.len() < 21 {
        if output.len() == 10 {
            output.push('-');
            continue;
        }
        let mut byte = [0u8; 1];
        getrandom::fill(&mut byte).map_err(Error::internal)?;
        if byte[0] < 248 {
            output.push(char::from(ALPHABET[usize::from(byte[0] % 62)]));
        }
    }
    Ok(output)
}

#[cfg(test)]
#[path = "../../../tests/unit/auth/fortify_actions.rs"]
mod tests;
