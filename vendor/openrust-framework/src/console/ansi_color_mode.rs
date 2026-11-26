use super::ConsoleFormatError;
/// ANSI调色能力；转换结果由前景或背景前缀补齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnsiColorMode {
    Ansi4,
    Ansi8,
    Ansi24,
}
impl AnsiColorMode {
    pub fn convert_from_hex_to_ansi_color_code(
        self,
        value: &str,
    ) -> Result<String, ConsoleFormatError> {
        let mut hex = value.replace('#', "");
        if hex.len() == 3 {
            hex = hex.chars().flat_map(|ch| [ch, ch]).collect();
        }
        if hex.len() != 6 {
            return Err(ConsoleFormatError::InvalidColor(value.to_owned()));
        }
        // 数值转换忽略非十六进制字符，保留既有转换约定。
        let digits: String = hex.chars().filter(char::is_ascii_hexdigit).collect();
        let rgb = if digits.is_empty() {
            0
        } else {
            u32::from_str_radix(&digits, 16).expect("六位十六进制值有效")
        };
        let (r, g, b) = ((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255);
        Ok(match self {
            Self::Ansi24 => format!("8;2;{r};{g};{b}"),
            Self::Ansi8 => {
                let index = if r == g && g == b {
                    if r < 8 {
                        16
                    } else if r > 248 {
                        231
                    } else {
                        232 + ((r - 8) * 24 + 123) / 247
                    }
                } else {
                    16 + 36 * ((r * 5 + 127) / 255)
                        + 6 * ((g * 5 + 127) / 255)
                        + (b * 5 + 127) / 255
                };
                format!("8;5;{index}")
            }
            Self::Ansi4 => {
                ((u32::from(b >= 128) << 2) | (u32::from(g >= 128) << 1) | u32::from(r >= 128))
                    .to_string()
            }
        })
    }
}
