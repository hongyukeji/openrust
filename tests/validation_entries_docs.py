from pathlib import Path
import argparse
import re

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--source', type=Path, required=True)
source = parser.parse_args().source.resolve()
if not source.is_relative_to(root / 'target/tmp') or not (source / 'Cargo.toml').is_file():
    parser.error('只能向项目target/tmp内的隔离源码生成消费者')
parts = []
for lang in ['en', 'zh-Hans', 'zh-Hant']:
    text = (source / 'docs' / lang / 'the-basics/validation.md').read_text().split('{#direct-validator-entries}', 1)[1]
    code = re.search(r'```rust\n(.*?)```', text, re.S).group(1)
    conditional = (source / 'docs' / lang / 'the-basics/validation.md').read_text().split('{#conditional-multiple-fields}', 1)[1]
    code += '\nmod conditions {' + re.search(r'```rust\n(.*?)```', conditional, re.S).group(1) + '}\n'
    test = r'''
use openrust::contracts::{http::Kernel as _,session::Session as _};
fn display(request:&mut Request)->HttpFuture<'_,Result<Response,HttpError>>{Box::pin(async move{Response::json(StatusCode::OK,serde_json::json!({"errors":request.session()?.get("errors"),"old":request.old(None)}))})}
fn register()->Result<(),openrust::routing::RoutingError>{use openrust::support::facades::Route;Route::post("/article",validate_article)?;Route::post("/save",save_form)?;Route::post("/form",validate_form)?;Route::post("/collector",conditions::store_collector)?;Route::get("/display",display)?;Ok(())}
async fn call(kernel:&openrust::foundation::http::Kernel,method:&str,path:&str,body:&str,cookie:Option<&str>)->(u16,serde_json::Value,Option<String>){let mut request=http::Request::builder().method(method).uri(path).header("host","tutorial.test").header("accept","application/json").header("sec-fetch-site","same-origin").header("content-type","application/json");if let Some(cookie)=cookie{request=request.header("cookie",cookie);}let response=kernel.handle(&mut Request::from(request.body(openrust::http::Body::from(body.to_owned())).unwrap())).await;let status=response.status().as_u16();let cookie=response.headers().get_all("set-cookie").iter().filter_map(|line|line.to_str().ok()).find(|line|line.starts_with("entries_session=")).map(|line|line.split(';').next().unwrap().to_owned());let bytes=axum::body::to_bytes(response.into_parts().1,65536).await.unwrap();(status,serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),cookie)}
#[tokio::test]async fn direct_entries_from_formal_docs_run(){let root=tempfile::tempdir().unwrap();let app=openrust::foundation::Application::configure(root.path().into()).with_config(|_,_|Ok(std::collections::BTreeMap::from([("app".into(),serde_json::json!({"key":"a".repeat(32)})),("session".into(),serde_json::json!({"driver":"array","cookie":"entries_session","lottery":[0,100]}))]))).with_routing(openrust::foundation::configuration::Routing::default().web(register)).create().unwrap();let mut kernel=openrust::foundation::http::Kernel::new(app);kernel.bootstrap().await.unwrap();assert_eq!(call(&kernel,"POST","/article",r#"{"title":"Hello","ignored":42}"#,None).await.0,201);assert_eq!(call(&kernel,"POST","/article","{}",None).await.0,422);let(status,_,cookie)=call(&kernel,"POST","/save",r#"{"draft":"saved"}"#,None).await;assert_eq!(status,302);let(_,flashed,_)=call(&kernel,"GET","/display","",cookie.as_deref()).await;assert_eq!(flashed["errors"]["article"]["title"][0],"The title field is required.");assert_eq!(flashed["old"]["draft"],"saved");let(_,clean,_)=call(&kernel,"GET","/display","",cookie.as_deref()).await;assert!(clean["errors"].is_null());assert_eq!(call(&kernel,"POST","/form",r#"{"title":"Hello"}"#,None).await.0,201);let(status,errors,_)=call(&kernel,"POST","/form",r#"{"title":"blocked"}"#,None).await;assert_eq!(status,422);assert_eq!(errors["errors"]["title"][0],"Title is blocked.");for data in [serde_json::json!({"games":99}),serde_json::json!({"games":100,"reason":"collection","cost":20}),serde_json::json!({"games":"100","reason":"collection","cost":20})]{assert_eq!(call(&kernel,"POST","/collector",&data.to_string(),None).await.0,201);}let(status,errors,_)=call(&kernel,"POST","/collector",r#"{"games":100}"#,None).await;assert_eq!(status,422);assert!(errors["errors"]["reason"].is_array());assert!(errors["errors"]["cost"].is_array());}
'''
    parts.append('mod ' + lang.replace('-', '_').lower() + '{' + code + test + '}')
(source / 'vendor/openrust/framework/tests/validation_entries_docs_generated.rs').write_text('\n'.join(parts))
