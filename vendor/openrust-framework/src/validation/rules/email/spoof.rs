use intl::unicode::{Script, script_extensions, spoof::is_single_script};
/// 复用UTS39脚本扩展与增广脚本解析，执行默认高度限制等级。
pub(super) fn passes(value: &str) -> bool {
    if is_single_script(value) {
        return true;
    }
    let without_latin: String = value
        .chars()
        .filter(|ch| !script_extensions(*ch).contains(Script::Latin))
        .collect();
    is_single_script(&without_latin)
        && without_latin.chars().all(|ch| {
            script_extensions(ch).iter().any(|script| {
                matches!(
                    script,
                    Script::Common
                        | Script::Inherited
                        | Script::Han
                        | Script::Hiragana
                        | Script::Katakana
                        | Script::Bopomofo
                        | Script::Hangul
                )
            })
        })
}
