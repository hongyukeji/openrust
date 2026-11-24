"""直接提取正式三语Artisan签名示例，生成隔离消费者。"""
import argparse,re,subprocess
from pathlib import Path
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);source=p.parse_args().source.resolve()
if not source.is_relative_to(root/'target/tmp'):p.error('源码必须位于项目target/tmp下')
parts=[]
for locale in ['en','zh-Hans','zh-Hant']:
    text=(source/'docs'/locale/'digging-deeper/artisan.md').read_text()
    section=text.split('{#command-signatures}',1)[1]
    code=re.search(r'```rust\n(.*?)```',section,re.S).group(1)
    checks=r'''
use openrust::support::facades::Facade as _;
#[tokio::test]
async fn formal_signature_example() {
    let root=std::env::temp_dir().join(format!("artisan-doc-{}-{}",std::process::id(),module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=openrust::foundation::Application::configure(root.clone()).with_routing(openrust::foundation::configuration::Routing::default().commands(register)).create().unwrap();
    app.boot().await.unwrap();
    openrust::support::facades::App::set_facade_application(Some(&app), async {
        let mut output=Output::default();
        assert_eq!(Artisan::call("mail:send",&["42".into(),"--queue=urgent".into(),"--id=7".into()],&mut output).await.unwrap(),0);
        assert!(output.stdout.contains("42") && output.stdout.contains("urgent"));
        let mut output=Output::default();
        assert_eq!(Artisan::call("help",&["mail:send".into(),"--format=json".into()],&mut output).await.unwrap(),0);
        assert!(output.stdout.contains("Send a marketing email"));
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
'''
    parts.append('mod '+locale.replace('-','_').lower()+'{\n'+code+checks+'}\n')
    injected=re.findall(r'```rust\n(.*?)```',section,re.S)[1]
    injected_checks=checks.replace('fn formal_signature_example()', 'fn formal_injected_example()').replace('.commands(register)', '.commands(register_injected)').replace('    app.boot().await.unwrap();', '    use openrust::contracts::{foundation::Application as _,container::Container as _};\n    app.container().instance(Arc::new(Mailer)).unwrap();\n    app.boot().await.unwrap();').replace('"mail:send"','"mail:send-injected"').replace('&["42".into(),"--queue=urgent".into(),"--id=7".into()]','&["42".into()]').replace('output.stdout.contains("42") && output.stdout.contains("urgent")','output.stdout.contains("Sent to 42")').replace('Send a marketing email','Send with an injected service')
    parts.append('mod '+locale.replace('-','_').lower()+'_injected{\n'+injected+injected_checks+'}\n')
target=source/'tests/artisan_docs_generated.rs';target.write_text('\n'.join(parts));subprocess.run(['rustfmt','+1.94.0','--edition','2024',str(target)],check=True)
print('generated',len(parts),'direct formal consumers')
