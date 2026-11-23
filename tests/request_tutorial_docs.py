"""从隔离快照的三语请求教程提取正式示例，编译并运行原生 HTTP Kernel 消费者。"""
import argparse
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source', required=True, type=Path)
args = parser.parse_args()
source = args.source.resolve()
if not source.is_relative_to(root / 'target/tmp') or not (source / 'Cargo.toml').is_file():
    parser.error('只能向项目 target/tmp/ 内的隔离源码生成测试')


def block(locale, topic, function, anchor=None):
    text = (source / 'docs' / locale / 'the-basics' / (topic + '.md')).read_text()
    if anchor is None:
        anchor = '{#validating-signed-route-requests}' if topic == 'urls' else '{#validating-incoming-requests}'
    text = text.split(anchor, 1)[1].split('\n## ', 1)[0]
    blocks = re.findall(r'```rust[^\n]*\n(.*?)```', text, re.S)
    found = [code for code in blocks if 'fn ' + function + '(' in code]
    assert len(found) == 1, (locale, topic, function, len(found))
    return found[0]


code = '''use openrust::{contracts::http::Kernel as _,foundation::{Application,configuration::Routing,http::Kernel},http::{Body,Request,StatusCode},routing::{Router,RoutingError}};
use serde_json::{Value,json};
async fn call(kernel:&Kernel,method:&str,path:&str,body:&str)->(StatusCode,Value) {
 let raw=http::Request::builder().method(method).uri(path).header("host","tutorial.test").header("accept","application/json").header("content-type","application/json").body(Body::from(body.to_owned())).unwrap();
 let response=kernel.handle(&mut Request::from(raw)).await;
 let status=response.status();let bytes=axum::body::to_bytes(response.into_parts().1,65536).await.unwrap();
 (status,serde_json::from_slice(&bytes).unwrap_or_else(|_|json!(String::from_utf8_lossy(&bytes))))
}
async fn verify(register:fn(&Application)->Result<Router,RoutingError>) {
 let root=tempfile::tempdir().unwrap();let app=Application::configure(root.path().to_owned()).with_config(|_,_|Ok(std::collections::BTreeMap::from([
 ("app".into(),json!({"key":"a".repeat(32),"locale":"en","supported_locales":[],"url":"http://tutorial.test","url_explicit":false})),
 ]))).with_routing(Routing::new(register)).create().unwrap();
 let mut kernel=Kernel::new(app);kernel.bootstrap().await.unwrap();
 assert_eq!(call(&kernel,"GET","http://tutorial.test/unsubscribe?email=signed@example.test", "").await.0,StatusCode::UNAUTHORIZED);
 let (_,link)=call(&kernel,"GET","http://tutorial.test/link", "").await;
 let (status,value)=call(&kernel,"GET",link.as_str().unwrap(),r#"{"email":"body@example.test"}"#).await;
 assert_eq!(status,StatusCode::OK);assert_eq!(value,"signed@example.test");
 let (status,value)=call(&kernel,"POST","http://tutorial.test/posts",r#"{"title":"Hello","admin":true}"#).await;
 assert_eq!(status,StatusCode::CREATED);assert_eq!(value,json!({"title":"Hello"}));
 assert_eq!(call(&kernel,"POST","http://tutorial.test/posts","{}").await.0,StatusCode::UNPROCESSABLE_ENTITY);
 let (status,value)=call(&kernel,"POST","http://tutorial.test/custom-posts","{}").await;
 assert_eq!(status,StatusCode::UNPROCESSABLE_ENTITY);assert_eq!(value["errors"]["title"][0],"Please provide article title.");
 let (status,value)=call(&kernel,"POST","http://tutorial.test/custom-posts",r#"{"title":"Hello","admin":true}"#).await;
 assert_eq!(status,StatusCode::CREATED);assert_eq!(value,json!({"title":"Hello"}));
 let (status,value)=call(&kernel,"POST","http://tutorial.test/strict-posts",r#"{"title":"Hello","admin":true}"#).await;
 assert_eq!(status,StatusCode::UNPROCESSABLE_ENTITY);assert!(value["errors"]["admin"].is_array());
 let (status,value)=call(&kernel,"POST","http://tutorial.test/strict-posts",r#"{"title":"Hello"}"#).await;
 assert_eq!(status,StatusCode::CREATED);assert_eq!(value,json!({"title":"Hello"}));
 let (status,value)=call(&kernel,"POST","http://tutorial.test/callback-posts",r#"{"title":"foo"}"#).await;
 assert_eq!(status,StatusCode::UNPROCESSABLE_ENTITY);assert_eq!(value["errors"]["title"][0],"The title is invalid.");
 for path in ["/callback-posts","/implicit-posts"] {
  let (status,value)=call(&kernel,"POST",&format!("http://tutorial.test{path}"),r#"{"title":"Hello","admin":true}"#).await;
  assert_eq!(status,StatusCode::CREATED);assert_eq!(value,json!({"title":"Hello"}));
 }
 for body in ["{}",r#"{"title":""}"#] {let (status,value)=call(&kernel,"POST","http://tutorial.test/implicit-posts",body).await;assert_eq!(status,StatusCode::UNPROCESSABLE_ENTITY);assert!(value["errors"]["title"].is_array());}
 let data=json!({"title":"x".repeat(256)}).to_string();assert_eq!(call(&kernel,"POST","http://tutorial.test/posts",&data).await.0,StatusCode::UNPROCESSABLE_ENTITY);
}
'''
for locale in ('en', 'zh-Hans', 'zh-Hant'):
    name = locale.lower().replace('-', '_')
    code += 'mod ' + name + ' {\nmod urls {\n' + block(locale, 'urls', 'unsubscribe')
    code += '''pub fn register(router:&mut openrust::routing::Router)->Result<(),openrust::routing::RoutingError> {
 router.get("/unsubscribe",unsubscribe)?.name("unsubscribe");
 router.get("/link",||async {Ok::<_,HttpError>(openrust::support::facades::Url::signed_route("unsubscribe",&[openrust::routing::RouteParameter::named("email","signed@example.test")],None,openrust::routing::SignatureMode::Absolute)?)})?;Ok(())
}}
mod validation {
''' + block(locale, 'validation', 'store') + '\nmod messages {\n' + block(locale, 'validation', 'store_with_messages', '{#custom-validation-messages}').replace('fn store_with_messages(', 'pub(super) fn store_with_messages(', 1) + '\n}\nmod unknown_fields {\n' + block(locale, 'validation', 'store_strict', '{#failing-on-unknown-fields}').replace('fn store_strict(', 'pub(super) fn store_strict(', 1) + '\n}\nmod callbacks {\n' + block(locale, 'validation', 'store_callback', '{#using-closures}').replace('fn store_callback(', 'pub(super) fn store_callback(', 1) + '\n}\nmod implicit {\n' + block(locale, 'validation', 'store_implicit', '{#implicit-rules}').replace('fn store_implicit(', 'pub(super) fn store_implicit(', 1) + '\n}\n' + '''pub fn register(router:&mut openrust::routing::Router)->Result<(),openrust::routing::RoutingError> {router.post("/posts",store)?;router.post("/custom-posts",messages::store_with_messages)?;router.post("/strict-posts",unknown_fields::store_strict)?;router.post("/callback-posts",callbacks::store_callback)?;router.post("/implicit-posts",implicit::store_implicit)?;Ok(())}
}
pub fn register(_: &openrust::foundation::Application)->Result<openrust::routing::Router,openrust::routing::RoutingError> {
 let mut router=openrust::routing::Router::new();urls::register(&mut router)?;validation::register(&mut router)?;Ok(router)
}}
'''
    code += '#[tokio::test]\nasync fn tutorial_' + name + '(){verify(' + name + '::register).await;}\n'
target = source / 'vendor/openrust/framework/tests/request_docs_generated.rs'
if not target.resolve().is_relative_to(source):
    parser.error('生成路径不能通过符号链接逃逸源码快照')
target.write_text(code)
subprocess.run(['rustfmt', '+1.94.0', '--edition', '2024', str(target)], check=True)
print('已直接提取三语共十八段教程，生成三个 HTTP Kernel 消费者')
