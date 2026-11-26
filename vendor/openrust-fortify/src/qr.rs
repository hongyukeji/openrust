//! 本地二维码编码；不将账户密钥交给外部图像服务。

use qrcode::{Color, EcLevel, QrCode};

use crate::{FortifyError, FortifyResult};

/// 将双因素URI编码为192px、无外边距的本地SVG。
///
/// 颜色与固定Fortify输出一致；SVG路径编码矩阵，不插入未转义的URI/XML文本。
/// 返回值包含可还原的账户密钥，只能由受保护管理端点交付。
pub fn two_factor_qr_code_svg(uri: &str) -> FortifyResult<String> {
    let code = QrCode::with_error_correction_level(uri.as_bytes(), EcLevel::L)
        .map_err(|_| FortifyError::QrEncoding)?;
    let width = code.width();
    let mut path = String::new();
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] == Color::Dark {
                use std::fmt::Write;
                write!(path, "M{x} {y}h1v1h-1z").expect("writing a string");
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"192\" height=\"192\" viewBox=\"0 0 {width} {width}\" shape-rendering=\"crispEdges\"><path fill=\"#ffffff\" d=\"M0 0h{width}v{width}H0z\"/><path fill=\"#2d3748\" d=\"{path}\"/></svg>"
    ))
}
