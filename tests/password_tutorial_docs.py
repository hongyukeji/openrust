from pathlib import Path
import re,argparse
root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser();parser.add_argument('--source',type=Path,required=True);source=parser.parse_args().source.resolve()
if not source.is_relative_to(root/'target/tmp') or not (source/'Cargo.toml').is_file():parser.error('只能向项目target/tmp内的隔离源码生成消费者')
parts=[]
for lang in ['en','zh-Hans','zh-Hant']:
 text=(source/'docs'/lang/'the-basics/validation.md').read_text().split('{#password-policies}',1)[1];code=re.search(r'```rust\n(.*?)```',text,re.S).group(1)
 test=r'''use openrust::contracts::http::Kernel as _;
#[tokio::test] async fn formal_password_tutorial_runs(){
 let root=tempfile::tempdir().unwrap();let app=Application::configure(root.path().into()).with_providers(vec![Box::new(PasswordServiceProvider)]).with_routing(openrust::foundation::configuration::Routing::new(|_|{let mut router=openrust::routing::Router::new();router.post("/password",store_password)?;Ok(router)})).create().unwrap();let mut kernel=openrust::foundation::http::Kernel::new(app);kernel.bootstrap().await.unwrap();
 for (value,status,message) in [("Abcdefgh123!",201,""),("abcdefgh123!",422,"uppercase"),("Abcdefgh1234",422,"symbol")]{let body=serde_json::json!({"password":value}).to_string();let request=http::Request::builder().method("POST").uri("/password").header("accept","application/json").header("content-type","application/json").body(openrust::http::Body::from(body)).unwrap();let response=kernel.handle(&mut Request::from(request)).await;assert_eq!(response.status().as_u16(),status);let bytes=axum::body::to_bytes(response.into_parts().1,65536).await.unwrap();if status==422{assert!(String::from_utf8_lossy(&bytes).contains(message));}}
}
'''
 parts.append('mod '+lang.replace('-','_').lower()+'{'+code+test+'}')
(source/'vendor/openrust-framework/tests/password_docs_generated.rs').write_text('\n'.join(parts))
