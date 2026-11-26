//! PHP crypt 的历史 `$2x$` 有符号字节兼容，仅用于验证既有哈希，不生成新密码哈希。
//!
//! 算法对照固定 PHP 8.5.0 `ext/standard/crypt_blowfish.c` 的 `BF_set_key`：
//! `flags_by_subtype` 对 x 选择 bug=1、safety=0，每个字节先转 signed char 再扩展至 u32。
//! 锁定 bcrypt0.19.3 的底层每次仅读取18个P-array key words，
//! 因此将这些历史 words 编码为 72 个无符号字节，即可复用成熟安全实现的全部轮函数。

/// 核验历史 `$2x$`；调用方须先执行成本预算及共享阻塞任务容量检查。
///
/// 成熟 bcrypt 实现直接接收固定72字节key，不追加或丢弃key中的NUL；
/// 盐、Eks轮函数、编码及23字节恒时比较均复用该实现。
pub(super) fn verify(value: &[u8], encoded: &str) -> bool {
    if !encoded.starts_with("$2x$") || encoded.len() != 60 {
        return false;
    }
    let key = zeroize::Zeroizing::new(signed_key(value));
    bcrypt::verify(key.as_slice(), encoded).is_ok_and(|matched| matched)
}

fn signed_key(value: &[u8]) -> [u8; 72] {
    let length = value
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(value.len())
        .min(72);
    let mut key = [0u8; 72];
    let mut position = 0;
    for word in key.as_chunks_mut::<4>().0 {
        let mut expanded = 0u32;
        for _ in 0..4 {
            let byte = if position < length {
                value[position]
            } else {
                0
            };
            expanded = (expanded << 8)
                | u32::from_ne_bytes(i32::from(i8::from_ne_bytes([byte])).to_ne_bytes());
            position += 1;
            if position > length {
                position = 0;
            }
        }
        word.copy_from_slice(&expanded.to_be_bytes());
    }
    key
}
