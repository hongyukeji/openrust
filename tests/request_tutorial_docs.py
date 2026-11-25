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
 let root=tempfile::tempdir().unwrap();std::fs::create_dir_all(root.path().join("lang/fr")).unwrap();std::fs::write(root.path().join("lang/fr/rules.json"),r#"{"invalid":"Le champ :attribute est invalide (:token)."}"#).unwrap();let app=Application::configure(root.path().to_owned()).with_config(|_,_|Ok(std::collections::BTreeMap::from([
 ("app".into(),json!({"key":"a".repeat(32),"locale":"en","supported_locales":[],"url":"http://tutorial.test","prefer_app_url":false})),
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
 let(status,data)=call(&kernel,"POST","http://tutorial.test/translated-posts",r#"{"title":"x"}"#).await;assert_eq!(status,StatusCode::UNPROCESSABLE_ENTITY);assert_eq!(data["errors"]["title"][0],"Le champ title est invalide (X).");
 for body in [r#"{"contact":"a@example.com"}"#,r#"{"contact":"abcdef"}"#]{assert_eq!(call(&kernel,"POST","http://tutorial.test/choice-posts",body).await.0,StatusCode::CREATED);}
 for body in ["{}",r#"{"contact":"x"}"#]{let(status,data)=call(&kernel,"POST","http://tutorial.test/choice-posts",body).await;assert_eq!(status,StatusCode::UNPROCESSABLE_ENTITY);assert!(data["errors"]["contact"].is_array());}
 for body in [r#"{"contact":{"email":"a@example.com"}}"#,r#"{"contact":{"name":"abcdef","extra":true}}"#]{let(status,data)=call(&kernel,"POST","http://tutorial.test/contact-posts",body).await;assert_eq!(status,StatusCode::CREATED);assert_eq!(data,serde_json::from_str::<Value>(body).unwrap());}
 assert_eq!(call(&kernel,"POST","http://tutorial.test/contact-posts",r#"{"contact":{"name":"x"}}"#).await.0,StatusCode::UNPROCESSABLE_ENTITY);
 let(status,data)=call(&kernel,"POST","http://tutorial.test/each-posts",r#"{"companies":[{"id":2},{"id":3}]}"#).await;assert_eq!(status,StatusCode::CREATED);assert_eq!(data,json!({"companies":[{"id":2},{"id":3}]}));
 assert_eq!(call(&kernel,"POST","http://tutorial.test/each-posts",r#"{"companies":[{"id":0}]}"#).await.0,StatusCode::UNPROCESSABLE_ENTITY);
 for(body,status)in [(r#"{"items":[{"quantity":1},{"quantity":10,"bulk":true}]}"#,StatusCode::CREATED),(r#"{"items":[{"quantity":1,"bulk":true}]}"#,StatusCode::UNPROCESSABLE_ENTITY)]{assert_eq!(call(&kernel,"POST","http://tutorial.test/bulk-posts",body).await.0,status);}
 let(status,data)=call(&kernel,"POST","http://tutorial.test/nested-posts",r#"{"items":[{"id":2,"tag":"A","extra":true}]}"#).await;assert_eq!(status,StatusCode::CREATED);assert_eq!(data,json!({"items":[{"id":2,"tag":"A"}]}));
 let(status,data)=call(&kernel,"POST","http://tutorial.test/safe-posts",r#"{"name":"A","email":"a@example.com","admin":true}"#).await;assert_eq!(status,StatusCode::CREATED);assert_eq!(data,json!({"selected":{"name":"A","email":"a@example.com"},"without_email":{"name":"A"},"merged":{"name":"A","email":"a@example.com","role":"reader"},"collection":{"name":"A","email":"a@example.com"},"name":"A"}));
 assert_eq!(call(&kernel,"POST","http://tutorial.test/safe-posts",r#"{"name":"A"}"#).await.0,StatusCode::UNPROCESSABLE_ENTITY);
 for(status,expected)in [("active",StatusCode::CREATED),("pending",StatusCode::UNPROCESSABLE_ENTITY),("unknown",StatusCode::UNPROCESSABLE_ENTITY)]{assert_eq!(call(&kernel,"POST","http://tutorial.test/status-posts",&json!({"status":status}).to_string()).await.0,expected);}
 for(body,expected)in [(json!({"zone":"first-zone","topping":"cream"}),StatusCode::CREATED),(json!({"zone":"third-zone"}),StatusCode::UNPROCESSABLE_ENTITY),(json!({"zone":"first-zone","topping":"cherries"}),StatusCode::UNPROCESSABLE_ENTITY)]{assert_eq!(call(&kernel,"POST","http://tutorial.test/zone-posts",&body.to_string()).await.0,expected);}
 let policy=json!({"name":"Sample_1","amount":"25.00","starts_on":"2024-01-01","website":"https://example.com","id":"550e8400-e29b-41d4-a716-446655440000","address":"127.0.0.1","zone":"America/New_York"});assert_eq!(call(&kernel,"POST","http://tutorial.test/policy-posts",&policy.to_string()).await.0,StatusCode::CREATED);
 for(field,value)in [("amount",json!("25.0")),("starts_on",json!("2023-01-01")),("website",json!("http://example.com")),("id",json!("550e8400-e29b-11d4-a716-446655440000")),("address",json!("::1")),("zone",json!("Asia/Tokyo"))]{let mut body=policy.clone();body[field]=value;assert_eq!(call(&kernel,"POST","http://tutorial.test/policy-posts",&body.to_string()).await.0,StatusCode::UNPROCESSABLE_ENTITY,"{field}");}
 for(body,expected)in [(json!({"name":"OK","profile":{"name":"A"},"count":2}),StatusCode::CREATED),(json!({"name":"OK","profile":{"extra":true},"count":2}),StatusCode::UNPROCESSABLE_ENTITY),(json!({"count":"2"}),StatusCode::UNPROCESSABLE_ENTITY)]{assert_eq!(call(&kernel,"POST","http://tutorial.test/typed-posts",&body.to_string()).await.0,expected);}
 for(body,expected)in [(json!({"requires_title":true}),StatusCode::UNPROCESSABLE_ENTITY),(json!({"requires_title":false}),StatusCode::CREATED),(json!({"requires_title":true,"title":"A","metadata":[]}),StatusCode::CREATED)]{assert_eq!(call(&kernel,"POST","http://tutorial.test/condition-posts",&body.to_string()).await.0,expected);}
 for(body,expected)in [(json!({"value":"A","confirmation":"A","status":"pending","review":false}),StatusCode::CREATED),(json!({"value":"A","confirmation":"B","status":"pending"}),StatusCode::UNPROCESSABLE_ENTITY),(json!({"value":"A","confirmation":"A","status":"pending","review":true}),StatusCode::UNPROCESSABLE_ENTITY)]{let(status,data)=call(&kernel,"POST","http://tutorial.test/aware-posts",&body.to_string()).await;assert_eq!(status,expected,"{data}");if body["review"]==true{assert_eq!(data["errors"]["review"][0],"需要人工复核。");}}
 let factories=json!({"username":"alice","confirmation":"alice","account_type":"business","terms":true,"email":"a@example.com","settings":{},"profile":{"name":"A"},"roles":["reader"],"nickname":"discard"});
 let(status,data)=call(&kernel,"POST","http://tutorial.test/factory-posts",&factories.to_string()).await;assert_eq!(status,StatusCode::CREATED,"{data}");assert!(data.get("nickname").is_none());
 for(field,value) in [("confirmation",json!("other")),("terms",json!(false)),("profile",json!({"admin":true})),("roles",json!(["reader","admin"]))] {let mut body=factories.clone();body[field]=value;assert_eq!(call(&kernel,"POST","http://tutorial.test/factory-posts",&body.to_string()).await.0,StatusCode::UNPROCESSABLE_ENTITY,"{field}");}
 let(status,data)=call(&kernel,"POST","http://tutorial.test/photo-posts",r#"{"photos":[{"description":"fine"},{"description":""}]}"#).await;assert_eq!(status,StatusCode::UNPROCESSABLE_ENTITY);let photo_message=data["errors"]["photos.1.description"][0].as_str().unwrap();assert!(photo_message.contains("2nd"));assert!(photo_message.contains("2"));assert!(photo_message.contains("empty"));assert!(!photo_message.contains(":position"));assert!(!photo_message.contains(":input"));
 assert_eq!(call(&kernel,"GET","http://tutorial.test/email-test-dns","").await.0,StatusCode::OK);
 for(value,expected)in [("user@example.com",StatusCode::CREATED),("not-email",StatusCode::UNPROCESSABLE_ENTITY),("pаypal@example.com",StatusCode::UNPROCESSABLE_ENTITY),("user@localhost",StatusCode::UNPROCESSABLE_ENTITY)]{assert_eq!(call(&kernel,"POST","http://tutorial.test/email-posts",&json!({"email":value}).to_string()).await.0,expected);}
 for(value,expected)in [("é@example.com",StatusCode::CREATED),("用户@例子.公司",StatusCode::UNPROCESSABLE_ENTITY)]{assert_eq!(call(&kernel,"POST","http://tutorial.test/native-email-posts",&json!({"email":value}).to_string()).await.0,expected);}
 for(value,expected)in [("user@example.com",StatusCode::CREATED),("blocked@example.com",StatusCode::UNPROCESSABLE_ENTITY)]{assert_eq!(call(&kernel,"POST","http://tutorial.test/custom-email-posts",&json!({"email":value}).to_string()).await.0,expected);}
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
''' + block(locale, 'validation', 'store') + '\nmod messages {\n' + block(locale, 'validation', 'store_with_messages', '{#custom-validation-messages}').replace('fn store_with_messages(', 'pub(super) fn store_with_messages(', 1) + '\n}\nmod unknown_fields {\n' + block(locale, 'validation', 'store_strict', '{#failing-on-unknown-fields}').replace('fn store_strict(', 'pub(super) fn store_strict(', 1) + '\n}\nmod callbacks {\n' + block(locale, 'validation', 'store_callback', '{#using-closures}').replace('fn store_callback(', 'pub(super) fn store_callback(', 1) + '\n}\nmod implicit {\n' + block(locale, 'validation', 'store_implicit', '{#implicit-rules}').replace('fn store_implicit(', 'pub(super) fn store_implicit(', 1) + '\n}\nmod translated {\n' + block(locale, 'validation', 'localized_failure', '{#translating-validation-messages}') + '''
pub(super) fn store(request:&mut openrust::http::Request)->openrust::http::HttpFuture<'_,Result<openrust::http::Response,openrust::http::HttpError>>{Box::pin(async move{
 let rule=openrust::validation::Rule::custom(|context:&openrust::validation::RuleContext<'_>,_:&str,_:Option<&serde_json::Value>|Ok(vec![localized_failure(context)]));
 let rules=openrust::validation::RuleSet::new().field("title",[rule])?;let data=request.validate(rules).await?;openrust::http::Response::json(openrust::http::StatusCode::OK,data.into_value())
})}
}\n''' + '\nmod choices {\n' + block(locale, 'validation', 'store_choice', '{#rule-any-of}').replace('fn store_choice(', 'pub(super) fn store_choice(', 1).replace('fn store_contact(', 'pub(super) fn store_contact(', 1) + '\n}\nmod foreach {\n' + block(locale, 'validation', 'store_each', '{#accessing-nested-array-data}').replace('fn store_each(', 'pub(super) fn store_each(', 1).replace('fn store_bulk(', 'pub(super) fn store_bulk(', 1).replace('fn store_nested(', 'pub(super) fn store_nested(', 1) + '\n}\nmod safe {\n' + block(locale, 'validation', 'store_safe', '{#working-with-validated-input}').replace('fn store_safe(', 'pub(super) fn store_safe(', 1) + '\n}\nmod enums {\n' + block(locale, 'validation', 'store_status', '{#rule-enum}').replace('fn store_status(', 'pub(super) fn store_status(', 1).replace('fn store_zone(', 'pub(super) fn store_zone(', 1) + '\n}\nmod fluent {\n' + block(locale, 'validation', 'store_policy', '{#fluent-rule-builders}').replace('fn store_policy(', 'pub(super) fn store_policy(', 1).replace('fn store_typed(', 'pub(super) fn store_typed(', 1).replace('fn store_condition(', 'pub(super) fn store_condition(', 1) + '\n}\nmod aware {\n' + block(locale, 'validation', 'store_aware', '{#accessing-additional-data}').replace('fn store_aware(', 'pub(super) fn store_aware(', 1) + '\n}\nmod factories {\n' + block(locale, 'validation', 'store_factories', '{#named-rule-factories}').replace('fn store_factories(', 'pub(super) fn store_factories(', 1) + '\n}\nmod positions {\n' + block(locale, 'validation', 'store_photos', '{#error-message-indexes-and-positions}').replace('fn store_photos(', 'pub(super) fn store_photos(', 1) + '\n}\nmod email {\n' + block(locale, 'validation', 'store_email', '{#email-validation-modes}').replace('fn store_email(', 'pub(super) fn store_email(', 1).replace('fn store_native_email(', 'pub(super) fn store_native_email(', 1).replace('fn store_custom_email(', 'pub(super) fn store_custom_email(', 1).replace('fn configure_email_tests(', 'pub(super) fn configure_email_tests(', 1) + '\n}\n' + '''pub fn register(router:&mut openrust::routing::Router)->Result<(),openrust::routing::RoutingError> {router.get("/email-test-dns",||async {email::configure_email_tests()?;openrust::http::Response::json(openrust::http::StatusCode::OK,true)})?;router.post("/email-posts",email::store_email)?;router.post("/native-email-posts",email::store_native_email)?;router.post("/custom-email-posts",email::store_custom_email)?;router.post("/photo-posts",positions::store_photos)?;router.post("/factory-posts",factories::store_factories)?;router.post("/posts",store)?;router.post("/aware-posts",aware::store_aware)?;router.post("/policy-posts",fluent::store_policy)?;router.post("/typed-posts",fluent::store_typed)?;router.post("/condition-posts",fluent::store_condition)?;router.post("/status-posts",enums::store_status)?;router.post("/zone-posts",enums::store_zone)?;router.post("/safe-posts",safe::store_safe)?;router.post("/custom-posts",messages::store_with_messages)?;router.post("/strict-posts",unknown_fields::store_strict)?;router.post("/callback-posts",callbacks::store_callback)?;router.post("/implicit-posts",implicit::store_implicit)?;router.post("/translated-posts",translated::store)?;router.post("/choice-posts",choices::store_choice)?;router.post("/contact-posts",choices::store_contact)?;router.post("/each-posts",foreach::store_each)?;router.post("/bulk-posts",foreach::store_bulk)?;router.post("/nested-posts",foreach::store_nested)?;Ok(())}
}
pub fn register(_: &openrust::foundation::Application)->Result<openrust::routing::Router,openrust::routing::RoutingError> {
 let mut router=openrust::routing::Router::new();urls::register(&mut router)?;validation::register(&mut router)?;Ok(router)
}}
'''
    code += '#[tokio::test]\nasync fn tutorial_' + name + '(){verify(' + name + '::register).await;}\n'
code += r'''
async fn verify_database(register:fn(&Application)->Result<Router,RoutingError>) {
 use openrust::database::DatabaseManager;
 let Ok(url)=std::env::var("OPENRUST_TEST_MYSQL_URL") else {assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());return;};
 let prefix=format!("directory_docs_{:016x}_",fastrand::u64(..));
 let config=std::collections::BTreeMap::from([("database".into(),json!({"default":"directory","connections":{"directory":{"url":url,"prefix":prefix}}}))]);
 let manager=DatabaseManager::new(std::sync::Arc::new(openrust::config::Repository::new(config.clone())),".");let db=manager.connection(None).unwrap();
 let table=format!("{prefix}accounts");db.statement(&format!("CREATE TABLE {table} (account_id INT PRIMARY KEY,email VARCHAR(100),tenant INT,deleted_at INT)"),&[]).await.unwrap();
 db.statement(&format!("INSERT INTO {table} VALUES(1,'owned@example.test',7,NULL),(2,'foreign@example.test',8,NULL),(3,'deleted@example.test',7,1)"),&[]).await.unwrap();
 let root=tempfile::tempdir().unwrap();let base=root.path().join(&prefix);std::fs::create_dir(&base).unwrap();let app=Application::configure(base).with_config(|base,_|Ok(std::collections::BTreeMap::from([("database".into(),json!({"default":"directory","connections":{"directory":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),"prefix":base.file_name().unwrap().to_str().unwrap()}}}))]))).with_routing(Routing::new(register)).create().unwrap();let mut kernel=Kernel::new(app);kernel.bootstrap().await.unwrap();
 for(body,expected)in [(json!({"id":1,"email":"new@example.test"}),StatusCode::CREATED),(json!({"id":2,"email":"new@example.test"}),StatusCode::CREATED),(json!({"id":3,"email":"new@example.test"}),StatusCode::UNPROCESSABLE_ENTITY),(json!({"id":1,"email":"owned@example.test"}),StatusCode::UNPROCESSABLE_ENTITY),(json!({"id":5,"email":"new@example.test"}),StatusCode::UNPROCESSABLE_ENTITY)] {let(status,data)=call(&kernel,"POST","http://tutorial.test/accounts",&body.to_string()).await;assert_eq!(status,expected,"{data}");}
 db.statement(&format!("DROP TABLE {table}"),&[]).await.unwrap();
}
'''
for locale in ('en','zh-Hans','zh-Hant'):
    name='database_'+locale.lower().replace('-','_')
    code+='mod '+name+' {\n'+block(locale,'validation','directory_rules','{#database-validation-rules}')+r'''
fn submit(request:&mut openrust::http::Request)->openrust::http::HttpFuture<'_,Result<openrust::http::Response,openrust::http::HttpError>> {Box::pin(async move {openrust::http::Response::json(openrust::http::StatusCode::CREATED,request.validate(directory_rules()?).await?)})}
pub fn register(_: &openrust::foundation::Application)->Result<openrust::routing::Router,openrust::routing::RoutingError> {let mut router=openrust::routing::Router::new();router.post("/accounts",submit)?;Ok(router)}
}
'''
    code+='#[tokio::test]\nasync fn tutorial_'+name+'(){verify_database('+name+'::register).await;}\n'
target = source / 'vendor/openrust/framework/tests/request_docs_generated.rs'
if not target.resolve().is_relative_to(source):
    parser.error('生成路径不能通过符号链接逃逸源码快照')
target.write_text(code)
subprocess.run(['rustfmt', '+1.94.0', '--edition', '2024', str(target)], check=True)
print('已直接提取三语正式教程，生成 HTTP Kernel 消费者')
