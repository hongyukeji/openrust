"""核对十八语标准资源的精确键、占位符、JSON结构及来源，不使用运行时回退。"""
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
LOCALES = ('en', 'zh-Hans', 'zh-Hant', 'ja', 'ko', 'fr', 'de', 'es', 'pt', 'it', 'ru', 'ar', 'hi', 'id', 'vi', 'th', 'tr', 'nl')
GROUPS = ('auth', 'passwords', 'pagination', 'validation')


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f'重复JSON键：{key}')
        result[key] = value
    return result


def read(locale, group):
    return json.loads((ROOT / 'vendor/openrust/framework/src/localization/lang' / locale / f'{group}.json').read_text(), object_pairs_hook=unique)


def leaves(value, prefix=''):
    result = {}
    for key, item in value.items():
        name = f'{prefix}.{key}'.strip('.')
        if isinstance(item, dict):
            result.update(leaves(item, name))
        else:
            result[name] = item
    return result


class StandardResources(unittest.TestCase):
    def test_each_locale_has_exact_standard_keys_and_replacement_parameters(self):
        for group in GROUPS:
            baseline = leaves(read('en', group))
            for locale in LOCALES:
                with self.subTest(locale=locale, group=group):
                    translated = leaves(read(locale, group))
                    self.assertEqual(set(baseline), set(translated))
                    for key, value in translated.items():
                        self.assertIsInstance(value, str, key)
                        self.assertTrue(value.strip(), key)
                        # 框架允许 :attribute／:Attribute／:ATTRIBUTE 三种大小写替换。
                        tokens = lambda text: set(re.findall(r':[a-z_][a-z_0-9]*', text.lower()))
                        self.assertEqual(tokens(baseline[key]), tokens(value), key)
                    if group == 'validation':
                        self.assertEqual(read(locale, group)['custom'], {})
                        self.assertEqual(read(locale, group)['attributes'], {})

    def test_complete_application_groups_and_sentence_catalogs(self):
        for locale in LOCALES:
            for group in ('welcome', 'console', 'schema'):
                with self.subTest(locale=locale, group=group):
                    baseline = leaves(read('en', group))
                    translated = leaves(read(locale, group))
                    self.assertEqual(set(baseline), set(translated))
                    for key, value in translated.items():
                        self.assertIsInstance(value, str)
                        self.assertTrue(value.strip())
                        tokens = lambda text: set(re.findall(r':[A-Za-z_][A-Za-z_0-9]*', text))
                        self.assertEqual(tokens(baseline[key]), tokens(value), key)
            sentence = json.loads((ROOT / 'vendor/openrust/framework/src/localization/lang' / f'{locale}.json').read_text(), object_pairs_hook=unique)
            english = json.loads((ROOT / 'vendor/openrust/framework/src/localization/lang/en.json').read_text())
            self.assertEqual(set(sentence), set(english))
            for key, value in sentence.items():
                self.assertIsInstance(value, str)
                self.assertTrue(value.strip())
                self.assertEqual(set(re.findall(r':[A-Za-z_][A-Za-z_0-9]*', key)), set(re.findall(r':[A-Za-z_][A-Za-z_0-9]*', value)), (locale, key))
            name = read(locale, 'languages')
            self.assertTrue(name['name'])
            self.assertEqual(name.get('direction', 'ltr'), 'rtl' if locale == 'ar' else 'ltr')

    def test_bundled_catalog_indexes_exactly_the_canonical_resources(self):
        directory = ROOT / 'vendor/openrust/framework/src/localization/lang'
        source = (directory.parent / 'bundled.rs').read_text()
        included = re.findall(r'include_str!\("lang/([^"\n]+)"\)', source)
        expected = {str(p.relative_to(directory)) for p in directory.rglob('*.json') if p.name != 'standard-resources.source.json'}
        self.assertEqual(set(included), expected)
        self.assertEqual(len(included), len(expected))

    def test_preserves_the_upstream_license_and_pinned_source(self):
        self.assertIn('Permission is hereby granted, free of charge', (ROOT / 'vendor/openrust/framework/src/localization/stubs/NOTICE').read_text())
        metadata = json.loads((ROOT / '.agents/project/audits/03-07/localization-18-source.json').read_text())['standard_source']
        self.assertEqual(metadata['commit'], '3fc62be68ac6d462f6a679944526fbddf5fced09')
        self.assertEqual(set(metadata['locales']), set(LOCALES))


if __name__ == '__main__':
    unittest.main()
