"""直接提取三语日志教程，生成在独立进程及实际请求中执行的正式消费者。"""
import argparse,json,re,subprocess
from pathlib import Path
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);source=p.parse_args().source.resolve()
if not source.is_relative_to(root/'target/tmp'):p.error('源码必须位于项目target/tmp下')
parts=[]
for language in ['en','zh-Hans','zh-Hant']:
    text=(source/'docs'/language/'the-basics/logging.md').read_text()
    snippets=re.findall(r'```rust\n(.*?)```',text,re.S)
    main_number=0
    for number,code in enumerate(snippets):
        name=language.replace('-','_').lower()+'_case_'+str(number)
        if 'fn main()' in code:
            index=main_number;main_number+=1
            checks=[
                'assert!(read("storage/logs/app.log").contains("Order 42 created"));',
                'assert!(read("storage/logs/app.log").contains("Audit ready"));assert!(std::fs::read_dir(".").unwrap().any(|entry|entry.unwrap().file_name().to_string_lossy().starts_with("audit-")));',
                'assert_eq!(read("storage/logs/audit.log"),"Order recorded\\n");',
                'let text=read("storage/logs/audit.log");assert!(text.contains("Import started")&&text.contains("Import failed"));',
                '',
                'let text=read("storage/logs/errors.jsonl");let row:serde_json::Value=serde_json::from_str(text.trim()).unwrap();assert_eq!(row["message"],"Import failed");assert_eq!(row["context"]["exception"]["message"],"storage offline");',
            ][index]
            test='''
fn read(path:&str)->String{std::fs::read_to_string(path).unwrap()}
#[test]fn formal_main_example(){
    if std::env::var("DOC_LOG_EXAMPLE_CHILD").ok().as_deref()==Some(NAME){main().unwrap();CHECKS return;}
    let temporary=tempfile::tempdir().unwrap();let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",concat!(module_path!(),"::formal_main_example"),"--nocapture"]).env("DOC_LOG_EXAMPLE_CHILD",NAME).current_dir(temporary.path()).output().unwrap();
    assert!(output.status.success(),"{}\\n{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
}
'''.replace('CHECKS',checks)
            # 无文件输出的Syslog示例通过自身的真实接收断言；不生成未使用的read助手。
            if not checks:test=test.replace('fn read(path:&str)->String{std::fs::read_to_string(path).unwrap()}','')
            qualifier='#[cfg(unix)]' if 'std::{os::unix' in code else ''
        elif 'fn register_own_formatter' in code:
            test='\n#[test]fn formal_owned_formatter_example(){\n if std::env::var("DOC_LOG_EXAMPLE_CHILD").ok().as_deref()==Some(NAME){let manager=LogManager::new(Arc::new(openrust::config::Repository::new(Default::default())),std::env::current_dir().unwrap());register_own_formatter(&manager);manager.build(&serde_json::json!({"driver":"monolog","handler":"own_text","handler_with":{"prefix":"APP"},"formatter":"default"})).unwrap().info("formal own",&openrust::log::LogContext::new()).unwrap();return;}\n let root=tempfile::tempdir().unwrap();let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",concat!(NAME,"::formal_owned_formatter_example"),"--nocapture"]).env("DOC_LOG_EXAMPLE_CHILD",NAME).current_dir(root.path()).output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));assert_eq!(String::from_utf8(output.stderr).unwrap(),"APP:formal own\\n");assert!(!root.path().join("storage/logs/app.log").exists());\n}\n';qualifier=''
        elif 'fn register_formatter_tap' in code:
            test='\nstruct TutorialProvider;\nimpl openrust::support::ServiceProvider for TutorialProvider {\n fn boot<\'a>(&\'a mut self,app:&\'a Application)->openrust::http::HttpFuture<\'a,ProviderResult>{Box::pin(async move{register_formatter_tap(app)})}\n}\nfn tutorial_config(_: &std::path::Path,_:&openrust::config::Environment)->Result<std::collections::BTreeMap<String,serde_json::Value>,openrust::config::ConfigError>{Ok(std::collections::BTreeMap::from([("app".into(),serde_json::json!({"name":"Before","env":"production","key":"a".repeat(32)})),("session".into(),serde_json::json!({"driver":"array","lottery":[0,100]})),("logging".into(),serde_json::json!({"default":"tap","channels":{"tap":{"driver":"single","path":"tap.log","tap":["customize:orders,prod"]}}}))]))}\nfn tutorial_routes()->Result<(),openrust::routing::RoutingError>{openrust::support::facades::Route::get("/",tutorial_handler)}\nfn tutorial_handler(_: &mut openrust::http::Request)->openrust::http::HttpFuture<\'_,Result<openrust::http::Response,openrust::http::HttpError>>{Box::pin(async{use openrust::support::facades::{Config,Log};let context=openrust::log::LogContext::new();Config::set("app.name","After".into())?;Log::info("first",&context)?;Config::set("app.name","Late".into())?;Log::info("cached",&context)?;Log::forget_channel(None)?;Log::info("rebuilt",&context)?;Ok(openrust::http::Response::from("tap done"))})}\n#[tokio::test]async fn formal_container_tap_example(){use openrust::contracts::http::Kernel as _;let root=tempfile::tempdir().unwrap();let mut kernel=openrust::foundation::http::Kernel::new(Application::configure(root.path().into()).with_config(tutorial_config).with_providers(vec![Box::new(TutorialProvider)]).with_routing(openrust::foundation::configuration::Routing::default().web(tutorial_routes)).create().unwrap());kernel.bootstrap().await.unwrap();assert!(!root.path().join("tap.log").exists());let response=std::sync::Arc::new(kernel).handle_transport(axum::http::Request::builder().uri("/").header("host","localhost").body(openrust::http::Body::empty()).unwrap()).await;assert_eq!(response.status(),200);assert_eq!(std::fs::read_to_string(root.path().join("tap.log")).unwrap(),"After:orders,prod:first\\nAfter:orders,prod:cached\\nLate:orders,prod:rebuilt\\n");}\n';qualifier=''
        elif 'fn register_log_listener' in code:
            test='''
#[tokio::test]async fn formal_listener_example(){use openrust::{foundation::Application,support::facades::{App,Facade}};let temporary=tempfile::tempdir().unwrap();let mut app=Application::configure(temporary.path().into()).create().unwrap();app.boot().await.unwrap();App::set_facade_application(Some(&app),async {register_log_listener().unwrap();Log::info("formal-listener",&openrust::log::LogContext::new()).unwrap();}).await;assert!(std::fs::read_to_string(temporary.path().join("storage/logs/app.log")).unwrap().contains("formal-listener"));}
''';qualifier=''
        elif 'Log::log_with_request(' in code:
            code='use openrust::log::LogContext;use openrust::support::facades::Log;\nfn example(request:&mut openrust::http::Request)->openrust::http::HttpFuture<\'_,Result<openrust::http::Response,openrust::http::HttpError>>{Box::pin(async move {'+code+'Ok(openrust::http::Response::new(openrust::http::StatusCode::OK,openrust::http::Body::from("logged")))})}'
            test='''
fn routes()->Result<(),openrust::routing::RoutingError>{openrust::support::facades::Route::get("/orders",example)}
#[tokio::test]async fn formal_request_log_example(){use axum::handler::Handler as _;use openrust::{contracts::http::Kernel as _,foundation::{Application,configuration::Routing,http::Kernel}};use std::sync::Arc;use tokio::io::{AsyncReadExt as _,AsyncWriteExt as _};let temporary=tempfile::tempdir().unwrap();let mut kernel=Kernel::new(Application::configure(temporary.path().into()).with_routing(Routing::default().api(routes)).create().unwrap());kernel.bootstrap().await.unwrap();async fn transport(axum::extract::State(kernel):axum::extract::State<Arc<Kernel>>,request:axum::extract::Request)->axum::response::Response{kernel.handle_transport(request).await}let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();let server=tokio::spawn(async move{axum::serve(listener,transport.with_state(Arc::new(kernel)).into_make_service()).await.unwrap();});let mut socket=tokio::net::TcpStream::connect(address).await.unwrap();socket.write_all(format!("GET /api/orders HTTP/1.1\\r\\nHost: {address}\\r\\nConnection: close\\r\\n\\r\\n").as_bytes()).await.unwrap();let mut text=String::new();socket.read_to_string(&mut text).await.unwrap();assert!(text.starts_with("HTTP/1.1 200"),"{text}");assert!(text.ends_with("logged"));let log=std::fs::read_to_string(temporary.path().join("storage/logs/app.log")).unwrap();assert_eq!(log.matches("Order placed").count(),1);server.abort();let _=server.await;}
''';qualifier=''
        elif 'fn select(' in code:
            test='''
#[test]fn formal_pail_filter_example(){let mut line=serde_json::json!({"message":"payment failed","datetime":"2026-11-24T01:02:03+00:00","level_name":"ERROR","context":{},"channel":"tutorial","level":400,"extra":{}});assert!(select(&line.to_string()).unwrap());line["message"]="other".into();assert!(!select(&line.to_string()).unwrap());line["message"]="payment failed".into();line["level_name"]="CRITICAL".into();assert!(!select(&line.to_string()).unwrap());assert!(select("broken-json").is_err());}
''';qualifier='#[cfg(feature="pail")]'
        elif 'let default_native' in code:
            code=code.replace('use openrust::{log::LogContext, support::facades::Log};','')
            test='''
#[tokio::test]async fn formal_native_handle_example(){use openrust::{foundation::Application,log::LogContext,support::facades::{App,Facade,Log}};use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};let temporary=tempfile::tempdir().unwrap();let mut app=Application::configure(temporary.path().into()).create().unwrap();app.boot().await.unwrap();let calls=Arc::new(AtomicUsize::new(0));let observed=calls.clone();App::set_facade_application(Some(&app),async move {Log::listen(move |_|{observed.fetch_add(1,Ordering::SeqCst);Ok(())}).unwrap();CODE assert!(Arc::ptr_eq(&native,&default_native));Ok::<(),openrust::log::LogError>(())}).await.unwrap();assert_eq!(calls.load(Ordering::SeqCst),0);assert!(std::fs::read_to_string(temporary.path().join("storage/logs/app.log")).unwrap().contains("Worker started"));}
'''.replace('CODE',code);code='';qualifier=''
        elif 'let settings =' in code:
            test='''
#[tokio::test]async fn formal_remote_handler_settings(){use openrust::{foundation::Application,support::facades::{App,Facade,Log}};let temporary=tempfile::tempdir().unwrap();let receiver=std::net::UdpSocket::bind("127.0.0.1:0").unwrap();receiver.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();let mut app=Application::configure(temporary.path().into()).create().unwrap();app.boot().await.unwrap();App::set_facade_application(Some(&app),async {let mut settings={CODE settings};settings["handler_with"]["port"]=receiver.local_addr().unwrap().port().to_string().into();Log::build(&settings).unwrap().info("formal remote",&openrust::log::LogContext::new()).unwrap();}).await;let mut packet=[0;2048];let count=receiver.recv(&mut packet).unwrap();let text=std::str::from_utf8(&packet[..count]).unwrap();assert!(text.contains(" orders ")&&text.contains("formal remote"),"{text}");}
'''.replace('CODE',code);code='';qualifier=''
        else:raise RuntimeError(f'未分类正式Rust示例: {language} block {number}')
        test=test.replace('NAME',json.dumps(name))
        # test二进制的module_path含crate前缀，libtest名称只含模块路径。
        test=test.replace('concat!(module_path!(),"::formal_main_example")','concat!("'+name+'","::formal_main_example")')
        parts.append(qualifier+'\nmod '+name+'{'+('use crate::doc_temp as tempfile;\n' if 'tempfile::' in test else '')+code+'\n'+test+'}\n')
    assert main_number==6,(language,main_number)
# JSON配置也从正式三语正文提取，避免只验平行手写配置。
for language in ['en','zh-Hans','zh-Hant']:
    text=(source/'docs'/language/'the-basics/logging.md').read_text()
    for anchor in ['native-log-components','browser-log-output']:
        section=text.split('{#'+anchor+'}',1)[1].split('\n## ',1)[0]
        settings=re.search(r'```json\n(.*?)```',section,re.S).group(1)
        name=language.replace('-','_').lower()+'_'+anchor.replace('-','_')
        code='\nuse crate::doc_temp as tempfile;\nuse openrust::{foundation::{Application,http::Kernel,configuration::Routing},http::{HttpFuture,Request,Response,HttpError},contracts::http::Kernel as _,support::facades::{Log,Route}};\nfn routes()->Result<(),openrust::routing::RoutingError>{Route::get("/",handle)}\nfn handle(_: &mut Request)->HttpFuture<\'_,Result<Response,HttpError>>{Box::pin(async{Log::info("formal-config {id}",&openrust::log::LogContext::from_iter([("id".into(),42.into())]))?;Ok(Response::from("<p>formal</p>"))})}\nfn config(root:&std::path::Path,_:&openrust::config::Environment)->Result<std::collections::BTreeMap<String,serde_json::Value>,openrust::config::ConfigError>{let channel:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("channel.json")).unwrap()).unwrap();Ok(std::collections::BTreeMap::from([("app".into(),serde_json::json!({"env":"production","key":"a".repeat(32)})),("session".into(),serde_json::json!({"driver":"array","lottery":[0,100]})),("logging".into(),serde_json::json!({"default":"formal","channels":{"formal":channel}}))]))}\n#[tokio::test]async fn formal_json_configuration(){let root=tempfile::tempdir().unwrap();std::fs::write(root.path().join("channel.json"),SETTINGS).unwrap();let mut kernel=Kernel::new(Application::configure(root.path().into()).with_config(config).with_routing(Routing::default().web(routes)).create().unwrap());kernel.bootstrap().await.unwrap();let response=std::sync::Arc::new(kernel).handle_transport(axum::http::Request::builder().uri("/").header("host","localhost").body(openrust::http::Body::empty()).unwrap()).await;assert_eq!(response.status(),200);let text=String::from_utf8(axum::body::to_bytes(response.into_body(),1024*1024).await.unwrap().to_vec()).unwrap();CHECKS}\n'
        code=code.replace('SETTINGS',json.dumps(settings,ensure_ascii=False))
        checks='assert!(text.contains("<script>")&&text.contains("formal-config"));' if anchor=='browser-log-output' else 'let html=std::fs::read_to_string(root.path().join("storage/logs/formatted.html")).unwrap();assert!(html.contains("formal-config 42")&&html.contains("memory_usage"));assert!(!text.contains("<script>"));'
        parts.append('mod '+name+'{'+code.replace('CHECKS',checks)+'}\n')
temporary=r'''
mod doc_temp {
    use std::{path::{Path,PathBuf},sync::atomic::{AtomicU64,Ordering}};
    static NEXT:AtomicU64=AtomicU64::new(0);
    pub struct Directory(PathBuf);
    impl Directory {pub fn path(&self)->&Path{&self.0}}
    impl Drop for Directory {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
    pub fn tempdir()->std::io::Result<Directory>{let path=std::env::temp_dir().join(format!("doc-log-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::SeqCst)));std::fs::create_dir(&path)?;Ok(Directory(path))}
}
'''
target=source/'tests/logging_docs_generated.rs';target.write_text(temporary+'\n'+'\n'.join(parts));subprocess.run(['rustfmt','+1.95.0','--edition','2024',str(target)],check=True)
print('generated',len(parts),'formal tutorial consumers')
