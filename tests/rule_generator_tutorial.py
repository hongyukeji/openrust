"""从三语教程运行实际规则生成器，将原始生成代码用于真实 HTTP 验收。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source', required=True, type=Path)
parser.add_argument('--evidence-dir', type=Path, default=root / 'target/tmp/rule-generator-tutorial')
args = parser.parse_args()
source = args.source.resolve()
evidence = args.evidence_dir.resolve()
if not source.is_relative_to(root / 'target/tmp') or not (source / 'artisan').is_file():
    parser.error('只能在项目 target/tmp/ 下的隔离应用执行')
if not evidence.is_relative_to(root / 'target/tmp'):
    parser.error('验收记录目录必须位于项目 target/tmp/ 内')
evidence.mkdir(parents=True, exist_ok=True)
fixture = source / 'vendor/openrust-framework/tests/fixtures/rule_generated'
fixture.mkdir(parents=True, exist_ok=True)
commands = []
collisions = {'RuleContext': 'rule_context', 'Value': 'value', 'Box': 'box', 'Option': 'option', 'Vec': 'vec', 'Result': 'result'}
with tempfile.TemporaryDirectory(dir=evidence, prefix='rule-consumer-') as temp:
    app = Path(temp)
    env = dict(os.environ, APP_BASE_PATH=str(app))
    active_locale = None

    def artisan(*arguments):
        result = subprocess.run([str(source / 'artisan'), *arguments], cwd=source, env=env, capture_output=True, text=True)
        commands.append({'locale': active_locale, 'args': list(arguments), 'status': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr})
        assert result.returncode == 0, commands[-1]

    generated = []
    for locale in ('en', 'zh-Hans', 'zh-Hant'):
        active_locale = locale
        local = app / locale
        local.mkdir()
        env['APP_BASE_PATH'] = str(local)
        section = (source / 'docs' / locale / 'the-basics/validation.md').read_text().split('{#creating-custom-rules}', 1)[1]
        recipe = re.findall(r'```sh\n(.*?)```', section, re.S)[0].strip().splitlines()
        assert len(recipe) == 2
        for line in recipe:
            words = shlex.split(line)
            assert words[:2] == ['./artisan', 'make:rule']
            artisan(*words[1:])
        generated.append([(local / 'app/rules' / name).read_bytes() for name in ('uppercase.rs', 'required_title.rs')])
    assert generated[0] == generated[1] == generated[2]
    for name, data in zip(('plain.rs', 'implicit.rs'), generated[0]):
        (fixture / name).write_bytes(data)
    active_locale = None
    env['APP_BASE_PATH'] = str(app)
    for name, module in collisions.items():
        artisan('make:rule', name, '--implicit')
        shutil.copy2(app / 'app/rules' / (module + '.rs'), fixture / ('collision_' + module + '.rs'))
    artisan('stub:publish')
    for name in ('rule.rs.stub', 'rule.implicit.rs.stub'):
        stub = app / 'stubs' / name
        text = stub.read_text().replace('_attribute:', 'attribute:').replace('_value:', 'value:')
        text = text.replace('Box::pin(async {', 'Box::pin(async move {').replace(
            '::std::result::Result::Ok(::std::vec::Vec::new())',
            '::std::result::Result::Ok(if value.is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty())) { vec![validation::RuleFailure::new(format!("The {attribute} is required."))] } else { ::std::vec::Vec::new() })')
        text = subprocess.check_output(['rustfmt', '+1.95.0', '--edition', '2024', '--emit', 'stdout'], input=text.replace('{{class}}', 'GeneratedRule').encode()).decode().replace('GeneratedRule', '{{class}}')
        stub.write_text(text)
    artisan('make:rule', 'Checked')
    artisan('make:rule', 'ImplicitChecked', '--implicit')
    for name in ('checked', 'implicit_checked'):
        shutil.copy2(app / 'app/rules' / (name + '.rs'), fixture / (name + '.rs'))
    hashes = {f.name: hashlib.sha256(f.read_bytes()).hexdigest() for f in fixture.glob('*.rs')}
    (evidence / 'rule-generator-commands.json').write_text(json.dumps({'commands': commands, 'sha256': hashes}, ensure_ascii=False, indent=2))

support = (source / 'vendor/openrust-framework/tests/validation_rule_tutorial.rs').read_text()
transport = support[support.index('async fn transport('):support.index('async fn start(')]
modules = ''.join(f'#[path="fixtures/rule_generated/collision_{module}.rs"]mod collision_{module};\n' for module in collisions.values())
checks = ''.join(f'assert!(collision_{module}::{name}.is_implicit());' for name, module in collisions.items())
code = '''use axum::{extract::State,handler::Handler as _};
use openrust::{contracts::{http::Kernel as _,validation::ValidationRule},foundation::{Application,configuration::Routing,http::Kernel},http::{HttpError,HttpFuture,Request,Response,StatusCode},routing::Router,validation::{Data,Rule,RuleSet},support::facades::Validator};
use serde_json::{Value,json};use std::{collections::BTreeMap,net::SocketAddr,sync::Arc,time::Duration};
use tokio::{net::{TcpListener,TcpStream},io::{AsyncReadExt,AsyncWriteExt},time::timeout};
#[path="fixtures/rule_generated/plain.rs"]mod plain;
#[path="fixtures/rule_generated/implicit.rs"]mod implicit;
#[path="fixtures/rule_generated/checked.rs"]mod checked;
#[path="fixtures/rule_generated/implicit_checked.rs"]mod implicit_checked;
''' + modules + '''
fn validate(request:&mut Request)->HttpFuture<'_,Result<Response,HttpError>>{Box::pin(async move{
 let input:Value=serde_json::from_slice(&request.body_bytes().await?)?;
 let path=request.uri().path();let custom=match path{
 "/plain"=>Rule::custom(plain::Uppercase),"/implicit"=>Rule::custom(implicit::RequiredTitle),"/checked"|"/nullable"=>Rule::custom(checked::Checked),_=>Rule::custom(implicit_checked::ImplicitChecked)};
 let mut rules=vec![custom];if path.contains("nullable"){rules.insert(0,Rule::nullable());}
 let mut validator=Validator::make(Data::new(input)?,RuleSet::new().field("title",rules)?)?;
 let data=Box::pin(validator.validate_request(request)).await?;Response::json(StatusCode::OK,data.into_value())
})}
''' + transport + '''
#[tokio::test]async fn generated_rules_execute_through_the_real_http_validator(){
 assert!(!plain::Uppercase.is_implicit());assert!(implicit::RequiredTitle.is_implicit());
''' + checks + '''
 let root=tempfile::tempdir().unwrap();let app=Application::configure(root.path().to_owned()).with_config(|_,_|Ok(BTreeMap::from([("app".into(),json!({"locale":"en","supported_locales":[]}))])))
 .with_routing(Routing::new(|_|{let mut r=Router::new();for path in ["/plain","/implicit","/checked","/nullable","/implicit-checked","/implicit-nullable"]{r.post(path,validate)?;}Ok(r)})).create().unwrap();
 let mut kernel=Kernel::new(app);kernel.bootstrap().await.unwrap();let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();let service=transport.with_state(Arc::new(kernel)).into_make_service();let server=tokio::spawn(async move{axum::serve(listener,service).await.unwrap()});
 for path in ["/plain","/implicit"]{for body in ["{}",r#"{"title":null}"#,r#"{"title":""}"#]{assert_eq!(send(address,"POST",path,body,"").await.0,200);}}
 for body in ["{}",r#"{"title":""}"#]{assert_eq!(send(address,"POST","/checked",body,"").await.0,200);}
 assert_eq!(send(address,"POST","/nullable",r#"{"title":null}"#,"").await.0,200);
 for path in ["/implicit-checked","/implicit-nullable"]{for body in ["{}",r#"{"title":null}"#,r#"{"title":""}"#]{let(status,data,_)=send(address,"POST",path,body,"").await;assert_eq!(status,422,"{path} {body} {data}");assert_eq!(data["errors"]["title"][0],"The title is required.");}assert_eq!(send(address,"POST",path,r#"{"title":"Hello"}"#,"").await.0,200);}
 server.abort();let _=server.await;
}
'''
target = source / 'vendor/openrust-framework/tests/rule_generated_tutorial.rs'
target.write_text(code)
subprocess.run(['rustfmt', '+1.95.0', '--edition', '2024', str(target), *map(str, fixture.glob('*.rs'))], check=True)
for name, expected in hashes.items():
    assert hashlib.sha256((fixture / name).read_bytes()).hexdigest() == expected, name
subprocess.run(['cargo', 'test', '--manifest-path', str(source / 'Cargo.toml'), '-p', 'openrust-framework', '--test', 'rule_generated_tutorial', '--locked'], check=True)
