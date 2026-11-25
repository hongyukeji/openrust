/// 接收扩展错误；显示信息不包含正文、签名或密钥。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Webhook 配置无效：{0}")]
    InvalidConfig(&'static str),
    #[error("Webhook 正文超过限制")]
    PayloadTooLarge,
    #[error("Webhook 请求头超过限制")]
    HeadersTooLarge,
    #[error("Webhook 签名无效")]
    InvalidSignature,
    #[error("Webhook 运行操作失败")]
    Runtime {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
pub type Result<T> = std::result::Result<T, Error>;
