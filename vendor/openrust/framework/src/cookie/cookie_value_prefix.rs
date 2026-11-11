use aws_lc_rs::hmac;
use subtle::{Choice, ConstantTimeEq as _};

const PREFIX_BYTES: usize = 41;
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Binds an encrypted cookie payload to its cookie name.
pub struct CookieValuePrefix;

impl CookieValuePrefix {
    /// Creates Laravel's hexadecimal HMAC-SHA1 prefix and separator.
    pub fn create(cookie_name: &str, key: &[u8]) -> [u8; PREFIX_BYTES] {
        let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
        let mut context = hmac::Context::with_key(&key);
        context.update(cookie_name.as_bytes());
        context.update(b"v2");
        let digest = context.sign();
        let mut prefix = [b'|'; PREFIX_BYTES];
        let (hex_chunks, _) = prefix.as_chunks_mut::<2>();
        for (byte, output) in digest.as_ref().iter().zip(hex_chunks) {
            output[0] = HEX[usize::from(byte >> 4)];
            output[1] = HEX[usize::from(byte & 0x0f)];
        }
        prefix
    }

    /// Removes the prefix from a value that has already been authenticated.
    pub fn remove(cookie_value: &[u8]) -> &[u8] {
        cookie_value.get(PREFIX_BYTES..).unwrap_or_default()
    }

    /// Verifies the name binding using the current key and any previous keys.
    pub fn validate<'a, 'key>(
        cookie_name: &str,
        cookie_value: &'a [u8],
        keys: impl IntoIterator<Item = &'key [u8]>,
    ) -> Option<&'a [u8]> {
        let (prefix, value) = cookie_value.split_at_checked(PREFIX_BYTES)?;
        let mut matches = Choice::from(0);
        for key in keys {
            matches |= prefix.ct_eq(&Self::create(cookie_name, key));
        }
        bool::from(matches).then_some(value)
    }
}
