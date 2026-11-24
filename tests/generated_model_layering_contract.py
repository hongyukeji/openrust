"""生成产物的分层契约；只检查输出，不参与生成或修改应用。"""
import functools
import json
import os
from pathlib import Path
import re
import subprocess


@functools.lru_cache(maxsize=1)
def parser_binary():
    override = os.environ.get('OPENRUST_MODEL_LAYERING_PARSER')
    if override:
        path = Path(override)
        assert path.is_file(), 'Rust 解析器不存在'
        return str(path)
    root = Path(__file__).resolve().parents[1]
    base = root.parent
    assert base.parent.name == 'tmp' and base.parent.parent.name == 'target', '请通过 build_guard 在项目隔离快照中准备 Rust 解析器'
    assert os.environ.get('RUSTUP_TOOLCHAIN') == '1.94.0'
    assert Path(os.environ['CARGO_TARGET_DIR']).resolve().is_relative_to(base)
    with (base / 'generated-layering-parser-build.log').open('w') as log:
        result = subprocess.run(['cargo', 'test', '--locked', '-p', 'openrust-framework', '--test', 'model_layering_parser', '--no-run', '--message-format=json'],
                                cwd=root, env=dict(os.environ, CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0'), capture_output=True, text=True)
        log.write(result.stdout + result.stderr)
    assert result.returncode == 0, 'Rust 解析器编译失败，见 generated-layering-parser-build.log'
    for line in result.stdout.splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get('reason') == 'compiler-artifact' and value.get('target', {}).get('name') == 'model_layering_parser' and value.get('executable'):
            return value['executable']
    raise AssertionError('Cargo 未返回 Rust 解析器路径')


@functools.lru_cache(maxsize=128)
def inspect(source, parse_file=True):
    result = subprocess.run([parser_binary(), '--exact', 'parse_request', '--nocapture'],
                            input=json.dumps({'source': source, 'parse_file': parse_file}), text=True, capture_output=True,
                            env=dict(os.environ, OPENRUST_MODEL_LAYERING_PARSE='1'))
    assert result.returncode == 0, f'Rust 解析器执行失败：{result.stderr}'
    marker = 'MODEL_LAYERING_JSON '
    assert marker in result.stdout, 'Rust 解析器未返回结果'
    value = json.loads(result.stdout.split(marker, 1)[1].splitlines()[0])
    assert 'error' not in value, f'Rust 语法解析失败：{value.get("error")}'
    return value


def tokens(source):
    return inspect(source, False)['tokens']


CAPABILITIES = {
    'sanctum': '#[cfg(feature = "sanctum")] impl openrust::sanctum::HasApiTokens for User {}',
    'passport': '#[cfg(feature = "passport")] impl openrust_passport::HasApiTokens for User { fn get_provider_name(&self) -> &str { "users" } }',
    'fortify-2fa': '#[cfg(feature = "fortify")] impl openrust_fortify::TwoFactorAuthenticatable for User {}',
    'fortify-passkeys': '#[cfg(feature = "fortify")] impl openrust_fortify::passkeys::PasskeyAuthenticatable for User {}',
}
CLONE = '#[derive(Clone)]'
SENSITIVE = ['two_factor_secret', 'two_factor_recovery_codes']


def remove(sequence, part):
    count = 0
    result = list(sequence)
    while True:
        index = next((i for i in range(len(result) - len(part) + 1)
                      if result[i:i + len(part)] == part), None)
        if index is None:
            return result, count
        result[index:index + len(part)] = []
        count += 1


def assert_model(source, label):
    # 模板先替换成合法哨兵名称；交给真实 Rust 词法器及 syn 解析文件。
    source = source.replace('{{model}}', 'LayeringStub').replace('{{table_reference}}', '"layering_stubs"').replace('{{table}}', 'layering_stubs').replace('{{connection}}', '')
    code = ' '.join(inspect(source)['code_tokens'])
    forbidden = [r'\b(?:Db|DB|DatabaseManager|Transaction|Connection)\b',
                 r'\.\s*(?:table|transaction|lock_for_update|refresh_for_update)\s*\(',
                 r'\b(?:encrypt|decrypt|dispatch)\s*\(', r'\bHash\s*:\s*:\s*make\s*\(']
    for pattern in forbidden:
        assert not re.search(pattern, code), f'{label}：生成模型混入具体存储或业务编排：{pattern}'
    assert 'ModelState' in code and 'impl Model for' in code, f'{label}：缺少模型映射'


def assert_user_delta(before, after, phase):
    """只允许已确认的安装接入；任意旧实现保留，不要求用户代码关键词零命中。"""
    old_source, new_source = inspect(before), inspect(after)
    left, right = old_source['normalized'], new_source['normalized']
    changes = set()
    old_hidden = [old_source['hidden'].count(key) for key in SENSITIVE]
    new_hidden = [new_source['hidden'].count(key) for key in SENSITIVE]
    assert all(new >= old for old, new in zip(old_hidden, new_hidden)), f'{phase}：删除了已有敏感字段隐藏声明'
    if old_hidden != new_hidden:
        changes.add('hidden')
    for name, block in CAPABILITIES.items():
        left, old = remove(left, tokens(block))
        right, new = remove(right, tokens(block))
        assert old <= 1 and new <= 1 and new >= old, f'{phase}：模型能力重复或被删除：{name}'
        if new != old:
            changes.add(name)
    for name, block in [('clone', tokens(CLONE))]:
        left, old = remove(left, block)
        right, new = remove(right, block)
        assert old <= 1 and new <= 1 and new >= old, f'{phase}：模型声明重复或被删除：{name}'
        if new != old:
            changes.add(name)
    allowed = {'sanctum': {'sanctum'}, 'passport': {'passport'},
               'fortify': {'fortify-2fa', 'fortify-passkeys', 'clone', 'hidden'}}[phase]
    assert changes <= allowed, f'{phase}：写入了其他安装器的模型接入：{changes}'
    assert left == right, f'{phase}：安装器改变了约定接入之外的模型代码'


def assert_actions(root):
    directory = root / 'app/actions/fortify'
    assert (directory / 'create_new_user.rs').is_file()
    assert 'User::create' in (directory / 'create_new_user.rs').read_text()
    assert 'save_password' in (directory / 'reset_user_password.rs').read_text()
    assert 'Hash::make' in (directory / 'mod.rs').read_text()
    assert 'save_password' in (directory / 'update_user_password.rs').read_text()
    assert 'send_email_verification_notification' in (directory / 'update_user_profile_information.rs').read_text()


def assert_installed_user(source, phase):
    required = {'sanctum': ['sanctum'], 'passport': ['passport'],
                'fortify': ['fortify-2fa', 'fortify-passkeys']}[phase]
    sequence = tokens(source)
    for name in required:
        _, count = remove(sequence, tokens(CAPABILITIES[name]))
        assert count == 1, f'{phase}：缺少或重复模型能力：{name}'
    if phase == 'fortify':
        _, count = remove(sequence, tokens('#[derive(Clone)] pub(crate) struct User(ModelState);'))
        assert count == 1, 'Fortify：默认 User 缺少 Clone'
        metadata = inspect(source)
        assert metadata['hidden_found'] and all(metadata['hidden'].count(key) == 1 for key in SENSITIVE), 'Fortify：User 实际返回的 ModelMetadata.hidden 缺少双因素隐藏字段'
