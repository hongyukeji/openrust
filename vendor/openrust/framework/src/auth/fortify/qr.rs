//! 本地二维码编码；不将账户密钥交给外部图像服务。

use qrcode::{Color, EcLevel, QrCode};

use crate::{Error, Result};

/// 将双因素URI编码为192px、无外边距的本地SVG。
///
/// 颜色与固定Fortify输出一致；SVG路径编码矩阵，不插入未转义的URI/XML文本。
/// 返回值包含可还原的账户密钥，只能由受保护管理端点交付。
pub fn two_factor_qr_code_svg(uri: &str) -> Result<String> {
    let code = QrCode::with_error_correction_level(uri.as_bytes(), EcLevel::L)
        .map_err(|_| Error::custom("fortify_qr_encoding_failed", "双因素URI无法编码为二维码"))?;
    let width = code.width();
    let mut path = String::new();
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] == Color::Dark {
                use std::fmt::Write;
                write!(path, "M{x} {y}h1v1h-1z").map_err(Error::internal)?;
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"192\" height=\"192\" viewBox=\"0 0 {width} {width}\" shape-rendering=\"crispEdges\"><path fill=\"#ffffff\" d=\"M0 0h{width}v{width}H0z\"/><path fill=\"#2d3748\" d=\"{path}\"/></svg>"
    ))
}

#[cfg(test)]
mod tests {
    use super::two_factor_qr_code_svg;

    #[test]
    fn renderer_keeps_payload_out_of_xml_text_and_uses_official_size() -> crate::Result<()> {
        let svg = two_factor_qr_code_svg(
            "otpauth://totp/测试:a%3Cb?secret=JBSWY3DPEHPK3PXP&issuer=测试",
        )?;
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains("width=\"192\" height=\"192\""));
        assert!(svg.contains("#2d3748"));
        assert!(!svg.contains("JBSWY3DPEHPK3PXP"));
        assert!(!svg.contains("otpauth:"));
        assert!(!svg.contains("<script"));
        Ok(())
    }

    /// 仅显式执行时导出公开测试数据，供独立浏览器与Vision解码，不使用账户密钥。
    #[test]
    #[ignore = "显式导出公开SVG互操作fixture；浏览器/Swift由总控串行运行"]
    fn export_public_qr_interoperability_fixture() -> crate::Result<()> {
        use crate::auth::fortify::{
            GoogleTwoFactorAuthenticationProvider, TwoFactorAuthenticationProvider,
        };
        use std::path::PathBuf;
        let provider = GoogleTwoFactorAuthenticationProvider::new(None);
        let uri = provider.qr_code_url(
            "OpenRust Test",
            "public-fixture@example.test",
            "JBSWY3DPEHPK3PXP",
        )?;
        let svg = two_factor_qr_code_svg(&uri)?;
        // 导出归调用者选择的证据目录，独立框架包不推断宿主仓库根路径。
        let directory = std::env::var_os("OPENRUST_TEST_ARTIFACT_DIR").map_or_else(
            || std::env::temp_dir().join(format!("openrust-qr-fixture-{}", std::process::id())),
            PathBuf::from,
        );
        std::fs::create_dir_all(&directory).map_err(crate::Error::internal)?;
        std::fs::write(directory.join("fixture.svg"), svg).map_err(crate::Error::internal)?;
        std::fs::write(directory.join("fixture.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "source": "public synthetic test fixture", "uri": uri,
            "secret": "JBSWY3DPEHPK3PXP", "declared_svg_size": 192,
            "scope": "SVG→real browser PNG→independent Apple Vision decode; not Google Authenticator import",
        })).map_err(crate::Error::internal)?).map_err(crate::Error::internal)?;
        Ok(())
    }
}
