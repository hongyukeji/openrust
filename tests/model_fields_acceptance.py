"""真实 Artisan 自动生成、结构刷新及编译正反例，数据库仅使用隔离夹具。"""

import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    args = parser.parse_args()
    project = Path(__file__).resolve().parents[1]
    source, work = args.source.resolve(), args.work.resolve()
    target = Path(os.environ['CARGO_TARGET_DIR']).resolve()
    for path in (source, work, target):
        if not path.is_relative_to(project / 'target' / 'tmp'):
            raise ValueError('验收源码、临时文件和构建目录必须位于项目 target/tmp/')
    work.mkdir(parents=True, exist_ok=True)
    driver = os.environ.get('OPENRUST_FIELD_DATABASE', 'sqlite')
    if driver == 'sqlite':
        (work / 'fields.sqlite').touch()
    url = {'postgres': 'OPENRUST_TEST_POSTGRES_URL', 'mysql': 'OPENRUST_TEST_MYSQL_URL'}
    environment = dict(os.environ, APP_BASE_PATH=str(source), APP_ENV='testing',
                       DB_CONNECTION={'postgres': 'pgsql'}.get(driver, driver),
                       DB_URL=os.environ[url[driver]] if driver in url else '',
                       DB_DATABASE=str(work / 'fields.sqlite') if driver == 'sqlite' else 'openrust_acceptance',
                       FIELD_DRIVER=driver, FIELD_SQLITE=str(work / 'fields.sqlite'))
    binary = target / 'debug' / ('main.exe' if os.name == 'nt' else 'main')
    quote = '`' if driver == 'mysql' else '"'
    deps = target / 'debug/deps'
    libraries = {}
    for crate in ('openrust', 'tokio'):
        library = max(deps.glob(f'lib{crate}-*.rlib'), key=lambda path: path.stat().st_mtime)
        libraries[crate] = library
    compiler = ['rustup', 'run', '1.94.0', 'rustc', '--edition=2024', '-L', f'dependency={deps}']
    for crate, library in libraries.items():
        compiler.extend(['--extern', f'{crate}={library}'])

    def run(name, command, success=True):
        result = subprocess.run(command, env=environment, capture_output=True, text=True)
        (work / f'{name}.log').write_text(result.stdout + result.stderr)
        assert (result.returncode == 0) == success, f'{name}：退出码不符，见对应日志'
        return result

    def compile_code(name, content, success=True, executable=False, deny_warnings=False):
        path = work / f'{name}.rs'
        path.write_text(content)
        flags = [] if executable else ['--crate-type=lib', '--emit=metadata']
        if deny_warnings:
            flags.extend(['-D', 'warnings'])
        result = run(name, [*compiler, *flags, str(path), '--out-dir', str(work)], success)
        if not success:
            assert 'E0599' in result.stderr, f'{name}：必须因关联常量不存在而失败'
        return work / name

    db_setup = '''
use std::{collections::BTreeMap, sync::Arc};
use openrust::{config::{Repository, Value}, database::{Connection, DatabaseManager}};
fn object(entries: &[(&str, Value)]) -> Value {
    Value::Object(entries.iter().map(|(key,value)| ((*key).into(),value.clone())).collect())
}
fn database() -> Connection {
    let configuration = match std::env::var("FIELD_DRIVER").unwrap().as_str() {
        "sqlite" => object(&[("driver", "sqlite".into()), ("database", std::env::var("FIELD_SQLITE").unwrap().into())]),
        _ => object(&[("url", std::env::var("DB_URL").unwrap().into())]),
    };
    DatabaseManager::new(Arc::new(Repository::new(BTreeMap::from([(
        "database".into(), object(&[("default", "test".into()), ("connections", object(&[("test",configuration)]))])
    )]))), ".").connection(None).unwrap()
}
'''
    helper = compile_code('schema', db_setup + '''
#[tokio::main]
async fn main() {
    database().statement(&std::env::args().nth(1).unwrap(), &[]).await.unwrap();
}
''', executable=True)

    def sql(name, statement):
        run(name, [str(helper), statement])

    def artisan(name, *arguments, success=True):
        return run(name, [str(binary), 'artisan', *arguments], success)

    table = 'field_reference_probes'
    generated = source / 'app/models/field_reference_probe.rs'
    sql('create', f'create table {table} (id integer primary key, name varchar(80), {quote}order{quote} integer, parent_id integer, {quote}display-name{quote} varchar(80), {quote}1st{quote} integer)')

    def comment(name, text):
        if driver == 'sqlite':
            return
        escaped = text.replace('\\', '\\\\') if driver == 'mysql' else text
        literal = "'" + escaped.replace("'", "''") + "'"
        statement = (f'alter table {table} modify name varchar(80) comment {literal}'
                     if driver == 'mysql' else f'comment on column {table}.name is {literal}')
        sql(name, statement)

    try:
        comment('initial-comment', '姓名')
        artisan('generator', 'make:model', 'FieldReferenceProbe', '--force', '--fields')
        original = generated.read_text()
        assert 'const NAME = "name"' in original and 'const DISPLAY_NAME = "display-name"' in original
        assert 'const COLUMN_1ST = "1st"' in original and 'const ORDER = "order"' in original
        assert original.count('#[allow(dead_code') == 1, '未使用字段警告只在生成 impl 上设置一次'
        assert ('#[doc = "姓名"]' in original) == (driver != 'sqlite')
        if driver == 'sqlite':
            assert '#[doc =' not in original, '没有列注释时不得猜测字段说明'
        artisan('refresh', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == original, '重复刷新应保持相同内容'
        generated.write_text(generated.read_text() + '\n// 保留业务代码\n')
        artisan('refresh-custom', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text().endswith('// 保留业务代码\n')
        model = generated.read_text()
        reference = '\npub fn reference() -> &\'static str { FieldReferenceProbe::NAME }\n'
        compile_code('generated', model + reference, deny_warnings=True)
        compile_code('missing', model + reference.replace('::NAME', '::NAEM'), False)
        if driver != 'sqlite':
            # 文档必须真正进入 rustdoc，而不只是生成源码中的字符串。
            documentation = work / 'documentation'
            run('rustdoc', [*compiler[:3], 'rustdoc', *compiler[4:],
                           '--crate-name', 'generated', '--document-private-items',
                           str(work / 'generated.rs'), '-o', str(documentation)])
            page = (documentation / 'generated/struct.FieldReferenceProbe.html').read_text()
            assert '姓名' in page and 'associatedconstant.NAME' in page
            # 仅修改注释也须令 --check 过期；检查不得修改文件或时间戳。
            description = '姓名 "别名" \'引号\'\\路径\n第二行 */\r#[allow(dead_code)]\n"; compile_error!("注入"); //'
            comment('changed-comment', description)
            snapshot = (generated.read_bytes(), generated.stat().st_mtime_ns)
            result = artisan('comment-stale', 'model:fields', 'FieldReferenceProbe', '--check', success=False)
            assert result.returncode == 1
            assert snapshot == (generated.read_bytes(), generated.stat().st_mtime_ns)
            artisan('comment-refresh', 'model:fields', 'FieldReferenceProbe')
            compile_code('escaped-comment', generated.read_text() + reference, deny_warnings=True)
            assert generated.read_text() != model
            artisan('comment-fresh', 'model:fields', 'FieldReferenceProbe', '--check')
            snapshot = (generated.read_bytes(), generated.stat().st_mtime_ns)
            artisan('comment-idempotent', 'model:fields', 'FieldReferenceProbe')
            assert snapshot == (generated.read_bytes(), generated.stat().st_mtime_ns)
            for label, empty in [('blank-comment', ' \t\n'), ('deleted-comment', '')]:
                comment(label, empty)
                artisan(label + '-refresh', 'model:fields', 'FieldReferenceProbe')
                assert '#[doc =' not in generated.read_text(), '空白或删除的注释不生成文档'
            model = generated.read_text()
        # 真正执行生成模型的查询，包含保留字、NULL 和参数绑定。
        sql('insert', f"insert into {table} (id, name, {quote}order{quote}) values (1, '张三', 2)")
        consumer = compile_code('consumer', model + db_setup + '''
#[tokio::main]
async fn main() {
    let db = database();
    let mut query = FieldReferenceProbe::queryOn(&db);
    query.r#where(FieldReferenceProbe::NAME, "=", "张三");
    query.whereNull(FieldReferenceProbe::PARENT_ID);
    let users = query.orderBy(FieldReferenceProbe::ORDER, "asc").get().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].getAttribute(FieldReferenceProbe::ID), Some(&1.into()));
}
''', executable=True)
        run('consumer-run', [str(consumer)])
        # 同一模型无需重新编译 Artisan，即可刷新增删及重命名后的字段。
        sql('rename', f'alter table {table} rename column name to full_name')
        sql('add', f'alter table {table} add extra integer')
        sql('drop', f'alter table {table} drop column parent_id')
        artisan('regenerate', 'model:fields', 'FieldReferenceProbe')
        refreshed = generated.read_text()
        assert 'const EXTRA' in refreshed and 'const PARENT_ID' not in refreshed
        compile_code('renamed', refreshed + reference, False)
        compile_code('new-name', refreshed + reference.replace('::NAME', '::FULL_NAME'))
        before = generated.read_bytes()
        artisan('missing-table', 'model:fields', 'FieldReferenceProbe', '--table=not_a_table', success=False)
        assert generated.read_bytes() == before
        artisan('bad-connection', 'model:fields', 'FieldReferenceProbe', '--database=not_a_connection', success=False)
        assert generated.read_bytes() == before
        # 自定义表和命名空间；读取连接由 --database 显式选择。
        artisan('nested', 'make:model', 'Admin/FieldReferenceProbe', '--force', '--fields',
                f'--table={table}', f'--database={environment["DB_CONNECTION"]}')
        nested = source / 'app/models/admin/field_reference_probe.rs'
        compile_code('nested-model', nested.read_text() + reference.replace('::NAME', '::FULL_NAME'))
        assert f'ModelMetadata::new("{table}")' in nested.read_text()
        assert f'.connection("{environment["DB_CONNECTION"]}")' in nested.read_text()
        # 编译真实模型消费者，再经正式 Application::handleCommand 调用 Artisan。
        # 默认连接故意无效，确保同步实际使用模型保存的连接而不是碰巧命中默认库。
        artisan('linked', 'make:model', 'LinkedProbe', '--force', '--fields',
                f'--table={table}', f'--database={environment["DB_CONNECTION"]}')
        linked = source / 'app/models/linked_probe.rs'
        runner_source = (
            'mod app { pub mod models { pub mod linked_probe { include!(' + json.dumps(str(linked)) + '); } '
            'pub mod admin { pub mod field_reference_probe { include!(' + json.dumps(str(nested)) + '); } } } }\n'
            + db_setup + r'''
fn load(_: &std::path::Path, _: &openrust::config::Environment)
    -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    let configuration = match std::env::var("FIELD_DRIVER").unwrap().as_str() {
        "sqlite" => object(&[("driver", "sqlite".into()), ("database", std::env::var("FIELD_SQLITE").unwrap().into())]),
        _ => object(&[("url", std::env::var("DB_URL").unwrap().into())]),
    };
    Ok(BTreeMap::from([("database".into(), object(&[
        ("default", "not_configured".into()),
        ("connections", object(&[(&std::env::var("DB_CONNECTION").unwrap(), configuration)])),
    ]))]))
}
#[tokio::main]
async fn main() {
    let app = openrust::foundation::Application::configure(std::env::var("APP_BASE_PATH").unwrap().into())
        .withConfig(load).create().unwrap();
    let status = app.handleCommand(&std::env::args_os().skip(1).collect::<Vec<_>>()).await.unwrap();
    std::process::exit(status);
}
''')
        runner = compile_code('registered', runner_source, executable=True)

        def registered(name, *arguments, success=True):
            return run(name, [str(runner), 'model:fields', *arguments], success)

        def source_snapshot():
            return {path: (path.read_bytes(), path.stat().st_mtime_ns)
                    for path in (linked, nested)}

        registered('compiled-check', '--all', '--check')
        registered('multiple-check', 'LinkedProbe', 'Admin/FieldReferenceProbe', '--check')
        registered('conflicting-selection', 'LinkedProbe', '--all', success=False)
        empty = compile_code('unregistered', runner_source.split('\n', 1)[1], executable=True)
        result = run('no-registered-models', [str(empty), 'model:fields', '--all', '--check'], False)
        assert '没有已编译' in result.stdout + result.stderr, '空集合不能静默通过 CI 检查'
        # 在没有源码锁的独立应用根执行检查，验证不是仅保留已有锁文件。
        check_root = work / 'read-only-source'
        for path in (linked, nested):
            copy = check_root / path.relative_to(source)
            copy.parent.mkdir(parents=True, exist_ok=True)
            copy.write_bytes(path.read_bytes())
        environment['APP_BASE_PATH'] = str(check_root)
        try:
            assert not (check_root / 'tmp').exists()
            registered('without-source-lock', '--all', '--check')
            assert not (check_root / 'tmp').exists(), '--check 不应创建源码锁目录'
        finally:
            environment['APP_BASE_PATH'] = str(source)
        sql('registered-add', f'alter table {table} add registered_extra integer')
        unchanged = source_snapshot()
        locks = set((source / 'tmp').glob('*'))
        result = registered('compiled-stale', '--all', '--check', success=False)
        assert result.returncode == 1
        assert str(linked) in result.stdout and str(nested) in result.stdout
        assert source_snapshot() == unchanged, '--check 不得修改内容或时间戳'
        assert set((source / 'tmp').glob('*')) == locks, '--check 不创建源码锁或临时文件'
        registered('batch-preflight', 'LinkedProbe', 'UnknownProbe', success=False)
        assert source_snapshot() == unchanged, '批量预检失败不得先写入有效模型'
        registered('batch-override', 'LinkedProbe', 'Admin/FieldReferenceProbe', '--table=unused', success=False)
        assert source_snapshot() == unchanged
        registered('compiled-refresh', '--all')
        assert all('const REGISTERED_EXTRA' in path.read_text() for path in (linked, nested))
        unchanged = source_snapshot()
        registered('compiled-idempotent', '--all')
        registered('compiled-fresh', '--all', '--check')
        assert source_snapshot() == unchanged, '相同结果不应重写文件'
        registered('connection-override', 'LinkedProbe', '--database=not_configured', success=False)
        assert source_snapshot() == unchanged
        # 配置变化重新编译后使用新表，不能要求每次重新输入 --table。
        sql('other-create', 'create table field_reference_other (id integer, payload integer)')
        try:
            nested.write_text(nested.read_text().replace(f'ModelMetadata::new("{table}")',
                                                         'ModelMetadata::new("field_reference_other")'))
            runner = compile_code('registered', runner_source, executable=True)
            registered('metadata-changed-check', 'Admin/FieldReferenceProbe', '--check', success=False)
            registered('metadata-changed-refresh', 'Admin/FieldReferenceProbe')
            assert 'const PAYLOAD' in nested.read_text() and 'const REGISTERED_EXTRA' not in nested.read_text()
            registered('metadata-changed-fresh', '--all', '--check')
        finally:
            sql('other-drop', 'drop table field_reference_other')
        sql('collision-create', f'create table field_collision ({quote}a-b{quote} integer, a_b integer)')
        try:
            artisan('collision', 'model:fields', 'FieldReferenceProbe', '--table=field_collision', success=False)
            assert generated.read_bytes() == before
            absent = source / 'app/models/collision_probe.rs'
            artisan('new-collision', 'make:model', 'CollisionProbe', '--fields', '--table=field_collision', success=False)
            assert not absent.exists(), '生成失败不得留下半成品'
        finally:
            sql('collision-drop', 'drop table field_collision')
        sql('unicode-create', f'create table field_unicode ({quote}姓名{quote} integer)')
        try:
            artisan('unicode', 'model:fields', 'FieldReferenceProbe', '--table=field_unicode', success=False)
            assert generated.read_bytes() == before
        finally:
            sql('unicode-drop', 'drop table field_unicode')
        # 表名中的引号既不能注入 Rust 源码，也不能改变数据库元数据。
        unusual_table = 'field"custom'
        identifier = quote + unusual_table.replace(quote, quote + quote) + quote
        sql('quoted-create', f'create table {identifier} (id integer)')
        try:
            artisan('quoted', 'make:model', 'QuotedProbe', '--force', '--fields', f'--table={unusual_table}')
            quoted_model = source / 'app/models/quoted_probe.rs'
            compile_code('quoted-model', quoted_model.read_text() + "\npub fn reference() -> &'static str { QuotedProbe::ID }\n")
        finally:
            sql('quoted-drop', f'drop table {identifier}')
        generated.write_text(refreshed.replace('// openrust:model-fields end', '// removed end'))
        damaged = generated.read_bytes()
        artisan('damaged', 'model:fields', 'FieldReferenceProbe', success=False)
        assert generated.read_bytes() == damaged
        for label, broken in [
            ('duplicate-marker', refreshed + '// openrust:model-fields begin\n'),
            ('reversed-marker', refreshed.replace('// openrust:model-fields begin', '// openrust:model-fields TEMP').replace('// openrust:model-fields end', '// openrust:model-fields begin').replace('// openrust:model-fields TEMP', '// openrust:model-fields end')),
        ]:
            generated.write_text(broken)
            artisan(label, 'model:fields', 'FieldReferenceProbe', success=False)
            assert generated.read_text() == broken
        generated.write_text(refreshed)
        artisan('exists', 'make:model', 'FieldReferenceProbe', '--fields', success=False)
        assert generated.read_text() == refreshed
        artisan('plain', 'make:model', 'PlainProbe', '--force')
        assert 'model_fields!' not in (source / 'app/models/plain_probe.rs').read_text()
        artisan('database-requires-fields', 'make:model', 'NoFields', '--database=sqlite', success=False)
        assert not (source / 'app/models/no_fields.rs').exists()
        artisan('traversal', 'model:fields', '../User', success=False)
        # 已发布自定义模板必须保存连接；缺少占位符时失败且不留下新模型。
        stub_directory = source / 'stubs'
        custom_stub = stub_directory / 'model.rs.stub'
        stub_directory.mkdir(exist_ok=True)
        old_stub = custom_stub.read_bytes() if custom_stub.exists() else None
        stub_model = source / 'app/models/stub_probe.rs'
        old_model = stub_model.read_bytes() if stub_model.exists() else None
        stub_model.unlink(missing_ok=True)
        try:
            template = (source / 'vendor/openrust/framework/src/foundation/console/stubs/model.rs.stub').read_text()
            custom_stub.write_text(template.replace('{{connection}}', ''))
            artisan('old-custom-stub', 'make:model', 'StubProbe', '--fields',
                    f'--table={table}', f'--database={environment["DB_CONNECTION"]}', success=False)
            assert not (source / 'app/models/stub_probe.rs').exists()
            custom_stub.write_text(template)
            artisan('custom-stub', 'make:model', 'StubProbe', '--fields',
                    f'--table={table}', f'--database={environment["DB_CONNECTION"]}')
            assert f'.connection("{environment["DB_CONNECTION"]}")' in (source / 'app/models/stub_probe.rs').read_text()
        finally:
            if old_model is None:
                stub_model.unlink(missing_ok=True)
            else:
                stub_model.write_bytes(old_model)
            if old_stub is None:
                custom_stub.unlink()
            else:
                custom_stub.write_bytes(old_stub)
        print(f'{driver}: 自动生成、真实查询、刷新、编译正反例及失败保护 PASS', flush=True)
    finally:
        sql('cleanup', f'drop table {table}')


if __name__ == '__main__':
    main()
