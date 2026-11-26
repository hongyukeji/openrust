use crate::Fortify;
use openrust::support::Str;

pub type RecoveryCodeGenerator = dyn Fn() -> String + Send + Sync;
pub struct RecoveryCode;
impl RecoveryCode {
    pub fn generate(fortify: &Fortify) -> String {
        if let Some(callback) = fortify.recovery_code_generator() {
            return callback();
        }
        format!("{}-{}", Str::random(10), Str::random(10))
    }
}
/// The default PHP json_encode string-array format, including slash and UTF-16 escapes.
pub(crate) fn encode(codes: &[String]) -> String {
    let mut json = String::from("[");
    for (index, code) in codes.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        let encoded = serde_json::to_string(code).expect("String JSON serialization");
        for character in encoded.chars() {
            if character == '/' {
                json.push_str("\\/");
            } else if character.is_ascii() {
                json.push(character);
            } else {
                let mut buffer = [0; 2];
                for unit in character.encode_utf16(&mut buffer) {
                    use std::fmt::Write as _;
                    write!(json, "\\u{unit:04x}").expect("String formatting");
                }
            }
        }
    }
    json.push(']');
    json
}
