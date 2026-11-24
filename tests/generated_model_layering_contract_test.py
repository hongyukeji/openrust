"""校验分层断言能拒绝越界改动，并允许保留既有用户定制。"""
import unittest
from pathlib import Path
from generated_model_layering_contract import CAPABILITIES, assert_model, assert_user_delta, assert_installed_user

BASE = '''use openrust::database::eloquent::{Model, ModelState};
struct User(ModelState);
impl Model for User {}
impl User { fn marker() -> &'static str { "custom" } }
'''


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

    def test_existing_storage_customization_is_preserved_instead_of_rewritten(self):
        custom = BASE + 'fn legacy() { Db::connection(None).table("legacy"); }'
        assert_user_delta(custom, custom + CAPABILITIES['sanctum'], 'sanctum')
        with self.assertRaises(AssertionError):
            assert_user_delta(custom, BASE + CAPABILITIES['sanctum'], 'sanctum')

    def test_required_capabilities_and_fortify_metadata_are_not_optional(self):
        for phase in ['sanctum', 'passport', 'fortify']:
            with self.assertRaises(AssertionError):
                assert_installed_user(BASE, phase)
        assert_installed_user(BASE + CAPABILITIES['sanctum'], 'sanctum')
        assert_installed_user(BASE + CAPABILITIES['passport'], 'passport')
        fortify = BASE.replace('struct User', '#[derive(Clone)] pub(crate) struct User')
        fortify += CAPABILITIES['fortify-2fa'] + CAPABILITIES['fortify-passkeys']
        with self.assertRaises(AssertionError):
            assert_installed_user(fortify, 'fortify')
        assert_installed_user(fortify + 'fn fields() { hidden(&["password", "two_factor_secret", "two_factor_recovery_codes"]); }', 'fortify')

    def test_strings_and_raw_strings_are_preserved(self):
        custom = BASE + 'fn urls() { let a = "https://example.test"; let b = r#"Db::table // text"#; }'
        assert_user_delta(custom, custom + CAPABILITIES['sanctum'], 'sanctum')
        with self.assertRaises(AssertionError):
            assert_user_delta(custom, custom.replace('example.test', 'changed.test') + CAPABILITIES['sanctum'], 'sanctum')

    def test_foreign_capability_and_duplicate_are_rejected(self):
        with self.assertRaises(AssertionError):
            assert_user_delta(BASE, BASE + CAPABILITIES['passport'], 'sanctum')
        with self.assertRaises(AssertionError):
            assert_user_delta(BASE, BASE + CAPABILITIES['sanctum'] * 2, 'sanctum')


if __name__ == '__main__':
    unittest.main()
