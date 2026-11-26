"""运行正式 Artisan 生成器，将原始生成文件编译为实际 HTTP 消费者。"""
import argparse
import hashlib
import json
import os
import re
import shlex
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source', required=True, type=Path)
parser.add_argument('--evidence-dir', type=Path, default=root / 'target/tmp/request-generator-tutorial')
args = parser.parse_args()
source = args.source.resolve()
if not source.is_relative_to(root / 'target/tmp') or not (source / 'artisan').is_file():
    parser.error('只能在项目 target/tmp/ 下的隔离应用执行')
evidence = args.evidence_dir.resolve()
if not evidence.is_relative_to(root / 'target/tmp'):
    parser.error('验收记录目录必须位于项目 target/tmp/ 内')
evidence.mkdir(parents=True, exist_ok=True)
fixture = source / 'vendor/openrust-framework/tests/fixtures/request_generated'
fixture.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(dir=evidence, prefix='request-consumer-') as temp:
    app = Path(temp)
    env = dict(os.environ, APP_BASE_PATH=str(app))
    commands = []
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
        text = (source / 'docs' / locale / 'the-basics/validation.md').read_text().split('{#creating-form-requests}', 1)[1]
        blocks = re.findall(r'```sh\n(.*?)```', text, re.S)
        recipe = blocks[0].strip().splitlines()
        assert len(recipe) == 2
        for line in recipe:
            words = shlex.split(line)
            assert words[:2] == ['./artisan', 'make:request']
            artisan(*words[1:])
        artifact = local / 'app/http/requests/store_article_request.rs'
        assert (local / 'app/http/requests/admin/store_article_request.rs').is_file()
        generated.append(artifact.read_bytes())
    assert generated[0] == generated[1] == generated[2]
    (fixture / 'denied.rs').write_bytes(generated[0])
    env['APP_BASE_PATH'] = str(app)
    active_locale = None
    for name in ('Request', 'FormRequest', 'Box', 'Option', 'Vec', 'Result'):
        artisan('make:request', name)
        shutil.copy2(app / 'app/http/requests' / (('form_request' if name == 'FormRequest' else name.lower()) + '.rs'), fixture / ('collision_' + name.lower() + '.rs'))
    artisan('stub:publish')
    stub = app / 'stubs/request.rs.stub'
    text = stub.read_text()
    start = text.index('        ::std::boxed::Box::pin(async {')
    end = text.index('\n        })', start) + len('\n        })')
    text = text[:start] + '        ::std::boxed::Box::pin(async { ::std::result::Result::Ok(()) })' + text[end:]
    text = text.replace('::std::result::Result::Ok(validation::RuleSet::new())', 'validation::RuleSet::parse([("title", "required|string|min:3")])')
    text = subprocess.check_output(['rustfmt', '+1.95.0', '--edition', '2024', '--emit', 'stdout'], input=text.replace('{{class}}', 'GeneratedRequest').encode()).decode().replace('GeneratedRequest', '{{class}}')
    stub.write_text(text)
    artisan('make:request', 'ApprovedRequest')
    shutil.copy2(app / 'app/http/requests/approved_request.rs', fixture / 'approved.rs')
    (evidence / 'request-generator-commands.json').write_text(json.dumps({'commands': commands, 'sha256': {f.name: hashlib.sha256(f.read_bytes()).hexdigest() for f in fixture.glob('*.rs')}}, ensure_ascii=False, indent=2))

support = (source / 'vendor/openrust-framework/tests/request_signature_tutorial.rs').read_text()
transport = support[support.index('async fn transport('):support.index('async fn start(')]
send = support[support.index('async fn send('):support.index('#[tokio::test]')]
modules = ''.join(f'#[path="fixtures/request_generated/collision_{name.lower()}.rs"]mod collision_{name.lower()};\n' for name in ('Request', 'FormRequest', 'Box', 'Option', 'Vec', 'Result'))
code = '''use axum::{extract::State,handler::Handler as _};
use openrust::{contracts::http::Kernel as _,foundation::{Application,configuration::Routing,http::{Kernel,ValidatedFormRequest}},http::{HttpError,Response,StatusCode},routing::Router};
use serde_json::{Value,json};use std::{collections::BTreeMap,net::SocketAddr,sync::Arc,time::Duration};
use tokio::{net::{TcpListener,TcpStream},io::{AsyncReadExt,AsyncWriteExt},time::timeout};
#[path="fixtures/request_generated/denied.rs"]mod denied;
#[path="fixtures/request_generated/approved.rs"]mod approved;
async fn deny(_:ValidatedFormRequest<denied::StoreArticleRequest>)->Result<Response,HttpError>{panic!("默认生成的表单必须先获得授权");}
async fn approve(form:ValidatedFormRequest<approved::ApprovedRequest>)->Result<Response,HttpError>{Response::json(StatusCode::CREATED,form.into_parts().1.into_value())}
''' + modules + '''
async fn collision<F:openrust::foundation::http::FormRequest+Default+'static>(_:ValidatedFormRequest<F>)->Result<Response,HttpError>{panic!("默认表单必须拒绝");}
''' + transport + send + '''
#[tokio::test]
async fn generated_request_types_authorize_and_validate_real_http_before_dispatch(){
 let root=tempfile::tempdir().unwrap();let app=Application::configure(root.path().to_owned()).with_config(|_,_|Ok(BTreeMap::from([("app".into(),json!({"locale":"en","supported_locales":[]}))])))
 .with_routing(Routing::new(|_|{let mut r=Router::new();r.post("/deny",deny)?;r.post("/approve",approve)?;r.post("/collision/Request",collision::<collision_request::Request>)?;r.post("/collision/FormRequest",collision::<collision_formrequest::FormRequest>)?;r.post("/collision/Box",collision::<collision_box::Box>)?;r.post("/collision/Option",collision::<collision_option::Option>)?;r.post("/collision/Vec",collision::<collision_vec::Vec>)?;r.post("/collision/Result",collision::<collision_result::Result>)?;Ok(r)})).create().unwrap();
 let mut kernel=Kernel::new(app);kernel.bootstrap().await.unwrap();let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();let service=transport.with_state(Arc::new(kernel)).into_make_service();let server=tokio::spawn(async move{axum::serve(listener,service).await.unwrap()});
 for path in ["/collision/Request","/collision/FormRequest","/collision/Box","/collision/Option","/collision/Vec","/collision/Result"]{assert_eq!(send(address,"POST",path,"{}","").await.0,403);}
 assert_eq!(send(address,"POST","/deny",r#"{"title":"Hello"}"#,"").await.0,403);
 for body in ["{}",r#"{"title":"x"}"#]{let (status,data,_)=send(address,"POST","/approve",body,"").await;assert_eq!(status,422,"{data}");assert!(data["errors"]["title"].is_array());}
 let (status,data,_)=send(address,"POST","/approve",r#"{"title":"Hello","admin":true}"#,"").await;assert_eq!(status,201);assert_eq!(data,json!({"title":"Hello"}));server.abort();let _=server.await;
}
'''
target = source / 'vendor/openrust-framework/tests/request_generated_tutorial.rs'
target.write_text(code)
subprocess.run(['rustfmt', '+1.95.0', '--edition', '2024', str(target), *map(str, fixture.glob('*.rs'))], check=True)
for name, expected in json.loads((evidence / 'request-generator-commands.json').read_text())['sha256'].items():
    assert hashlib.sha256((fixture / name).read_bytes()).hexdigest() == expected, name
subprocess.run(['cargo', 'test', '--manifest-path', str(source / 'Cargo.toml'), '-p', 'openrust-framework', '--test', 'request_generated_tutorial', '--locked'], check=True)
