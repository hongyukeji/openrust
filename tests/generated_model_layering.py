"""在项目内隔离快照运行；用生成应用的真实 HTTP／Artisan 验证模型分层与定制保留。"""
import base64
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import time
import urllib.error
import urllib.request

from generated_model_layering_contract import assert_actions, assert_model, assert_user_delta, assert_installed_user

FEATURES = 'sanctum,passport,fortify'
CUSTOM_USER = '''
impl User {
    pub(crate) fn layering_marker() -> &'static str { "retained" }
}
'''
SERVICE = '''use crate::app::models::{User, layering_probe::LayeringProbe, field_probe::FieldProbe, customized_probe::CustomizedProbe, misplaced_hidden_probe::MisplacedHiddenProbe};
use openrust::{contracts::auth::MustVerifyEmail, database::eloquent::Model, hashing::{Hash, HashOptions}, http::HttpError};
fn hides_sensitive<M: Model>(model: &M) -> Result<bool, HttpError> {
    let values = model.to_array()?;
    let serialized = serde_json::to_string(&values).unwrap();
    Ok(["two_factor_secret", "two_factor_recovery_codes"].iter().all(|key| model.get_hidden().contains(key) && !values.contains_key(*key))
        && !serialized.contains("layering-secret-probe") && !serialized.contains("layering-recovery-probe"))
}
fn capabilities<T: openrust::sanctum::HasApiTokens + openrust_passport::HasApiTokens + openrust_fortify::TwoFactorAuthenticatable + openrust_fortify::passkeys::PasskeyAuthenticatable>() {}
pub(crate) fn run() -> openrust::http::HttpFuture<'static, Result<String, HttpError>> {
    Box::pin(async {
    capabilities::<User>();
    let password = Hash::make("layering-test-password", &HashOptions::new()).await?;
    let mut user = User::create([("name".into(), "Layering".into()), ("email".into(), "layering@example.test".into()), ("password".into(), password.into())].into()).await?;
    assert!(!user.has_verified_email());
    assert!(user.mark_email_as_verified().await.map_err(|e| HttpError::Internal { source: Box::new(e) })?);
    let mut loaded = User::find(user.get_key().unwrap().clone()).await?.unwrap();
    assert!(loaded.has_verified_email());
    let sensitive = || [("two_factor_secret".into(), "layering-secret-probe".into()), ("two_factor_recovery_codes".into(), "layering-recovery-probe".into())].into();
    loaded.force_fill(sensitive())?;
    assert!(hides_sensitive(&loaded)?);
    let mut exposed = MisplacedHiddenProbe::from_state(Default::default());
    exposed.force_fill(sensitive())?;
    assert!(!hides_sensitive(&exposed)?);
    let leaked = exposed.to_array()?;
    assert_eq!(leaked["two_factor_secret"], "layering-secret-probe");
    assert_eq!(leaked["two_factor_recovery_codes"], "layering-recovery-probe");
    user.delete().await?;
    Ok(serde_json::json!({"table": LayeringProbe::from_state(Default::default()).get_table(), "custom": CustomizedProbe::stub_marker(), "field": FieldProbe::EMAIL, "user": User::layering_marker(), "sensitive_hidden": true, "misplaced_hidden_rejected": true, "verified": loaded.has_verified_email()}).to_string())
    })
}
'''
COMMAND = '''use openrust::{console::{Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output}, foundation::Application};
struct Probe(CommandDefinition);
impl Command for Probe {
    fn definition(&self) -> &CommandDefinition { &self.0 }
    fn handle<'a>(&'a self, _: &'a Application, _: &'a Input, output: &'a mut Output) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let value = crate::app::services::layering_probe::run().await.map_err(|e| ConsoleError::new(e.to_string()))?;
            output.write_raw(&value).map_err(|e| ConsoleError::new(e.to_string()))?;
            Ok(0)
        })
    }
}
pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(clap::Command::new("layering:probe")))))
}
'''
EXPECTED = {'table': 'layering_probes', 'custom': 'custom-stub', 'field': 'email', 'user': 'retained', 'verified': True, 'sensitive_hidden': True, 'misplaced_hidden_rejected': True}


def run():
    source = Path(__file__).resolve().parents[1]
    base = source.parent
    assert base.parent.name == 'tmp' and base.parent.parent.name == 'target', '必须在项目 target/tmp/ 隔离源码中运行'
    target = Path(os.environ['CARGO_TARGET_DIR']).resolve()
    assert target.is_relative_to(base), '必须使用本工作流独占 Cargo 缓存'
    assert os.environ.get('OPENRUST_REQUIRE_MYSQL') == '1' and os.environ.get('OPENRUST_ACCEPTANCE_MYSQL_CONTAINER'), '必须提供独占 MySQL 验收入口'
    generated = base / 'generated'
    assert not generated.exists(), '已有生成应用须先核对归属，不能覆盖'
    def ignore(directory, names):
        if Path(directory) == source:
            return set(names) & {'.git', '.agents', 'docs', 'target', 'tmp', 'storage', 'node_modules', '__pycache__'}
        return set()
    shutil.copytree(source, generated, ignore=ignore)
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    origin = f'http://127.0.0.1:{port}'
    env = dict(os.environ, RUSTUP_TOOLCHAIN='1.94.0', APP_BASE_PATH=str(generated), APP_ENV='testing', APP_DEBUG='false', APP_LOCALE='en',
               APP_KEY='base64:' + base64.b64encode(b'a' * 32).decode(), APP_URL=origin, APP_HOST='127.0.0.1', APP_PORT=str(port), SERVER_PORT=str(port),
               DB_CONNECTION='mysql', DB_URL=os.environ['OPENRUST_TEST_MYSQL_URL'], DB_PREFIX='ml_',
               CACHE_STORE='array', SESSION_DRIVER='array', QUEUE_CONNECTION='sync', MAIL_MAILER='array', BCRYPT_ROUNDS='4',
               APP_DISABLED_RUNNERS='queue,schedule,reverb,grpc', NO_COLOR='1', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
    for key in ['APP_CONFIG_CACHE', 'APP_PACKAGES_CACHE', 'APP_ROUTES_CACHE', 'APP_EVENTS_CACHE', 'APP_VIEW_CACHE', 'APP_STORAGE_PATH']:
        env.pop(key, None)
    binary = target / 'debug/main'
    count = 0
    def command(label, args, timeout=900, success=True):
        nonlocal count
        count += 1
        log = base / f'generated-layering-{count:02}-{label}.log'
        with log.open('w') as output:
            result = subprocess.run(args, cwd=generated, env=env, stdout=output, stderr=subprocess.STDOUT, timeout=timeout)
        assert (result.returncode == 0) == success, f'{label} 退出码 {result.returncode}，见 {log.name}'
        return log.read_text()
    def build(features=True):
        args = ['cargo', 'build', '--locked', '--bin', 'main']
        if features:
            args += ['--features', FEATURES]
        command('build', args)
    def cli(*args, success=True):
        return command(args[0].replace(':', '-'), [str(binary), 'artisan', *args], success=success)
    def http(label, installed=False):
        with (base / f'generated-layering-{label}-http.log').open('w') as output:
            server = subprocess.Popen([str(binary)], cwd=generated, env=env, stdout=output, stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic() + 30
                while True:
                    try:
                        urllib.request.urlopen(origin + '/up', timeout=1).close()
                        break
                    except urllib.error.URLError:
                        assert server.poll() is None and time.monotonic() < deadline, f'{label} HTTP 未就绪'
                        time.sleep(.1)
                assert urllib.request.urlopen(origin + '/', timeout=5).status == 200
                if installed:
                    try:
                        urllib.request.urlopen(origin + '/api/user', timeout=5).close()
                        raise AssertionError('Sanctum 保护路由未拒绝匿名请求')
                    except urllib.error.HTTPError as error:
                        assert error.code == 401
                    with urllib.request.urlopen(origin + '/layering-probe', timeout=10) as response:
                        assert json.load(response) == EXPECTED
            finally:
                if server.poll() is None:
                    server.send_signal(signal.SIGINT)
                assert server.wait(timeout=15) == 0
        print(label, '真实 HTTP PASS', flush=True)

    user = generated / 'app/models/user.rs'
    assert_model(user.read_text(), '默认 User')
    build(features=False)
    http('default')
    cli('migrate', '--force')
    user.write_text(user.read_text() + CUSTOM_USER)
    cli('make:model', 'LayeringProbe')
    cli('make:model', 'FieldProbe', '--fields', '--table=users', '--database=mysql')
    for name in ['layering_probe', 'field_probe']:
        assert_model((generated / f'app/models/{name}.rs').read_text(), name)
    stub = generated / 'stubs/model.rs.stub'
    stub.parent.mkdir(exist_ok=True)
    stub_source = (generated / 'vendor/openrust/framework/src/foundation/console/stubs/model.rs.stub').read_text()
    stub_source += '\nimpl {{model}} { pub fn stub_marker() -> &\'static str { "custom-stub" } }\n'
    stub.write_text(stub_source)
    cli('make:model', 'CustomizedProbe')
    customized = generated / 'app/models/customized_probe.rs'
    assert_model(customized.read_text(), '自定义 stub 模型')
    customized.write_text(customized.read_text() + '\n// 保留已有模型定制\n')
    retained_custom_model = customized.read_bytes()
    cli('make:model', 'CustomizedProbe', success=False)
    assert customized.read_bytes() == retained_custom_model
    generated_models = {p.name: p.read_bytes() for p in user.parent.glob('*.rs') if p != user}
    for phase, args in [
        ('sanctum', ['install:api', '--without-migration-prompt']),
        ('passport', ['install:api', '--passport', '--without-migration-prompt', '--no-interaction']),
        ('fortify', ['fortify:install']),
    ]:
        if phase == 'fortify':
            build()  # Passport 自带重建按默认 feature 执行；恢复 Fortify 命令的编译入口。
        before = user.read_text()
        cli(*args)
        after = user.read_text()
        assert_user_delta(before, after, phase)
        assert_installed_user(after, phase)
        assert_model(after, f'{phase} 安装后的 User')
        cli(*args)
        assert user.read_text() == after, f'{phase} 重复安装改动模型'
        assert_user_delta(after, user.read_text(), phase)
        assert stub.read_text() == stub_source
        assert {p.name: p.read_bytes() for p in user.parent.glob('*.rs') if p != user} == generated_models
        assert CUSTOM_USER in user.read_text()
        print(phase, '安装前后白名单、重复安装与定制保留 PASS', flush=True)
    assert_actions(generated)
    # 沿用既有 Fortify 生成应用夹具的必需配置：默认 User 需要邮箱验证路由。
    config = generated / 'config/fortify.rs'
    text = config.read_text().replace('"views":true', '"views":false').replace('"two-factor-authentication","passkeys"]', '"two-factor-authentication","email-verification"]')
    assert text != config.read_text()
    config.write_text(text)
    action = generated / 'app/actions/fortify/create_new_user.rs'
    action.write_text(action.read_text() + '\n// 保留已有 Action 定制\n')
    action_source = action.read_bytes()
    cli('fortify:install')
    assert action.read_bytes() == action_source
    # 故意错放字段的模型只用于序列化反例；没有业务流程或数据库访问。
    misplaced = (generated / 'vendor/openrust/framework/src/foundation/console/stubs/model.rs.stub').read_text()
    misplaced = misplaced.replace('{{model}}', 'MisplacedHiddenProbe').replace('{{table_reference}}', '"users"').replace('{{connection}}', '')
    misplaced = misplaced.replace('ModelMetadata::new("users")', 'ModelMetadata::new("users").fillable(&["two_factor_secret", "two_factor_recovery_codes"]).hidden(&["password", "remember_token"])')
    (generated / 'app/models/misplaced_hidden_probe.rs').write_text(misplaced)
    modules = generated / 'app/models/mod.rs'
    modules.write_text(modules.read_text() + '\npub(crate) mod layering_probe;\npub(crate) mod field_probe;\npub(crate) mod customized_probe;\npub(crate) mod misplaced_hidden_probe;\n')
    services = generated / 'app/services'
    assert not services.exists()
    services.mkdir()
    (services / 'mod.rs').write_text('pub(crate) mod layering_probe;\n')
    (services / 'layering_probe.rs').write_text(SERVICE)
    app = generated / 'app/mod.rs'
    app.write_text(app.read_text() + '\npub(crate) mod services;\n')
    (generated / 'app/console/commands/mod.rs').write_text(COMMAND)
    routes = generated / 'routes/web.rs'
    routes.write_text(routes.read_text().replace('    Route::get("/",', '    Route::get("/layering-probe", layering_probe)?;\n    Route::get("/",', 1) + '''
fn layering_probe(_: &mut openrust::http::Request) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async { Ok(openrust::http::Response::from(crate::app::services::layering_probe::run().await?)) })
}
''')
    build()
    cli('migrate', '--force')
    assert 'Ran' in cli('migrate:status')
    assert 'layering-probe' in cli('route:list')
    assert json.loads(cli('layering:probe').strip()) == EXPECTED
    http('installed', installed=True)
    command('fmt', ['cargo', 'fmt', '--all'])
    command('clippy', ['cargo', 'clippy', '--locked', '--features', FEATURES, '--bin', 'main', '--', '-D', 'warnings'])
    cli('migrate:rollback', '--force')
    print('生成应用：模型编译、真实 HTTP／Artisan、数据库写入／验证／删除、严格 Clippy PASS', flush=True)


if __name__ == '__main__':
    run()
