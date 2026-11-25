/// 发送扩展错误，不包含密钥或原始正文。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Webhook 发送配置无效：{0}")]
    InvalidConfig(&'static str),
    #[error("Webhook 正文不符合签名协议")]
    InvalidPayload,
}
pub type Result<T> = std::result::Result<T, Error>;
