//! 通过模块内显式生成脚本产生的 gRPC Protobuf 类型。

#[allow(clippy::pedantic)]
pub mod echo {
    include!("generated/echo.rs");
}
