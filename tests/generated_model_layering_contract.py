"""生成产物的分层契约；只检查输出，不参与生成或修改应用。"""
import re


def tokens(source):
    # 保留字符串内容，跳过注释；避免仅因格式或注释变化误判实现。
    pattern = re.compile(r'(?P<comment>//[^\n]*|/\*[\s\S]*?\*/)|r(?P<hashes>\#*)"[\s\S]*?"(?P=hashes)|"(?:\\.|[^"\\])*"|::|[A-Za-z_][A-Za-z_0-9]*|[^\s]')
    return [match.group() for match in pattern.finditer(source)
            if match.lastgroup != 'comment']


CAPABILITIES = {
    'sanctum': '#[cfg(feature = "sanctum")] impl openrust::sanctum::HasApiTokens for User {}',
    'passport': '#[cfg(feature = "passport")] impl openrust_passport::HasApiTokens for User { fn get_provider_name(&self) -> &str { "users" } }',
    'fortify-2fa': '#[cfg(feature = "fortify")] impl openrust_fortify::TwoFactorAuthenticatable for User {}',
    'fortify-passkeys': '#[cfg(feature = "fortify")] impl openrust_fortify::passkeys::PasskeyAuthenticatable for User {}',
}
CLONE = tokens('#[derive(Clone)]')
HIDDEN = tokens(', "two_factor_secret", "two_factor_recovery_codes"')


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
    code = ' '.join(token for token in tokens(source)
                    if not (token.startswith(chr(34)) or re.match(r'^r#*"', token)))
    forbidden = [r'\b(?:Db|DB|DatabaseManager|Transaction|Connection)\b',
                 r'\.\s*(?:table|transaction|lock_for_update|refresh_for_update)\s*\(',
                 r'\b(?:encrypt|decrypt|dispatch)\s*\(', r'\bHash\s*::\s*make\s*\(']
    for pattern in forbidden:
        assert not re.search(pattern, code), f'{label}：生成模型混入具体存储或业务编排：{pattern}'
    assert 'ModelState' in code and 'impl Model for' in code, f'{label}：缺少模型映射'


def assert_user_delta(before, after, phase):
    """只允许已确认的安装接入；任意旧实现保留，不要求用户代码关键词零命中。"""
    left, right = tokens(before), tokens(after)
    changes = set()
    for name, block in CAPABILITIES.items():
        left, old = remove(left, tokens(block))
        right, new = remove(right, tokens(block))
        assert old <= 1 and new <= 1 and new >= old, f'{phase}：模型能力重复或被删除：{name}'
        if new != old:
            changes.add(name)
    for name, block in [('clone', CLONE), ('hidden', HIDDEN)]:
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
        _, count = remove(sequence, HIDDEN)
        assert count == 1, 'Fortify：缺少双因素隐藏字段'
