"""真实 Artisan 自动生成、结构刷新及编译正反例，数据库仅使用隔离夹具。"""

import argparse
import json
import os
import re
import shutil
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
    environment = dict(os.environ, APP_BASE_PATH=str(source), APP_ENV='testing', APP_LOCALE='zh-Hans', APP_FALLBACK_LOCALE='en',
                       DB_CONNECTION={'postgres': 'pgsql'}.get(driver, driver),
                       DB_URL=os.environ[url[driver]] if driver in url else '',
                       DB_PREFIX='',
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

    def run(name, command, success=True, cwd=None):
        result = subprocess.run(command, env=environment, capture_output=True, text=True, cwd=cwd)
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

    # where是Rust关键字；普通方法调用在解析阶段失败，r#where沿用现有公开名称。
    keyword = work / 'where_keyword.rs'
    keyword.write_text('struct Query; impl Query { fn r#where(&self, _: &str) {} } fn main() { Query.where("name"); }')
    result = run('where-keyword', [*compiler, str(keyword), '--out-dir', str(work)], False)
    assert 'keyword `where`' in result.stderr and 'r#where' in result.stderr

    db_setup = '''
use std::{collections::BTreeMap, sync::Arc};
use openrust::{config::{Repository, Value}, database::{Connection, DatabaseManager}};
fn object(entries: &[(&str, Value)]) -> Value {
    Value::Object(entries.iter().map(|(key,value)| ((*key).into(),value.clone())).collect())
}
fn database() -> Connection {
    let configuration = match std::env::var("FIELD_DRIVER").unwrap().as_str() {
        "sqlite" => object(&[("driver", "sqlite".into()), ("database", std::env::var("FIELD_SQLITE").unwrap().into()), ("prefix", std::env::var("DB_PREFIX").unwrap_or_default().into())]),
        _ => object(&[("url", std::env::var("DB_URL").unwrap().into()), ("prefix", std::env::var("DB_PREFIX").unwrap_or_default().into())]),
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
    sql('create', f'create table {table} (name varchar(80), id integer primary key, {quote}order{quote} integer, parent_id integer, {quote}display-name{quote} varchar(80), {quote}1st{quote} integer)')

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
        # 首次同步：普通模型没有字段宏、生成元信息或边界标记。
        artisan('plain-create', 'make:model', 'FieldReferenceProbe', '--force')
        plain = generated.read_text()
        assert 'model_fields!' not in plain and '#[generated' not in plain
        assert '@auto-generated' not in plain and 'openrust:model-fields' not in plain
        business_method = '\nimpl FieldReferenceProbe { pub fn business_label(&self) -> &str { "保留业务方法" } }\n// 保留首次生成前的业务注释\n'
        plain += business_method
        generated.write_text(plain)
        compile_code('plain-model-before-fields', plain, deny_warnings=True)
        before = (generated.read_bytes(), generated.stat().st_mtime_ns)
        artisan('plain-check-without-markers', 'model:fields', 'FieldReferenceProbe', '--check', success=False)
        assert before == (generated.read_bytes(), generated.stat().st_mtime_ns)
        entry = ['cmd', '/d', '/c', str(source / 'artisan.bat')] if os.name == 'nt' else [str(source / 'artisan')]
        run('plain-first-fields', [*entry, 'model:fields', 'FieldReferenceProbe'], cwd=source)
        first_sync = generated.read_text()
        assert first_sync.count('openrust::model_fields!') == 1
        assert first_sync.count('#[generated(command = "artisan model:fields FieldReferenceProbe")]') == 1
        assert first_sync.index('use openrust::') < first_sync.index('openrust::model_fields!') < first_sync.index('pub struct FieldReferenceProbe')
        assert first_sync.endswith(business_method)
        compile_code('plain-model-after-fields', first_sync, deny_warnings=True)
        first_state = (generated.read_bytes(), generated.stat().st_mtime_ns)
        run('plain-repeat-fields', [*entry, 'model:fields', 'FieldReferenceProbe'], cwd=source)
        run('plain-repeat-check', [*entry, 'model:fields', 'FieldReferenceProbe', '--check'], cwd=source)
        assert first_state == (generated.read_bytes(), generated.stat().st_mtime_ns)
        artisan('generator', 'make:model', 'FieldReferenceProbe', '--force', '--fields')
        original = generated.read_text()
        declarations = re.findall(r'^        ([A-Z_0-9]+) = (".*");$', original, re.MULTILINE)
        assert [(field, json.loads(value)) for field, value in declarations] == [
            ('TABLE', table), ('NAME', 'name'), ('ID', 'id'), ('ORDER', 'order'),
            ('PARENT_ID', 'parent_id'), ('DISPLAY_NAME', 'display-name'), ('COLUMN_1ST', '1st')]
        assert '/// 表名\n        TABLE' in original
        assert 'ModelMetadata::new(FieldReferenceProbe::TABLE)' in original
        assert original.count('"' + table + '"') == 1, '表名只在 TABLE 中维护一处'
        assert 'NAME = "name"' in original and 'DISPLAY_NAME = "display-name"' in original
        assert 'COLUMN_1ST = "1st"' in original and 'ORDER = "order"' in original
        assert '#[allow(dead_code' not in original and '@model' not in original
        assert original.index('openrust::model_fields!') < original.index('pub struct FieldReferenceProbe')
        assert original.index('use openrust::') < original.index('openrust::model_fields!')
        assert ('/// 姓名' in original) == (driver != 'sqlite')
        if driver == 'sqlite':
            assert '#[doc =' not in original and original.count('/// ') == 1, '无注释时仅生成默认表名标签'
        for locale, title, label in [('en', 'Model Table and Fields', 'Table'),
                                     ('zh-Hant', '模型資料表與欄位', '表名'),
                                     ('zh-Hans', '模型表与字段', '表名')]:
            environment['APP_LOCALE'] = locale
            before = (generated.read_bytes(), generated.stat().st_mtime_ns)
            artisan('locale-' + locale + '-stale', 'model:fields', 'FieldReferenceProbe', '--check', success=(f'/// {label}\n        TABLE' in generated.read_text()))
            assert before == (generated.read_bytes(), generated.stat().st_mtime_ns)
            artisan('locale-' + locale, 'model:fields', 'FieldReferenceProbe')
            localized = generated.read_text()
            assert '#[generated(command = "artisan model:fields FieldReferenceProbe")]' in localized
            assert '/*\n|---' not in localized
            assert f'/// {label}\n        TABLE' in localized
            assert 'artisan model:fields FieldReferenceProbe' in localized
            assert ('/// 姓名' in localized) == (driver != 'sqlite'), '数据库注释不自动翻译'
            compile_code('locale-' + locale, localized, deny_warnings=True)
            artisan('locale-' + locale + '-check', 'model:fields', 'FieldReferenceProbe', '--check')
        # 语言资源可扩展至任意规范语言；识别旧区不依赖语言枚举或历史文案。
        extra_locales = ['fr', 'de', 'es', 'it', 'pt', 'ja', 'ko', 'ru', 'ar', 'hi', 'th', 'vi', 'tr', 'id', 'nl']
        resources = {}
        try:
            for locale in extra_locales:
                path = source / 'lang' / locale / 'console.json'
                resources[path] = path.read_bytes() if path.exists() else None
                path.parent.mkdir(parents=True, exist_ok=True)
                title = f'Fields {locale}'
                data = {'model_fields': {'title': title, 'notice': 'Database schema',
                                        'sync': 'Sync `./artisan model:fields :model`.', 'table': f'Table-{locale}'}}
                path.write_text(json.dumps(data))
                environment['APP_LOCALE'] = locale
                artisan('resource-' + locale, 'model:fields', 'FieldReferenceProbe')
                text = generated.read_text()
                assert '#[generated(command =' in text and f'/// Table-{locale}\n        TABLE' in text
                assert text.count('openrust::model_fields!') == 1
                artisan('resource-' + locale + '-check', 'model:fields', 'FieldReferenceProbe', '--check')
            # 翻译原文改变也不需要枚举历史译文；注释开闭符不能变成Rust源码。
            path = source / 'lang/fr/console.json'
            data['model_fields']['title'] = 'Nouveau /* titre */'
            path.write_text(json.dumps(data))
            environment['APP_LOCALE'] = 'fr'
            artisan('resource-changed-check', 'model:fields', 'FieldReferenceProbe', '--check')
            artisan('resource-changed-refresh', 'model:fields', 'FieldReferenceProbe')
            text = generated.read_text()
            assert 'Nouveau' not in text and text.count('openrust::model_fields!') == 1
            compile_code('resource-changed', text, deny_warnings=True)
            # 缺少当前语言的词条时，由统一译器按app.fallback_locale回退。
            path.write_text('{}')
            artisan('resource-fallback', 'model:fields', 'FieldReferenceProbe')
            assert '#[generated(command =' in generated.read_text()
        finally:
            environment['APP_LOCALE'] = 'zh-Hans'
            for path, previous in resources.items():
                if previous is None:
                    path.unlink(missing_ok=True)
                else:
                    path.write_bytes(previous)
            artisan('resource-restored', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == original
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
                assert '#[doc =' not in generated.read_text() and generated.read_text().count('/// ') == 1, '空白或删除的列注释仅保留表名说明'
            for label, description in [('table-description', '测试表'),
                                       ('table-special', '表 "注释"\n第二行\r控制字符'),
                                       ('table-blank', ' \t\n'), ('table-empty', '')]:
                escaped = description.replace('\\', '\\\\') if driver == 'mysql' else description
                literal = "'" + escaped.replace("'", "''") + "'"
                statement = (f'alter table {table} comment = {literal}' if driver == 'mysql'
                             else f'comment on table {table} is {literal}')
                sql(label, statement)
                before = (generated.read_bytes(), generated.stat().st_mtime_ns)
                artisan(label + '-check', 'model:fields', 'FieldReferenceProbe', '--check', success=(label == 'table-empty'))
                assert before == (generated.read_bytes(), generated.stat().st_mtime_ns)
                artisan(label + '-refresh', 'model:fields', 'FieldReferenceProbe')
                text = generated.read_text()
                if label == 'table-description':
                    assert '/// 测试表\n        TABLE' in text
                if not description.strip():
                    assert '/// 表名\n        TABLE' in text
                compile_code(label, text + reference, deny_warnings=True)
                artisan(label + '-fresh', 'model:fields', 'FieldReferenceProbe', '--check')
            model = generated.read_text()
        # 真正执行生成模型的查询，包含保留字、NULL 和参数绑定。
        sql('insert', f"insert into {table} (id, name, {quote}order{quote}) values (1, '张三', 2)")
        consumer = compile_code('consumer', model + db_setup + '''
#[tokio::main]
async fn main() {
    let db = database();
    let mut query = FieldReferenceProbe::queryOn(&db);
    query.r#where(FieldReferenceProbe::NAME, "=", "张三");
    query.where_null(FieldReferenceProbe::PARENT_ID);
    let users = query.orderBy(FieldReferenceProbe::ORDER, "asc").get().await.unwrap();
    assert_eq!(FieldReferenceProbe::TABLE, "field_reference_probes");
    assert_eq!(FieldReferenceProbe::from_state(Default::default()).getTable(), FieldReferenceProbe::TABLE);
    let mut strings = FieldReferenceProbe::queryOn(&db);
    strings.r#where("name", "=", "张三");
    assert_eq!(strings.get().await.unwrap().len(), users.len());
    let aliased = db.table(&format!("{} as u", FieldReferenceProbe::TABLE));
    let aliased = aliased.r#where(&format!("u.{}", FieldReferenceProbe::NAME), "=", "张三");
    assert_eq!(aliased.select([format!("u.{}", FieldReferenceProbe::ID).as_str()]).get().await.unwrap().len(), 1);
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
        assert 'EXTRA' in refreshed and 'PARENT_ID' not in refreshed
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
        assert 'artisan model:fields Admin/FieldReferenceProbe' in nested.read_text()
        compile_code('nested-model', nested.read_text() + reference.replace('::NAME', '::FULL_NAME'))
        assert 'ModelMetadata::new(FieldReferenceProbe::TABLE)' in nested.read_text()
        assert f'TABLE = "{table}";' in nested.read_text()
        assert f'.connection("{environment["DB_CONNECTION"]}")' in nested.read_text()
        # 编译真实模型消费者，再经正式 Application::handle_command 调用 Artisan。
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
    Ok(BTreeMap::from([
        ("app".into(), object(&[("locale", std::env::var("APP_LOCALE").unwrap().into()), ("fallback_locale", "en".into())])),
        ("database".into(), object(&[
            ("default", "not_configured".into()),
            ("connections", object(&[(&std::env::var("DB_CONNECTION").unwrap(), configuration)])),
        ])),
    ]))
}
#[tokio::main]
async fn main() {
    let app = openrust::foundation::Application::configure(std::env::var("APP_BASE_PATH").unwrap().into())
        .with_config(load).create().unwrap();
    let status = app.handle_command(&std::env::args_os().skip(1).collect::<Vec<_>>()).await.unwrap();
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
        stale_source = linked.read_text().replace('ModelMetadata::new(LinkedProbe::TABLE)', 'ModelMetadata::new("different_table")')
        linked.write_text(stale_source)
        registered('outdated-metadata', 'LinkedProbe', success=False)
        assert linked.read_text() == stale_source
        linked.write_text(stale_source.replace('ModelMetadata::new("different_table")', 'ModelMetadata::new(LinkedProbe::TABLE)'))
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
        shutil.copytree(source / 'lang', check_root / 'lang', dirs_exist_ok=True)
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
        assert all('REGISTERED_EXTRA' in path.read_text() for path in (linked, nested))
        unchanged = source_snapshot()
        registered('compiled-idempotent', '--all')
        registered('compiled-fresh', '--all', '--check')
        assert source_snapshot() == unchanged, '相同结果不应重写文件'
        registered('connection-override', 'LinkedProbe', '--database=not_configured', success=False)
        assert source_snapshot() == unchanged
        # 配置变化重新编译后使用新表，不能要求每次重新输入 --table。
        sql('other-create', 'create table field_reference_other (id integer, payload integer)')
        try:
            nested.write_text(nested.read_text().replace(f'TABLE = "{table}";', 'TABLE = "field_reference_other";'))
            runner = compile_code('registered', runner_source, executable=True)
            registered('metadata-changed-check', 'Admin/FieldReferenceProbe', '--check', success=False)
            registered('metadata-changed-refresh', 'Admin/FieldReferenceProbe')
            assert 'PAYLOAD' in nested.read_text() and 'REGISTERED_EXTRA' not in nested.read_text()
            registered('metadata-changed-fresh', '--all', '--check')
        finally:
            sql('other-drop', 'drop table field_reference_other')
        sql('registered-drop', f'alter table {table} drop column registered_extra')
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
        # 未编译标准模型首先从源码表名读取，不能误用类名推断的表。
        custom_source = refreshed[refreshed.index('pub struct FieldReferenceProbe'):]
        custom_source = ('use openrust::database::eloquent::{Model, ModelMetadata, ModelState};\n\n' + custom_source).replace(
            'FieldReferenceProbe', 'SourceTableProbe').replace('ModelMetadata::new(SourceTableProbe::TABLE)', f'ModelMetadata::new("{table}")')
        custom_model = source / 'app/models/source_table_probe.rs'
        custom_model.write_text(custom_source)
        try:
            artisan('source-table', 'model:fields', 'SourceTableProbe')
            assert f'TABLE = "{table}";' in custom_model.read_text()
            assert 'ModelMetadata::new(SourceTableProbe::TABLE)' in custom_model.read_text()
            compile_code('source-table', custom_model.read_text(), deny_warnings=True)
            getter = custom_source.replace('impl Model for SourceTableProbe {',
                "impl Model for SourceTableProbe {\n    fn getTable(&self) -> &'static str { \"" + table + "\" }")
            getter = getter.replace(f'ModelMetadata::new("{table}")', 'ModelMetadata::new("fallback")')
            custom_model.write_text(getter)
            artisan('custom-table-getter', 'model:fields', 'SourceTableProbe', f'--table={table}')
            assert 'ModelMetadata::new("fallback")' in custom_model.read_text()
            compile_code('custom-table-getter', custom_model.read_text(), deny_warnings=True)
        finally:
            custom_model.unlink()
        # 旧版内嵌区域迁移到顶部的新分区；边界由 Rust 语法位置确认。
        section = refreshed.index('openrust::model_fields!')
        model_start = refreshed.index('pub struct FieldReferenceProbe')
        business = refreshed[:section] + refreshed[model_start:]
        legacy_block = (
            '// openrust:model-fields begin\n'
            'openrust::database::eloquent::model_fields! {\n'
            '    @model FieldReferenceProbe;\n'
            '    impl FieldReferenceProbe { pub const FULL_NAME = "full_name"; }\n'
            '}\n// openrust:model-fields end\n')
        legacy = business.replace('ModelMetadata::new(FieldReferenceProbe::TABLE)', f'ModelMetadata::new("{table}")') + '\n' + legacy_block
        generated.write_text(legacy)
        artisan('legacy-check', 'model:fields', 'FieldReferenceProbe', '--check', success=False)
        assert generated.read_text() == legacy
        artisan('legacy-migrate', 'model:fields', 'FieldReferenceProbe')
        migrated = generated.read_text()
        assert 'openrust:model-fields' not in migrated and '@model' not in migrated
        assert 'ModelMetadata::new(FieldReferenceProbe::TABLE)' in migrated and migrated.count(f'"{table}"') == 1
        assert migrated.index('openrust::model_fields!') < migrated.index('pub struct FieldReferenceProbe')
        assert migrated.endswith('// 保留业务代码\n')
        compile_code('migrated', migrated + reference.replace('::NAME', '::FULL_NAME'), deny_warnings=True)
        for label, broken in [
            ('legacy-business-item', legacy.replace('// openrust:model-fields end', 'pub fn business_logic() -> bool { true }\n// openrust:model-fields end')),
            ('legacy-wrong-model', legacy.replace('@model FieldReferenceProbe;', '@model AnotherProbe;')),
            ('legacy-wrong-owner', legacy.replace('impl FieldReferenceProbe { pub const', 'impl AnotherProbe { pub const')),
            ('legacy-macro-attribute', legacy.replace('openrust::database::eloquent::model_fields!', '#[cfg(any())]\nopenrust::database::eloquent::model_fields!')),
            ('damaged', legacy.replace('// openrust:model-fields end', '// removed end')),
            ('duplicate-marker', legacy + '// openrust:model-fields begin\n'),
            ('reversed-marker', legacy.replace('// openrust:model-fields begin', '// openrust:model-fields TEMP').replace('// openrust:model-fields end', '// openrust:model-fields begin').replace('// openrust:model-fields TEMP', '// openrust:model-fields end')),
            ('duplicate-section', refreshed + refreshed[section:model_start]),
            ('new-unknown-argument', refreshed.replace('#[generated(command =', '#[generated(unknown =', 1)),
            ('new-wrong-owner', refreshed.replace('    FieldReferenceProbe {', '    AnotherProbe {', 1)),
            ('new-duplicate-declaration', refreshed + '\nopenrust::model_fields! { FieldReferenceProbe { ID = "id"; } }\n'),
            ('new-duplicate-metadata', refreshed.replace('    #[generated(', '    #[generated(command = "duplicate")]\n    #[generated(', 1)),

            ('damaged-section', refreshed.replace('#[generated(command =', '#[generated(broken =', 1)),
            ('missing-generated-macro', refreshed.replace('openrust::model_fields!', 'openrust::missing_fields!', 1)),
            ('renamed-generated-macro', refreshed.replace('openrust::model_fields!', 'openrust::other_fields!')),
            ('invalid-rust', refreshed + '\nfn incomplete(\n'),
        ]:
            generated.write_text(broken)
            artisan(label, 'model:fields', 'FieldReferenceProbe', success=False)
            assert generated.read_text() == broken
        # 改标题或字段说明仍定位同一宏；刷新恢复数据库生成内容，不追加第二份。
        edited = refreshed.replace('| 模型表与字段', '| 我的模型字段').replace('FULL_NAME = "full_name";', 'FULL_NAME = "edited_name";')
        generated.write_text(edited)
        artisan('edited-generated-region', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == refreshed
        artisan('edited-generated-region-check', 'model:fields', 'FieldReferenceProbe', '--check')
        # 完整删除说明可恢复；连同宏一起删除时，已编译命令仍能恢复源码。
        without_header = refreshed.replace('    #[generated(command = "artisan model:fields FieldReferenceProbe")]\n', '')
        generated.write_text(without_header)
        artisan('deleted-header-refresh', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == refreshed
        without_block = refreshed[:section] + refreshed[model_start:]
        generated.write_text(without_block)
        compile_code('deleted-fields-do-not-compile', without_block + reference.replace('::NAME', '::FULL_NAME'), False)
        artisan('deleted-block-recover', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == refreshed
        compile_code('recovered-fields-compile', generated.read_text() + reference.replace('::NAME', '::FULL_NAME'))
        artisan('recovered-fields-repeat', 'model:fields', 'FieldReferenceProbe', '--check')
        business_comment = refreshed[:section] + '/* 业务说明，不能被生成器删除或移动。 */\n' + refreshed[section:]
        generated.write_text(business_comment)
        artisan('metadata-preserves-neighboring-comment', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == business_comment
        generated.write_text(refreshed.replace('command = "artisan model:fields FieldReferenceProbe"', 'command = "artisan model:fields FieldReferenceProbe",'))
        artisan('metadata-trailing-comma', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == refreshed
        # 上一版顶部紧凑布局可原地升级，不留下第二份分区或表名字符串。
        old_header = '/*\n|--------------------------------------------------------------------------\n| 模型字段\n|--------------------------------------------------------------------------\n|\n| 根据数据库表结构生成字段与说明，通过 model:fields 命令同步。\n| 调整表结构或列说明后，请重新同步并构建；不要直接修改此区域。\n|\n*/\n'
        old_compact = refreshed[:section] + old_header + refreshed[section:]
        old_compact = old_compact.replace('    #[generated(command = "artisan model:fields FieldReferenceProbe")]\n', '').replace(
            f'        TABLE = "{table}";\n', '').replace('ModelMetadata::new(FieldReferenceProbe::TABLE)', f'ModelMetadata::new("{table}")')
        generated.write_text(old_compact)
        artisan('compact-upgrade-check', 'model:fields', 'FieldReferenceProbe', '--check', success=False)
        assert generated.read_text() == old_compact
        artisan('compact-upgrade', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == refreshed
        artisan('compact-upgrade-stable', 'model:fields', 'FieldReferenceProbe', '--check')
        # 不在 derive / 文档属性和模型类型之间插入；真实语法解析避开字符串中的括号。
        decorated = ('//! 模型模块说明\n#![allow(dead_code)]\n' + business).replace(
            'pub struct FieldReferenceProbe',
            'pub const EXAMPLE: &str = r###"括号 } { 与 \"引号\""###;\n'
            '/// 模型类型说明\n#[derive(Default)]\npub struct FieldReferenceProbe')
        generated.write_text(decorated)
        artisan('decorated-refresh', 'model:fields', 'FieldReferenceProbe')
        decorated = generated.read_text()
        assert decorated.startswith('//! 模型模块说明\n#![allow(dead_code)]\n')
        assert decorated.index('openrust::model_fields!') < decorated.index('pub const EXAMPLE')
        assert '/// 模型类型说明\n#[derive(Default)]\npub struct FieldReferenceProbe' in decorated
        compile_code('decorated', decorated + reference.replace('::NAME', '::FULL_NAME'), deny_warnings=True)
        artisan('decorated-idempotent', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text() == decorated
        # BOM / shebang 的字节偏移不得导致源码被截断。
        generated.write_text('\ufeff#!/usr/bin/env rust-script\n' + decorated)
        artisan('bom-shebang', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text().startswith('\ufeff#!/usr/bin/env rust-script\n//! 模型模块说明')
        compile_code('bom-shebang', generated.read_text() + reference.replace('::NAME', '::FULL_NAME'), deny_warnings=True)
        # Windows 换行的生成区可刷新，业务源码字节保持不变。
        windows = refreshed.replace('\n', '\r\n')
        generated.write_bytes(windows.encode())
        artisan('crlf-refresh', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_bytes().endswith(business[business.index('pub struct'):].replace('\n', '\r\n').encode())
        stable = generated.read_bytes()
        artisan('crlf-idempotent', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_bytes() == stable
        # 导入与模型写在同一行时，生成区仍位于类型之前。
        inline = business.replace(';\n\npub struct', '; pub struct', 1)
        generated.write_text(inline)
        artisan('inline-import', 'model:fields', 'FieldReferenceProbe')
        assert generated.read_text().index('openrust::model_fields!') < generated.read_text().index('pub struct')
        compile_code('inline-import', generated.read_text() + reference.replace('::NAME', '::FULL_NAME'), deny_warnings=True)
        # 无导入的模型模块保留文件文档、inner 属性与类型文档。
        no_import = business.split(';\n', 1)[1].lstrip()
        no_import = re.sub(r'\b(ModelState|ModelMetadata|Model)\b',
                           lambda match: 'openrust::database::eloquent::' + match[0], no_import)
        no_import = '//! 模型模块说明\n#![allow(dead_code)]\n/// 模型类型说明\n' + no_import
        generated.write_text(no_import)
        artisan('no-import', 'model:fields', 'FieldReferenceProbe')
        assert '/// 模型类型说明\npub struct' in generated.read_text()
        compile_code('no-import', generated.read_text() + reference.replace('::NAME', '::FULL_NAME'), deny_warnings=True)
        # 原始字符串中的旧边界文本不得被误当作生成区而删除。
        raw_markers = ('\npub const MARKER_EXAMPLE: &str = r###"\n'
                       '// openrust:model-fields begin\n保留示例文本\n'
                       '// openrust:model-fields end\n'
                       '// @auto-generated:model-fields:begin\n保留新标记示例\n'
                       '// @auto-generated:model-fields:end\n"###;\n')
        generated.write_text(business + raw_markers)
        artisan('raw-markers', 'model:fields', 'FieldReferenceProbe')
        assert raw_markers in generated.read_text()
        compile_code('raw-markers', generated.read_text() + reference.replace('::NAME', '::FULL_NAME'), deny_warnings=True)
        generated.write_text(refreshed)
        sql('table-column-create', f'create table field_table_column (zeta integer, {quote}table{quote} integer, id integer primary key, alpha integer)')
        try:
            artisan('table-column', 'make:model', 'TableColumnProbe', '--force', '--fields', '--table=field_table_column')
            probe = (source / 'app/models/table_column_probe.rs').read_text()
            declarations = re.findall(r'^        ([A-Z_0-9]+) = (".*");$', probe, re.MULTILINE)
            assert [(field, json.loads(value)) for field, value in declarations] == [
                ('TABLE', 'field_table_column'), ('ZETA', 'zeta'), ('COLUMN_TABLE', 'table'), ('ID', 'id'), ('ALPHA', 'alpha')]
            compile_code('table-column', probe + "\npub fn reference() -> &'static str { TableColumnProbe::COLUMN_TABLE }\n", deny_warnings=True)
            # 表名覆盖同步到 TABLE，标准元数据仍引用同一常量。
            artisan('table-override', 'model:fields', 'FieldReferenceProbe', '--table=field_table_column')
            override = generated.read_text()
            assert 'TABLE = "field_table_column";' in override and 'ModelMetadata::new(FieldReferenceProbe::TABLE)' in override
            assert table not in override
            compile_code('table-override', override + "\npub fn reference() -> &'static str { FieldReferenceProbe::TABLE }\n", deny_warnings=True)
        finally:
            sql('table-column-drop', 'drop table field_table_column')
            generated.write_text(refreshed)
        sql('table-collision-create', f'create table field_table_collision ({quote}table{quote} integer, column_table integer)')
        try:
            artisan('table-collision', 'model:fields', 'FieldReferenceProbe', '--table=field_table_collision', success=False)
            assert generated.read_text() == refreshed
        finally:
            sql('table-collision-drop', 'drop table field_table_collision')
        # 手写 TABLE 在生成区外时拒绝覆盖或制造重复关联常量。
        manual_table = refreshed + '\nimpl FieldReferenceProbe { pub const TABLE: &str = "manual"; }\n'
        generated.write_text(manual_table)
        artisan('manual-table-conflict', 'model:fields', 'FieldReferenceProbe', success=False)
        assert generated.read_text() == manual_table
        generated.write_text(refreshed)
        # 表结构重建后的列顺序变化也属于过期内容；主键不享有特殊展示位置。
        order_model = source / 'app/models/order_probe.rs'
        sql('order-create', 'create table field_order_probes (zeta integer, id integer primary key, alpha integer)')
        try:
            artisan('order-generate', 'make:model', 'OrderProbe', '--force', '--fields', '--table=field_order_probes')
            sql('order-recreate-drop', 'drop table field_order_probes')
            sql('order-recreate', 'create table field_order_probes (alpha integer, zeta integer, id integer primary key)')
            before_order = (order_model.read_bytes(), order_model.stat().st_mtime_ns)
            result = artisan(
                'order-stale', 'model:fields', 'OrderProbe', '--check', success=False)
            assert result.returncode == 1
            assert before_order == (order_model.read_bytes(), order_model.stat().st_mtime_ns)
            artisan('order-refresh', 'model:fields', 'OrderProbe')
            assert re.findall(r'^        ([A-Z_0-9]+) = ', order_model.read_text(), re.MULTILINE) == ['TABLE', 'ALPHA', 'ZETA', 'ID']
            compile_code('order-model', order_model.read_text(), deny_warnings=True)
            artisan('order-fresh', 'model:fields', 'OrderProbe', '--check')
        finally:
            sql('order-drop', 'drop table field_order_probes')
            order_model.unlink(missing_ok=True)
        # 常量保存逻辑表名，连接前缀继续由既有 Schema 和查询语法器处理。
        prefix_model = source / 'app/models/prefix_probe.rs'
        sql('prefix-create', 'create table tenant_field_prefix_probes (id integer primary key, title varchar(80))')
        sql('prefix-insert', "insert into tenant_field_prefix_probes (id, title) values (1, '前缀表')")
        try:
            if driver != 'sqlite':
                statement = ("alter table tenant_field_prefix_probes comment = '租户记录'" if driver == 'mysql'
                             else "comment on table tenant_field_prefix_probes is '租户记录'")
                sql('prefix-comment', statement)
            environment['DB_PREFIX'] = 'tenant_'
            artisan('prefix-generate', 'make:model', 'PrefixProbe', '--force', '--fields', '--table=field_prefix_probes')
            text = prefix_model.read_text()
            assert 'TABLE = "field_prefix_probes";' in text
            assert ('/// 租户记录' in text) == (driver != 'sqlite')
            consumer = compile_code('prefix-consumer', text + db_setup + '''
#[tokio::main]
async fn main() {
    let db = database();
    let mut query = PrefixProbe::queryOn(&db);
    query.r#where(PrefixProbe::TITLE, "=", "前缀表");
    assert_eq!(query.get().await.unwrap().len(), 1);
    assert_eq!(db.table(PrefixProbe::TABLE).get().await.unwrap().len(), 1);
}
''', executable=True)
            run('prefix-consumer-run', [str(consumer)])
            artisan('prefix-fresh', 'model:fields', 'PrefixProbe', '--check')
        finally:
            environment['DB_PREFIX'] = ''
            sql('prefix-drop', 'drop table tenant_field_prefix_probes')
            prefix_model.unlink(missing_ok=True)
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
            compile_code('custom-stub-model', stub_model.read_text(), deny_warnings=True)
            stub_model.unlink()
            custom_stub.write_text(template.replace('{{table_reference}}', '"{{table}}"'))
            artisan('legacy-custom-stub', 'make:model', 'StubProbe', '--fields',
                    f'--table={table}', f'--database={environment["DB_CONNECTION"]}')
            assert 'ModelMetadata::new(StubProbe::TABLE)' in stub_model.read_text()
            assert stub_model.read_text().count(f'"{table}"') == 1
            compile_code('legacy-custom-stub-model', stub_model.read_text(), deny_warnings=True)
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
