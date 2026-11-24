"""分层断言的正反例；语法由 proc_macro2／syn 解析，不以正则模拟 Rust。"""
import unittest
from pathlib import Path
from generated_model_layering_contract import CAPABILITIES, assert_model, assert_user_delta, assert_installed_user

BASE = '''use openrust::database::eloquent::{Model, ModelState};
pub(crate) struct User(ModelState);
impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        // 中文注释用于验证源码 span 的字节位置。
        static METADATA: ModelMetadata = ModelMetadata::new("users").fillable(&["name"]).hidden(&["password"]);
        &METADATA
    }
}
impl User { fn marker() -> &'static str { "custom" } }
'''


def fortify(source=BASE):
    return (source.replace('pub(crate) struct User', '#[derive(Clone)] pub(crate) struct User')
            .replace('.hidden(&["password"])', '.hidden(&["password", "two_factor_secret", "two_factor_recovery_codes"])')
            + CAPABILITIES['fortify-2fa'] + CAPABILITIES['fortify-passkeys'])


class LayeringContract(unittest.TestCase):
    def test_shipped_default_model_stub_and_fortify_publication_paths(self):
        root = Path(__file__).resolve().parents[1]
        assert_model((root / 'app/models/user.rs').read_text(), '默认 User')
        assert_model((root / 'vendor/openrust/framework/src/foundation/console/stubs/model.rs.stub').read_text(), '内置模型模板')
        resources = (root / 'vendor/openrust/fortify/src/resources.rs').read_text()
        self.assertNotIn('"app/models/', resources)
        for name in ['create_new_user', 'reset_user_password', 'update_user_password', 'update_user_profile_information']:
            self.assertIn(f'"app/actions/fortify/{name}.rs"', resources)

    def test_models_allow_state_and_comments_but_reject_storage_and_workflows(self):
        assert_model(BASE + '// Db::connection and .table() are documentation\n', '默认模型')
        for code in ['Db::connection(None)', 'x.table("users")', 'x.transaction(work)',
                     'x.encrypt(value)', 'Event::dispatch(event)', 'Hash::make(password)']:
            with self.subTest(code=code), self.assertRaises(AssertionError):
                assert_model(BASE + f'fn injected() {{ {code}; }}', '越界模型')

    def test_character_literal_does_not_swallow_database_access(self):
        for literal in [r"'\"'", "'\"'", r"'\\'", "'中'", 'b\'"\'']:
            with self.subTest(literal=literal), self.assertRaises(AssertionError):
                assert_model(BASE + f'fn injected() {{ let quote = {literal}; Db::connection(None).table("users"); }}', '字符边界')
        assert_model(BASE + r'''fn text() { let quote = '"'; let escaped = '\\'; let text = r##"Db::connection // text"##; }''', '合法字符与原始字符串')

    def test_nested_comments_are_not_executable_code(self):
        assert_model(BASE + '/* 外层 /* 内层 */ Db::connection(None).table("users"); */', '嵌套注释')
        with self.assertRaises(AssertionError):
            assert_model(BASE + '/* 外层 /* 内层 */ 注释 */ fn bad() { Db::connection(None); }', '注释外代码')

    def test_installers_allow_only_their_declared_model_edits(self):
        for name, phase in [('sanctum', 'sanctum'), ('passport', 'passport'), ('fortify-2fa', 'fortify')]:
            with self.subTest(phase=phase):
                installed = BASE + CAPABILITIES[name]
                assert_user_delta(BASE, installed, phase)
                assert_user_delta(installed, installed, phase)
                with self.assertRaises(AssertionError):
                    assert_user_delta(BASE, installed + 'fn injected() { db.table("users"); }', phase)
                with self.assertRaises(AssertionError):
                    assert_user_delta(BASE, installed.replace('"custom"', '"changed"'), phase)
        assert_user_delta(BASE, fortify(), 'fortify')

    def test_existing_storage_customization_is_preserved_instead_of_rewritten(self):
        custom = BASE + 'fn legacy() { Db::connection(None).table("legacy"); }'
        assert_user_delta(custom, custom + CAPABILITIES['sanctum'], 'sanctum')
        with self.assertRaises(AssertionError):
            assert_user_delta(custom, BASE + CAPABILITIES['sanctum'], 'sanctum')

    def test_required_capabilities_and_real_metadata(self):
        for phase in ['sanctum', 'passport', 'fortify']:
            with self.assertRaises(AssertionError):
                assert_installed_user(BASE, phase)
        assert_installed_user(BASE + CAPABILITIES['sanctum'], 'sanctum')
        assert_installed_user(BASE + CAPABILITIES['passport'], 'passport')
        assert_installed_user(fortify(), 'fortify')

    def test_sensitive_fields_in_fillable_do_not_count_as_hidden(self):
        wrong = fortify().replace('.hidden(&["password", "two_factor_secret", "two_factor_recovery_codes"])', '.hidden(&["password"])')
        wrong = wrong.replace('.fillable(&["name"])', '.fillable(&["name", "two_factor_secret", "two_factor_recovery_codes"])')
        with self.assertRaises(AssertionError):
            assert_user_delta(BASE, wrong, 'fortify')
        with self.assertRaises(AssertionError):
            assert_installed_user(wrong, 'fortify')

    def test_unrelated_hidden_declarations_are_not_user_metadata(self):
        plain = fortify().replace('.hidden(&["password", "two_factor_secret", "two_factor_recovery_codes"])', '.hidden(&["password"])')
        candidates = [
            plain + 'fn fields() { hidden(&["two_factor_secret", "two_factor_recovery_codes"]); }',
            plain + 'struct Other; impl Model for Other { fn metadata() { hidden(&["two_factor_secret", "two_factor_recovery_codes"]); } }',
            plain.replace('static METADATA:', 'static UNUSED: ModelMetadata = ModelMetadata::new("unused").hidden(&["two_factor_secret", "two_factor_recovery_codes"]); static METADATA:'),
            fortify().replace(';\n        &METADATA', '.hidden(&["password"]);\n        &METADATA'),
        ]
        for source in candidates:
            with self.subTest(source=source), self.assertRaises(AssertionError):
                assert_installed_user(source, 'fortify')
            with self.assertRaises(AssertionError):
                assert_user_delta(BASE, source, 'fortify')

    def test_strings_raw_strings_and_lifetimes_are_preserved(self):
        custom = BASE + '''fn urls<'a>(x: &'a str) -> &'a str { let a = "https://example.test"; let b = r#"Db::table // text"#; x }'''
        assert_user_delta(custom, custom + CAPABILITIES['sanctum'], 'sanctum')
        with self.assertRaises(AssertionError):
            assert_user_delta(custom, custom.replace('example.test', 'changed.test') + CAPABILITIES['sanctum'], 'sanctum')
        assert_installed_user(fortify().replace('"two_factor_secret"', 'r#"two_factor_secret"#'), 'fortify')

    def test_foreign_capability_duplicate_and_invalid_syntax_are_rejected(self):
        with self.assertRaises(AssertionError):
            assert_user_delta(BASE, BASE + CAPABILITIES['passport'], 'sanctum')
        with self.assertRaises(AssertionError):
            assert_user_delta(BASE, BASE + CAPABILITIES['sanctum'] * 2, 'sanctum')
        with self.assertRaises(AssertionError):
            assert_model(BASE + 'fn invalid( {', '无效语法')


if __name__ == '__main__':
    unittest.main()
