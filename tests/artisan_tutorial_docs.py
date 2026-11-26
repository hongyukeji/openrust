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
    typed=re.search(r'```rust\n(.*?)```',text.split('{#typed-command-input}',1)[1],re.S).group(1)
    typed_checks=checks.replace('fn formal_signature_example()', 'fn formal_typed_example()').replace('.commands(register)', '.commands(register_typed)').replace('"mail:send"','"reports:send"').replace('&["42".into(),"--queue=urgent".into(),"--id=7".into()]','&["--from=2026-11-25".into(),"--limit=12".into(),"--active".into()]').replace('output.stdout.contains("42") && output.stdout.contains("urgent")','output.stdout.contains("limit=12, active=true, date=true")').replace('Send a marketing email','Send a typed report')
    parts.append('mod '+locale.replace('-','_').lower()+'_typed{\n'+typed+typed_checks+'}\n')
    isolated=re.search(r'```rust\n(.*?)```',text.split('{#isolatable-commands}',1)[1],re.S).group(1)
    isolated_checks=checks.replace('fn formal_signature_example()', 'fn formal_isolated_example()').replace('.commands(register)', '.commands(register_isolated)').replace('.with_routing(', '.with_config(|_,_|Ok(std::collections::BTreeMap::from([("cache".into(),serde_json::json!({"default":"array","stores":{"array":{"driver":"array"}}}))]))).with_routing(').replace('"mail:send"','"reports:isolated"').replace('&["42".into(),"--queue=urgent".into(),"--id=7".into()]','&["42".into(),"--isolated=12".into()]').replace('output.stdout.contains("42") && output.stdout.contains("urgent")','output.stdout.contains("Report for") && output.stdout.contains("42")').replace('assert!(output.stdout.contains("Send a marketing email"));','assert!(output.stdout.contains("isolated"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_isolated{\n'+isolated+isolated_checks+'}\n')
    edited=re.search(r'```rust\n(.*?)```',text.split('{#editing-command-input}',1)[1],re.S).group(1)
    edited_checks=checks.replace('fn formal_signature_example()', 'fn formal_edited_example()').replace('.commands(register)', '.commands(register_edited)').replace('"mail:send"','"reports:edit"').replace('&["42".into(),"--queue=urgent".into(),"--id=7".into()]','&["Ada".into(),"--ansi".into(),"--no-interaction".into()]').replace('output.stdout.contains("42") && output.stdout.contains("urgent")','output.stdout.contains("user=42, queue=priority")').replace('Send a marketing email','Edit parsed command input')
    parts.append('mod '+locale.replace('-','_').lower()+'_edited{\n'+edited+edited_checks+'}\n')
    prompted=re.search(r'```rust\n(.*?)```',text.split('{#prompting-for-missing-input}',1)[1],re.S).group(1)
    prompt_checks=checks.replace('fn formal_signature_example()', 'fn formal_prompted_example()').replace('.commands(register)', '.commands(register_prompted)').replace('"mail:send"','"reports:prompt"').replace('&["42".into(),"--queue=urgent".into(),"--id=7".into()]','&["42".into(),"--queue".into()]').replace('output.stdout.contains("42") && output.stdout.contains("urgent")','output.stdout.contains("user=42, queue=true")').replace('assert!(output.stdout.contains("Send a marketing email"));','assert!(output.stdout.contains("user"));')
    prompt_checks=prompt_checks.replace('    app.boot().await.unwrap();','    app.boot().await.unwrap();\n    if std::env::var_os("FORMAL_PROMPT_TTY").is_some() { assert_eq!(app.handle_command(&["reports:prompt".into()]).await.unwrap(),0); return; }')
    parts.append('mod '+locale.replace('-','_').lower()+'_prompted{\n'+prompted+prompt_checks+'}\n')
    registry=re.search(r'```rust\n(.*?)```',text.split('{#command-lifecycle}',1)[1],re.S).group(1)
    registry_checks=r'''
use openrust::support::facades::Facade as _;
#[tokio::test]
async fn formal_registry_example() {
    let root=std::env::temp_dir().join(format!("artisan-doc-{}-{}",std::process::id(),module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=openrust::foundation::Application::configure(root.clone()).with_routing(openrust::foundation::configuration::Routing::default().commands(register_counter)).create().unwrap();
    app.boot().await.unwrap();
    openrust::support::facades::App::set_facade_application(Some(&app), async {
        for expected in ["1","2"] { let mut output=Output::default(); assert_eq!(Artisan::call("reports:count",&[],&mut output).await.unwrap(),0); assert_eq!(output.stdout,expected); }
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
'''
    parts.append('mod '+locale.replace('-','_').lower()+'_registry{\n'+registry+registry_checks+'}\n')
    output_code=re.search(r'```rust\n(.*?)```',text.split('{#writing-command-output}',1)[1],re.S).group(1)
    output_checks='use openrust::console::Output;\n'+registry_checks.replace('fn formal_registry_example()', 'fn formal_output_example()').replace('.commands(register_counter)', '.commands(register_output)').replace('"reports:count"','"reports:output"').replace('assert_eq!(output.stdout,expected);','assert!(output.stdout.contains("Report ready") && output.stdout.contains("Summary") && output.stdout.contains(&format!("Calls: {}",expected))); assert!(!output.stdout.contains("<options="));')
    parts.append('mod '+locale.replace('-','_').lower()+'_output{\n'+output_code+output_checks+'}\n')
    styled_code=re.search(r'```rust\n(.*?)```',text.split('{#output-style-verbosity}',1)[1],re.S).group(1)
    styled_checks=r"""
use openrust::support::facades::Facade as _;
#[tokio::test]
async fn formal_style_example() {
    let root=std::env::temp_dir().join(format!("artisan-style-{}-{}",std::process::id(),module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=openrust::foundation::Application::configure(root.clone()).with_routing(openrust::foundation::configuration::Routing::default().commands(register_styled)).create().unwrap();
    app.boot().await.unwrap();
    openrust::support::facades::App::set_facade_application(Some(&app),async {
        for flag in ["--no-ansi","-v","--quiet","--silent"] {
            let mut output=Output::default();
            assert_eq!(Artisan::call("reports:styled",&[flag.into()],&mut output).await.unwrap(),0);
            if flag=="--silent" { assert!(output.stdout.is_empty()); }
            else { assert!(output.stdout.contains("Essential")); assert_eq!(output.stdout.contains("Details"),flag=="-v"); assert_eq!(output.stdout.contains("Summary"),flag!="--quiet"); assert_eq!(output.stdout.contains("<raw>text"),flag!="--quiet"); }
        }
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
"""
    parts.append('mod '+locale.replace('-','_').lower()+'_styled{\n'+styled_code+styled_checks+'}\n')
    question_code=re.search(r'```rust\n(.*?)```',text.split('{#interactive-questions}',1)[1],re.S).group(1)
    question_checks=r"""
use openrust::{console::Output, support::facades::{App, Facade as _}};
#[tokio::test]
async fn formal_questions_example() {
    let root=std::env::temp_dir().join(format!("artisan-questions-{}-{}",std::process::id(),module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=openrust::foundation::Application::configure(root.clone()).with_routing(openrust::foundation::configuration::Routing::default().commands(register_questions)).create().unwrap();
    app.boot().await.unwrap();
    if std::env::var_os("FORMAL_QUESTIONS_TTY").is_some() { assert_eq!(app.handle_command(&["reports:questions".into()]).await.unwrap(),0); return; }
    App::set_facade_application(Some(&app),async {
        let mut output=Output::default();
        assert_eq!(Artisan::call("reports:questions", &[], &mut output).await.unwrap(),0);
        assert!(output.stdout.contains("Guest") && output.stdout.contains("blue") && output.stdout.contains("false"));
        for flag in ["--no-interaction", "--quiet", "--silent"] {
            let mut output=Output::default();
            assert_eq!(Artisan::call("reports:questions", &[flag.into()], &mut output).await.unwrap(),0);
            assert!(!output.stdout.contains(" > "));
        }
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
"""
    parts.append('mod '+locale.replace('-','_').lower()+'_questions{\n'+question_code+question_checks+'}\n')
    choices_code=re.search(r'```rust\n(.*?)```',text.split('{#choice-questions}',1)[1],re.S).group(1)
    choices_checks=r"""
use openrust::{console::Output, support::facades::{App, Facade as _}};
#[tokio::test]
async fn formal_choices_example() {
    let root=std::env::temp_dir().join(format!("artisan-choices-{}-{}",std::process::id(),module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=openrust::foundation::Application::configure(root.clone()).with_routing(openrust::foundation::configuration::Routing::default().commands(register_choices)).create().unwrap();
    app.boot().await.unwrap();
    if std::env::var_os("FORMAL_CHOICES_TTY").is_some() { assert_eq!(app.handle_command(&["reports:choices".into()]).await.unwrap(),0); return; }
    App::set_facade_application(Some(&app),async {
        let mut output=Output::default();
        assert_eq!(Artisan::call("reports:choices", &["--no-interaction".into()], &mut output).await.unwrap(),0);
        assert_eq!(serde_json::from_str::<serde_json::Value>(output.stdout.trim()).unwrap(),serde_json::json!({"channel":"email","formats":["csv","json"],"priority":"normal"}));
        for flag in ["--quiet", "--silent"] {
            let mut output=Output::default();
            assert_eq!(Artisan::call("reports:choices", &[flag.into()], &mut output).await.unwrap(),0);
            assert!(output.stdout.is_empty());
        }
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
"""
    parts.append('mod '+locale.replace('-','_').lower()+'_choices{\n'+choices_code+choices_checks+'}\n')
    table_code=re.search(r'```rust\n(.*?)```',text.split('{#tables}',1)[1],re.S).group(1)
    table_checks=r"""
use openrust::support::facades::{App,Facade as _};
#[tokio::test]
async fn formal_tables_example(){
    let root=std::env::temp_dir().join(format!("artisan-table-doc-{}-{}",std::process::id(),module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=openrust::foundation::Application::configure(root.clone()).with_routing(openrust::foundation::configuration::Routing::default().commands(register_tables)).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app),async {
        for flag in ["--no-ansi","--ansi","--quiet","--silent"] {
            let mut output=Output::default();
            assert_eq!(Artisan::call("reports:table",&[flag.into()],&mut output).await.unwrap(),0);
            if flag=="--quiet" || flag=="--silent" {assert!(output.stdout.is_empty());}
            else {assert!(output.stdout.contains("ada@example.test") && output.stdout.contains("li@example.test"));assert_eq!(output.stdout.contains("\x1b[32m"),flag=="--ansi");}
        }
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
"""
    parts.append('mod '+locale.replace('-','_').lower()+'_tables{\n'+table_code+table_checks+'}\n')
    progress_code=re.search(r'```rust\n(.*?)```',text.split('{#progress-bars}',1)[1],re.S).group(1)
    progress_checks=r"""
use openrust::support::facades::{App,Facade as _};
#[tokio::test]
async fn formal_progress_example(){
    let root=std::env::temp_dir().join(format!("artisan-progress-doc-{}-{}",std::process::id(),module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=openrust::foundation::Application::configure(root.clone()).with_routing(openrust::foundation::configuration::Routing::default().commands(register_progress)).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app),async {
        for flag in ["--no-ansi","--ansi","--quiet","--silent"] {
            let mut output=Output::default();
            assert_eq!(Artisan::call("reports:progress",&[flag.into()],&mut output).await.unwrap(),0);
            if flag=="--quiet" || flag=="--silent" {assert!(output.stdout.is_empty());}
            else {assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));}
        }
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
"""
    parts.append('mod '+locale.replace('-','_').lower()+'_progress{\n'+progress_code+progress_checks+'}\n')
    iteration_code=re.search(r'```rust\n(.*?)```',text.split('{#lazy-progress-iteration}',1)[1],re.S).group(1)
    iteration_checks=progress_checks.replace('formal_progress_example','formal_progress_iteration_example').replace('register_progress)', 'register_progress_iteration)').replace('"reports:progress"','"reports:iterate"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("sum:12"));assert_eq!(output.stdout.matches("100%").count(),1);')
    parts.append('mod '+locale.replace('-','_').lower()+'_progress_iteration{\n'+iteration_code+iteration_checks+'}\n')
    async_progress_code=re.search(r'```rust\n(.*?)```',text.split('{#async-progress-callbacks}',1)[1],re.S).group(1)
    async_progress_checks=progress_checks.replace('formal_progress_example','formal_async_progress_example').replace('register_progress)', 'register_progress_async)').replace('"reports:progress"','"reports:async"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("values:12"));assert_eq!(output.stdout.matches("100%").count(),2);')
    async_progress_checks='use openrust::console::Output;\n'+async_progress_checks
    parts.append('mod '+locale.replace('-','_').lower()+'_progress_async{\n'+async_progress_code+async_progress_checks+'}\n')
    progress_output_code=re.search(r'```rust\n(.*?)```',text.split('{#progress-placeholder-output}',1)[1],re.S).group(1)
    progress_output_checks=progress_checks.replace('formal_progress_example','formal_progress_output_example').replace('register_progress)', 'register_progress_output)').replace('"reports:progress"','"reports:progress-output"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("step:0"));assert!(output.stdout.contains("stage:2"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_progress_output{\n'+progress_output_code+progress_output_checks+'}\n')
    configuration_code=re.search(r'```rust\n(.*?)```',text.split('{#output-configuration}',1)[1],re.S).group(1)
    configuration_checks=progress_checks.replace('formal_progress_example','formal_output_configuration_example').replace('register_progress)', 'register_output_configuration)').replace('"reports:progress"','"reports:output-config"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert_eq!(output.stdout,"details;quiet;plain");')
    parts.append('mod '+locale.replace('-','_').lower()+'_output_configuration{\n'+configuration_code+configuration_checks+'}\n')
    formatter_style_code=re.search(r'```rust\n(.*?)```',text.split('{#formatter-styles}',1)[1],re.S).group(1)
    formatter_style_checks=progress_checks.replace('formal_progress_example','formal_formatter_style_example').replace('register_progress)', 'register_formatter_styles)').replace('"reports:progress"','"reports:styles"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("\\x1b[34;1mready\\x1b[39;22m"));assert!(output.stdout.contains("stack:plain"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_formatter_styles{\n'+formatter_style_code+formatter_style_checks+'}\n')
    formatter_code=re.search(r'```rust\n(.*?)```',text.split('{#output-formatters}',1)[1],re.S).group(1)
    formatter_checks=progress_checks.replace('formal_progress_example','formal_formatter_example').replace('register_progress)', 'register_formatter)').replace('"reports:progress"','"reports:format"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert_eq!(output.stdout,"ready\\none two\\nthree\\n\\x1b[32;1mdone\\x1b[39;22m\\n");')
    parts.append('mod '+locale.replace('-','_').lower()+'_formatters{\n'+formatter_code+formatter_checks+'}\n')
    color_code=re.search(r'```rust\n(.*?)```',text.split('{#colors-terminal}',1)[1],re.S).group(1)
    color_checks=progress_checks.replace('formal_progress_example','formal_color_example').replace('register_progress)', 'register_colors)').replace('"reports:progress"','"reports:colors"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let expected=if flag=="--ansi" {"\\x1b[32;1mready\\x1b[39;22m\\nterminal:"}else{"ready\\nterminal:"};assert!(output.stdout.starts_with(expected),"{flag}: {:?}",output.stdout);')
    parts.append('mod '+locale.replace('-','_').lower()+'_colors{\n'+color_code+color_checks+'}\n')
    layout_code=re.search(r'```rust\n(.*?)```',text.split('{#table-layout-controls}',1)[1],re.S).group(1)
    layout_checks=progress_checks.replace('formal_progress_example','formal_layout_example').replace('register_progress)', 'register_layout)').replace('"reports:progress"','"reports:layout"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/table-layout-oracle.json")).unwrap();let expected=cases.iter().find(|case|case["headers"]==serde_json::json!(["Name","Count"])&&case["decorated"]==serde_json::json!(flag=="--ansi")).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_layout{\n'+layout_code+layout_checks+'}\n')
    rows_code=re.search(r'```rust\n(.*?)```',text.split('{#table-rows-orientation}',1)[1],re.S).group(1)
    rows_checks=progress_checks.replace('formal_progress_example','formal_rows_example').replace('register_progress)', 'register_rows)').replace('"reports:progress"','"reports:rows"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/table-rows-oracle.json")).unwrap();let expected=cases.iter().find(|case|case["headers"]==serde_json::json!(["Name","Value"])&&case["decorated"]==serde_json::json!(flag=="--ansi")).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_rows{\n'+rows_code+rows_checks+'}\n')
    definitions_code=re.search(r'```rust\n(.*?)```',text.split('{#table-style-definitions}',1)[1],re.S).group(1)
    definitions_checks=progress_checks.replace('formal_progress_example','formal_table_definitions_example').replace('register_progress)', 'register_table_styles)').replace('"reports:progress"','"reports:table-styles"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/table-definitions-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["decorated"]==serde_json::json!(flag=="--ansi") && c["ops"][0][0]=="external" && c["ops"].as_array().unwrap().len()==9).unwrap();let text:String=expected["trace"].as_array().unwrap().iter().map(|s|s.as_str().unwrap()).collect();assert_eq!(output.stdout,text);')
    parts.append('mod '+locale.replace('-','_').lower()+'_table_definitions{\n'+definitions_code+definitions_checks+'}\n')
    separators_code=re.search(r'```rust\n(.*?)```',text.split('{#table-row-separators}',1)[1],re.S).group(1)
    separators_checks=progress_checks.replace('formal_progress_example','formal_separators_example').replace('register_progress)', 'register_separators)').replace('"reports:progress"','"reports:separators"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/table-separators-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["style"]=="box-double" && c["decorated"]==serde_json::json!(flag=="--ansi") && c["mode"]=="default" && c["headers"]==serde_json::json!(["A","B"]) && c["rows"]==serde_json::json!([["one","two"],"@sep",["three"]]) && c.get("header").is_none()).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_separators{\n'+separators_code+separators_checks+'}\n')
    cell_styles_code=re.search(r'```rust\n(.*?)```',text.split('{#table-cell-styles}',1)[1],re.S).group(1)
    cell_styles_checks=progress_checks.replace('formal_progress_example','formal_cell_styles_example').replace('register_progress)', 'register_cell_styles)').replace('"reports:progress"','"reports:cell-styles"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/table-cell-styles-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["decorated"]==serde_json::json!(flag=="--ansi") && c["headers"]==serde_json::json!(["Name","Score"])).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_cell_styles{\n'+cell_styles_code+cell_styles_checks+'}\n')
    spans_code=re.search(r'```rust\n(.*?)```',text.split('{#table-cell-spans}',1)[1],re.S).group(1)
    spans_checks=progress_checks.replace('formal_progress_example','formal_spans_example').replace('register_progress)', 'register_spans)').replace('"reports:progress"','"reports:spans"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/table-spans-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["decorated"]==serde_json::json!(flag=="--ansi") && c["headers"][0][0]["cell"]=="Summary").unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_spans{\n'+spans_code+spans_checks+'}\n')
    sections_code=re.search(r'```rust\n(.*?)```',text.split('{#console-sections}',1)[1],re.S).group(1)
    sections_checks=progress_checks.replace('formal_progress_example','formal_sections_example').replace('register_progress)', 'register_sections)').replace('"reports:progress"','"reports:sections"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/table-append-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["decorated"]==serde_json::json!(flag=="--ansi") && c["style"]=="default" && c["mode"]=="default" && c["before"]==false && c["rows"][0][0]=="x").unwrap();assert_eq!(output.stdout,expected["steps"].as_array().unwrap().last().unwrap()["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_sections{\n'+sections_code+sections_checks+'}\n')
    section_progress_code=re.search(r'```rust\n(.*?)```',text.split('{#section-consumers}',1)[1],re.S).group(1)
    section_progress_checks=progress_checks.replace('formal_progress_example','formal_section_progress_example').replace('register_progress)', 'register_section_progress)').replace('"reports:progress"','"reports:section-progress"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/section-progress-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["decorated"]==serde_json::json!(flag=="--ansi") && c["width"]==80 && c["level"]==32 && c["ctor"]=="manual" && c["overwrite"].is_null() && c["format"]=="<info>%current%/%max%</info>").unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_section_progress{\n'+section_progress_code+section_progress_checks+'}\n')
    editor_code=re.search(r'```rust\n(.*?)```',text.split('{#section-editors}',1)[1],re.S).group(1)
    editor_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_editor_example').replace('register_progress)', 'register_editor)').replace('"reports:progress"','"reports:editor"').replace('&[flag.into()]','&[flag.into(),"--no-interaction".into()]').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert_eq!(output.stdout,"answer=Guest\\n");')
    parts.append('mod '+locale.replace('-','_').lower()+'_editor{\n'+editor_code+editor_checks+'}\n')
    nested_code=re.search(r'```rust\n(.*?)```',text.split('{#nested-output-styles}',1)[1],re.S).group(1)
    nested_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_nested_example').replace('register_progress)', 'register_nested)').replace('"reports:progress"','"reports:nested"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert_eq!(output.stdout,"\\nA\\n\\nB\\ncount=1\\n");')
    parts.append('mod '+locale.replace('-','_').lower()+'_nested{\n'+nested_code+nested_checks+'}\n')
    text_layout_code=re.search(r'```rust\n(.*?)```',text.split('{#style-text-layout}',1)[1],re.S).group(1)
    text_layout_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_text_layout_example').replace('register_progress)', 'register_text_layout)').replace('"reports:progress"','"reports:text-layout"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/style-text-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["depth"]==1&&c["section"]==false&&c["level"]==32&&c["decorated"]==serde_json::json!(flag=="--ansi")&&c["operations"][0][1]=="Hello").unwrap();assert_eq!(output.stdout,expected["steps"].as_array().unwrap().last().unwrap()["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_text_layout{\n'+text_layout_code+text_layout_checks+'}\n')
    block_code=re.search(r'```rust\n(.*?)```',text.split('{#styled-message-blocks}',1)[1],re.S).group(1)
    block_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_blocks_example').replace('register_progress)', 'register_blocks)').replace('"reports:progress"','"reports:blocks"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/style-block-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["depth"]==1&&c["section"]==false&&c["level"]==32&&c["width"]==80&&c["decorated"]==serde_json::json!(flag=="--ansi")&&c["operations"][0][1]=="Preparing report").unwrap();assert_eq!(output.stdout,expected["steps"].as_array().unwrap().last().unwrap()["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_blocks{\n'+block_code+block_checks+'}\n')
    helper_code=re.search(r'```rust\n(.*?)```',text.split('{#style-question-helpers}',1)[1],re.S).group(1)
    helper_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_style_helpers_example').replace('register_progress)', 'register_style_helpers)').replace('"reports:progress"','"reports:style-questions"').replace('&[flag.into()]','&[flag.into(),"--no-interaction".into()]').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let value:serde_json::Value=serde_json::from_str(output.stdout.trim()).unwrap();assert_eq!(value,serde_json::json!({"profile":{"name":"Ada"},"confirmed":true,"color":"green"}));')
    parts.append('mod '+locale.replace('-','_').lower()+'_style_helpers{\n'+helper_code+helper_checks+'}\n')
    factory_code=re.search(r'```rust\n(.*?)```',text.split('{#table-output-factory}',1)[1],re.S).group(1)
    factory_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_table_factory_example').replace('register_progress)', 'register_factory_table)').replace('"reports:progress"','"reports:factory-table"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/table-output-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["mode"]=="factory"&&c["depth"]==1&&c["section"]==false&&c["level"]==32&&c["decorated"]==serde_json::json!(flag=="--ansi")&&c["before"]==false&&c["custom_guide"]==false).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_table_factory{\n'+factory_code+factory_checks+'}\n')
    tables_code=re.search(r'```rust\n(.*?)```',text.split('{#style-table-helpers}',1)[1],re.S).group(1)
    tables_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_style_tables_example').replace('register_progress)', 'register_style_tables)').replace('"reports:progress"','"reports:style-tables"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/style-table-oracle.json")).unwrap();let mut expected=String::new();for(mode,variant)in[("table",0),("horizontal",7),("definition",0)]{let case=cases.iter().find(|c|c["mode"]==mode&&c["variant"]==variant&&c["depth"]==1&&c["section"]==false&&c["level"]==32&&c["decorated"]==serde_json::json!(flag=="--ansi")).unwrap();expected.push_str(case["output"].as_str().unwrap());}assert_eq!(output.stdout,expected);')
    parts.append('mod '+locale.replace('-','_').lower()+'_style_tables{\n'+tables_code+tables_checks+'}\n')
    interface_code=re.search(r'```rust\n(.*?)```',text.split('{#style-interface}',1)[1],re.S).group(1)
    interface_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_style_interface_example').replace('register_progress)', 'register_style_interface)').replace('"reports:progress"','"reports:interface"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/style-interface-docs-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["level"]==32&&c["decorated"]==serde_json::json!(flag=="--ansi")).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_style_interface{\n'+interface_code+interface_checks+'}\n')
    error_code=re.search(r'```rust\n(.*?)```',text.split('{#error-style}',1)[1],re.S).group(1)
    error_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_error_style_example').replace('register_progress)', 'register_error_style)').replace('"reports:progress"','"reports:error-style"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/error-style-docs-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["level"]==32&&c["decorated"]==serde_json::json!(flag=="--ansi")).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_error_style{\n'+error_code+error_checks+'}\n')
    nodes_code=re.search(r'```rust\n(.*?)```',text.split('{#tree-nodes}',1)[1],re.S).group(1)
    nodes_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_tree_nodes_example').replace('register_progress)', 'register_tree_nodes)').replace('"reports:progress"','"reports:tree-nodes"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert_eq!(output.stdout,"Project:3\\n");')
    parts.append('mod '+locale.replace('-','_').lower()+'_tree_nodes{\n'+nodes_code+nodes_checks+'}\n')
    trees_code=re.search(r'```rust\n(.*?)```',text.split('{#trees}',1)[1],re.S).group(1)
    trees_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_trees_example').replace('register_progress)', 'register_tree_output)').replace('"reports:progress"','"reports:trees"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/tree-docs-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["level"]==32&&c["decorated"]==serde_json::json!(flag=="--ansi")).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_trees{\n'+trees_code+trees_checks+'}\n')
    mutators_code=re.search(r'```rust\n(.*?)```',text.split('{#component-mutators}',1)[1],re.S).group(1)
    mutators_checks='\n#[test]\nfn formal_component_mutators_example(){assert_eq!(transform_component_text("Created [report].").unwrap(),"Created <options=bold>[report]</>");assert_eq!(transform_component_text("").unwrap(),"");}\n'
    parts.append('mod '+locale.replace('-','_').lower()+'_mutators{\n'+mutators_code+mutators_checks+'}\n')
    lines_code=re.search(r'```rust\n(.*?)```',text.split('{#line-components}',1)[1],re.S).group(1)
    lines_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_line_components_example').replace('register_progress)', 'register_line_components)').replace('"reports:progress"','"reports:components"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/line-docs-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["level"]==32&&c["decorated"]==serde_json::json!(flag=="--ansi")).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_line_components{\n'+lines_code+lines_checks+'}\n')
    display_code=re.search(r'```rust\n(.*?)```',text.split('{#display-components}',1)[1],re.S).group(1)
    display_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_display_components_example').replace('register_progress)', 'register_display_components)').replace('"reports:progress"','"reports:display"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','let cases:Vec<serde_json::Value>=serde_json::from_str(include_str!("../.agents/project/audits/artisan-tutorial/display-docs-oracle.json")).unwrap();let expected=cases.iter().find(|c|c["level"]==32&&c["decorated"]==serde_json::json!(flag=="--ansi")).unwrap();assert_eq!(output.stdout,expected["output"].as_str().unwrap());')
    parts.append('mod '+locale.replace('-','_').lower()+'_display_components{\n'+display_code+display_checks+'}\n')
    results_code=re.search(r'```rust\n(.*?)```',text.split('{#task-results}',1)[1],re.S).group(1)
    results_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_task_results_example').replace('register_progress)', 'register_task_results)').replace('"reports:progress"','"tasks:states"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert_eq!(output.stdout,"Success:1\\nFailure:2\\nSkipped:3\\n");')
    parts.append('mod '+locale.replace('-','_').lower()+'_task_results{\n'+results_code+results_checks+'}\n')
    tasks_code=re.search(r'```rust\n(.*?)```',text.split('{#task-components}',1)[1],re.S).group(1)
    tasks_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_task_components_example').replace('register_progress)', 'register_task_components)').replace('"reports:progress"','"reports:tasks"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert_eq!(output.stdout.matches("DONE").count(),3);assert_eq!(output.stdout.matches("SKIPPED").count(),1);assert_eq!(output.stdout.matches("FAIL").count(),1);assert!(output.stdout.contains("CSV "));')
    parts.append('mod '+locale.replace('-','_').lower()+'_task_components{\n'+tasks_code+tasks_checks+'}\n')
    helpers_code=re.search(r'```rust\n(.*?)```',text.split('{#question-helpers}',1)[1],re.S).group(1)
    helpers_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_question_helpers_example').replace('register_progress)', 'register_question_helpers)').replace('"reports:progress"','"reports:question-helpers"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("Users:Guest/Guest"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_question_helpers{\n'+helpers_code+helpers_checks+'}\n')
    qcomponents_code=re.search(r'```rust\n(.*?)```',text.split('{#question-components}',1)[1],re.S).group(1)
    qcomponents_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_question_components_example').replace('register_progress)', 'register_question_components)').replace('"reports:progress"','"reports:question-components"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("User:\\"Guest\\"; Color:\\"red\\"; Confirmed:false; Format:\\"CSV\\"; Secret supplied:false"),"{}",output.stdout);')
    parts.append('mod '+locale.replace('-','_').lower()+'_question_components{\n'+qcomponents_code+qcomponents_checks+'}\n')

    factory_code=re.search(r'```rust\n(.*?)```',text.split('{#component-factories}',1)[1],re.S).group(1)
    factory_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_component_factory_example').replace('register_progress)', 'register_component_factories)').replace('"reports:progress"','"components:save"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("Saved"));assert!(output.stdout.contains("Guest"));assert!(output.stdout.contains("DONE"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_factories{\n'+factory_code+factory_checks+'}\n')


    dynamic_code=re.search(r'```rust\n(.*?)```',text.split('{#dynamic-component-calls}',1)[1],re.S).group(1)
    dynamic_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_dynamic_components_example').replace('register_progress)', 'register_dynamic_components)').replace('"reports:progress"','"components:dynamic"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("Saved"));assert!(output.stdout.contains("red"));assert!(output.stdout.contains("DONE"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_dynamic_components{\n'+dynamic_code+dynamic_checks+'}\n')


    registry_code=re.search(r'```rust\n(.*?)```',text.split('{#custom-console-components}',1)[1],re.S).group(1)
    registry_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_custom_component_example').replace('register_progress)', 'register_custom_components)').replace('.with_routing(','.with_providers(vec![Box::new(BadgeProvider)]).with_routing(').replace('"reports:progress"','"components:badge"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("Badge:Ready"));assert!(output.stdout.contains("label"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_custom_components{\n'+registry_code+registry_checks+'}\n')


    core_code=re.search(r'```rust\n(.*?)```',text.split('{#component-data-scopes}',1)[1],re.S).group(1)
    core_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_component_data_example').replace('register_progress)', 'register_component_data)').replace('"reports:progress"','"components:data"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains(r#""active":true"#));assert!(output.stdout.contains("Guest"));assert!(output.stdout.contains("Calls:1"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_data{\n'+core_code+core_checks+'}\n')


    compile_code=re.search(r'```rust\n(.*?)```',text.split('{#component-markup-compilation}',1)[1],re.S).group(1)
    compile_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_component_compilation_example').replace('register_progress)', 'register_component_compilation)').replace('"reports:progress"','"components:compile"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("Compiled &lt;report&gt;"));assert!(output.stdout.contains("Custom:<strong>Ready</strong>"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_compilation{\n'+compile_code+compile_checks+'}\n')
    render_code=re.search(r'```rust\n(.*?)```',text.split('{#component-markup-rendering}',1)[1],re.S).group(1)
    render_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_component_rendering_example').replace('register_progress)', 'register_component_rendering)').replace('"reports:progress"','"components:render"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("INFO"));assert!(output.stdout.contains("Rendered <report>"));assert!(output.stdout.contains("⇂ First"));assert!(!output.stdout.contains("<div"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_rendering{\n'+render_code+render_checks+'}\n')
    palette_code=re.search(r'```rust\n(.*?)```',text.split('{#component-color-breakpoints}',1)[1],re.S).group(1)
    palette_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_component_colors_example').replace('register_progress)', 'register_component_colors)').replace('"reports:progress"','"components:colors"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("Palette"));assert!(output.stdout.contains("<bg=#0f172a;fg=#fde68a>"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_colors{\n'+palette_code+palette_checks+'}\n')
    sizing_code=re.search(r'```rust\n(.*?)```',text.split('{#component-sizes-and-alignment}',1)[1],re.S).group(1)
    sizing_checks='use openrust::console::Output;\n'+progress_checks.replace('formal_progress_example','formal_component_sizes_example').replace('register_progress)', 'register_component_sizes)').replace('"reports:progress"','"components:sizes"').replace('assert!(output.stdout.contains("CSV,JSON,HTML:11"));assert_eq!(output.stdout.matches("100%").count(),2);assert!(output.stdout.contains("build"));','assert!(output.stdout.contains("A                  B"));assert!(output.stdout.contains("Half"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_sizes{\n'+sizing_code+sizing_checks+'}\n')
    methods_code=re.search(r'```rust\n(.*?)```',text.split('{#component-text-transforms}',1)[1],re.S).group(1)
    methods_checks=sizing_checks.replace('formal_component_sizes_example','formal_component_text_example').replace('register_component_sizes)','register_component_text)').replace('"components:sizes"','"components:text"').replace('assert!(output.stdout.contains("A                  B"));assert!(output.stdout.contains("Half"));','assert!(output.stdout.contains("[X]mixed_words[Y]"));assert!(output.stdout.contains("Mix…"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_text{\n'+methods_code+methods_checks+'}\n')
    lists_code=re.search(r'```rust\n(.*?)```',text.split('{#component-list-elements}',1)[1],re.S).group(1)
    lists_checks=methods_checks.replace('formal_component_text_example','formal_component_lists_example').replace('register_component_text)','register_component_lists)').replace('"components:text"','"components:lists"').replace('assert!(output.stdout.contains("[X]mixed_words[Y]"));assert!(output.stdout.contains("Mix…"));','assert!(output.stdout.contains("• Alpha"));assert!(output.stdout.contains("1. First"));assert!(output.stdout.contains("2. Second"));assert!(output.stdout.contains("────────────"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_lists{\n'+lists_code+lists_checks+'}\n')
    pre_code=re.search(r'```rust\n(.*?)```',text.split('{#component-preformatted-output}',1)[1],re.S).group(1)
    pre_checks=methods_checks.replace('formal_component_text_example','formal_component_pre_example').replace('register_component_text)','register_component_pre)').replace('"components:text"','"components:pre"').replace('assert!(output.stdout.contains("[X]mixed_words[Y]"));assert!(output.stdout.contains("Mix…"));','assert!(output.stdout.contains("One   "));assert!(output.stdout.contains("Two   "));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_pre{\n'+pre_code+pre_checks+'}\n')
    html_table_code=re.search(r'```rust\n(.*?)```',text.split('{#component-html-tables}',1)[1],re.S).group(1)
    html_table_checks=methods_checks.replace('formal_component_text_example','formal_component_table_example').replace('register_component_text)','register_component_table)').replace('"components:text"','"components:table"').replace('assert!(output.stdout.contains("[X]mixed_words[Y]"));assert!(output.stdout.contains("Mix…"));','assert!(output.stdout.contains("Report"));assert!(output.stdout.contains("Alpha"));assert!(output.stdout.contains("12"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_html_table{\n'+html_table_code+html_table_checks+'}\n')
    code_snippet=re.search(r'```rust\n(.*?)```',text.split('{#component-code-snippets}',1)[1],re.S).group(1)
    code_checks=methods_checks.replace('formal_component_text_example','formal_component_code_example').replace('register_component_text)','register_component_code)').replace('"components:text"','"components:code"').replace('assert!(output.stdout.contains("[X]mixed_words[Y]"));assert!(output.stdout.contains("Mix…"));','assert!(output.stdout.contains("first line"));assert!(output.stdout.contains("second line"));assert!(output.stdout.contains("➜"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_component_code{\n'+code_snippet+code_checks+'}\n')
    time_snippet=re.search(r'```rust\n(.*?)```',text.split('{#component-time-helpers}',1)[1],re.S).group(1)
    time_checks=methods_checks.replace('formal_component_text_example','formal_time_helpers_example').replace('register_component_text)','register_time_helpers)').replace('"components:text"','"components:time"').replace('assert!(output.stdout.contains("[X]mixed_words[Y]"));assert!(output.stdout.contains("Mix…"));','assert!(output.stdout.contains("delay=-5; runtime=100.00ms"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_time_helpers{\n'+time_snippet+time_checks+'}\n')





    helper_snippet=re.search(r'```rust\n(.*?)```',text.split('{#console-helper-tools}',1)[1],re.S).group(1)
    helper_checks=time_checks.replace('formal_time_helpers_example','formal_helper_tools_example').replace('register_time_helpers)','register_helper_tools)').replace('"components:time"','"helpers:measure"').replace('assert!(output.stdout.contains("delay=-5; runtime=100.00ms"));','assert!(output.stdout.contains("Ready 3/2 é 1 min, 1 s"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_helper_tools{\n'+helper_snippet+helper_checks+'}\n')

    helper_set_snippet=re.search(r'```rust\n(.*?)```',text.split('{#console-helper-sets}',1)[1],re.S).group(1)
    helper_set_checks=helper_checks.replace('formal_helper_tools_example','formal_helper_set_example').replace('register_helper_tools)','register_helper_set)').replace('"helpers:measure"','"helpers:lookup"').replace('assert!(output.stdout.contains("Ready 3/2 é 1 min, 1 s"));','assert!(output.stdout.contains("question has 2 entries"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_helper_set{\n'+helper_set_snippet+helper_set_checks+'}\n')

    formatter_helpers_snippet=re.search(r'```rust\n(.*?)```',text.split('{#console-formatter-helpers}',1)[1],re.S).group(1)
    formatter_helpers_checks=helper_checks.replace('formal_helper_tools_example','formal_formatter_helpers_example').replace('register_helper_tools)','register_formatter_helpers)').replace('"helpers:measure"','"helpers:format-example"').replace('assert!(output.stdout.contains("Ready 3/2 é 1 min, 1 s"));','assert_eq!(output.stdout.contains("\\x1b["),flag=="--ansi");let plain=FormatterHelper::remove_decoration(output.get_formatter().as_ref(),Some(&output.stdout)).unwrap();assert!(plain.contains("[check] Ready"),"{flag}: {plain:?}");assert!(plain.contains("Processing"));assert!(plain.contains("Done"));')
    parts.append('mod '+locale.replace('-','_').lower()+'_formatter_helpers{\n'+formatter_helpers_snippet+formatter_helpers_checks+'}\n')

    process_helper_snippet=re.search(r'```rust\n(.*?)```',text.split('{#console-process-helper}',1)[1],re.S).group(1)
    process_helper_checks=helper_checks.replace('formal_helper_tools_example','formal_process_helper_example').replace('register_helper_tools)','register_process_helpers)').replace('"helpers:measure"','"tools:git-version"').replace('assert!(output.stdout.contains("Ready 3/2 é 1 min, 1 s"));','assert!(output.stdout.contains("git version "),"{flag}: {:?}",output.stdout);')
    parts.append('mod '+locale.replace('-','_').lower()+'_process_helper{\n'+process_helper_snippet+process_helper_checks+'}\n')

    default_helpers_snippet=re.search(r'```rust\n(.*?)```',text.split('{#command-helper-sets}',1)[1],re.S).group(1)
    default_helpers_checks=process_helper_checks.replace('formal_process_helper_example','formal_default_helpers_example').replace('register_process_helpers)','register_default_helpers)').replace('"tools:git-version"','"tools:helpers"')
    parts.append('mod '+locale.replace('-','_').lower()+'_default_helpers{\n'+default_helpers_snippet+default_helpers_checks+'}\n')

    style_repository_snippet=re.search(r'```rust\n(.*?)```',text.split('{#terminal-style-repository}',1)[1],re.S).group(1)
    style_repository_checks=process_helper_checks.replace('formal_process_helper_example','formal_style_repository_example').replace('register_process_helpers)','register_style_repository)').replace('"tools:git-version"','"tools:status-style"').replace('"git version "','"READY FOR WORK"')
    parts.append('mod '+locale.replace('-','_').lower()+'_style_repository{\n'+style_repository_snippet+style_repository_checks+'}\n')

    terminal_style_snippet=re.search(r'```rust\n(.*?)```',text.split('{#typed-terminal-style-methods}',1)[1],re.S).group(1)
    terminal_style_checks=style_repository_checks.replace('formal_style_repository_example','formal_terminal_style_methods_example').replace('register_style_repository)','register_terminal_styles)').replace('"tools:status-style"','"tools:report-label"').replace('"READY FOR WORK"','"REPORT READY"')
    parts.append('mod '+locale.replace('-','_').lower()+'_terminal_style_methods{\n'+terminal_style_snippet+terminal_style_checks+'}\n')

    terminal_element_snippet=re.search(r'```rust\n(.*?)```',text.split('{#typed-terminal-elements}',1)[1],re.S).group(1)
    terminal_element_checks=terminal_style_checks.replace('formal_terminal_style_methods_example','formal_terminal_elements_example').replace('register_terminal_styles)','register_terminal_elements)').replace('"tools:report-label"','"tools:terminal-element"').replace('assert!(output.stdout.contains("REPORT READY"),"{flag}: {:?}",output.stdout);','let plain=openrust::console::Helper::remove_decoration(output.get_formatter().as_ref(),Some(&output.stdout)).unwrap();assert!(plain.contains("Status ready"),"{flag}: {plain:?}");')
    parts.append('mod '+locale.replace('-','_').lower()+'_terminal_elements{\n'+terminal_element_snippet+terminal_element_checks+'}\n')

    standalone_styles_snippet=re.search(r'```rust\n(.*?)```',text.split('{#standalone-terminal-styles}',1)[1],re.S).group(1)
    standalone_styles_checks=terminal_element_checks.replace('formal_terminal_elements_example','formal_standalone_style_example').replace('register_terminal_elements)','register_standalone_styles)').replace('"tools:terminal-element"','"tools:standalone-style"').replace('"Status ready"','"Available: 9 columns"')
    parts.append('mod '+locale.replace('-','_').lower()+'_standalone_styles{\n'+standalone_styles_snippet+standalone_styles_checks+'}\n')

    terminal_properties_snippet=re.search(r'```rust\n(.*?)```',text.split('{#terminal-style-properties}',1)[1],re.S).group(1)
    terminal_properties_checks=standalone_styles_checks.replace('formal_standalone_style_example','formal_terminal_style_properties_example').replace('register_standalone_styles)','register_style_properties)').replace('"tools:standalone-style"','"tools:style-properties"').replace('"Available: 9 columns"','"Properties ready"')
    parts.append('mod '+locale.replace('-','_').lower()+'_terminal_style_properties{\n'+terminal_properties_snippet+terminal_properties_checks+'}\n')

    style_attachment_snippet=re.search(r'```rust\n(.*?)```',text.split('{#terminal-style-attachment}',1)[1],re.S).group(1)
    style_attachment_checks=terminal_properties_checks.replace('formal_terminal_style_properties_example','formal_terminal_style_attachment_example').replace('register_style_properties)','register_attached_styles)').replace('"tools:style-properties"','"tools:attached-style"').replace('"Properties ready"','"Attached ready"')
    parts.append('mod '+locale.replace('-','_').lower()+'_terminal_style_attachment{\n'+style_attachment_snippet+style_attachment_checks+'}\n')

    style_inheritance_snippet=re.search(r'```rust\n(.*?)```',text.split('{#terminal-style-inheritance}',1)[1],re.S).group(1)
    style_inheritance_checks=style_attachment_checks.replace('formal_terminal_style_attachment_example','formal_terminal_style_inheritance_example').replace('register_attached_styles)','register_inherited_styles)').replace('"tools:attached-style"','"tools:inherited-style"').replace('"Attached ready"','"Inherited ready"')
    parts.append('mod '+locale.replace('-','_').lower()+'_terminal_style_inheritance{\n'+style_inheritance_snippet+style_inheritance_checks+'}\n')

    style_modifiers_snippet=re.search(r'```rust\n(.*?)```',text.split('{#terminal-style-modifiers}',1)[1],re.S).group(1)
    style_modifiers_checks=style_inheritance_checks.replace('formal_terminal_style_inheritance_example','formal_terminal_style_modifiers_example').replace('register_inherited_styles)','register_style_modifiers)').replace('"tools:inherited-style"','"tools:style-modifiers"').replace('"Inherited ready"','"Result: ready!"')
    parts.append('mod '+locale.replace('-','_').lower()+'_terminal_style_modifiers{\n'+style_modifiers_snippet+style_modifiers_checks+'}\n')

    dispatch_snippet=re.search(r'```rust\n(.*?)```',text.split('{#terminal-element-dispatch}',1)[1],re.S).group(1)
    dispatch_checks=style_modifiers_checks.replace('formal_terminal_style_modifiers_example','formal_terminal_element_dispatch_example').replace('register_style_modifiers)','register_element_dispatch)').replace('"tools:style-modifiers"','"tools:element-dispatch"').replace('"Result: ready!"','"Result: ready"')
    parts.append('mod '+locale.replace('-','_').lower()+'_terminal_element_dispatch{\n'+dispatch_snippet+dispatch_checks+'}\n')

    html_recovery_snippet=re.search(r'```rust\n(.*?)```',text.split('{#terminal-html-recovery}',1)[1],re.S).group(1)
    html_recovery_checks=dispatch_checks.replace('formal_terminal_element_dispatch_example','formal_terminal_html_recovery_example').replace('register_element_dispatch)','register_html_recovery)').replace('"tools:element-dispatch"','"tools:html-recovery"')
    parts.append('mod '+locale.replace('-','_').lower()+'_terminal_html_recovery{\n'+html_recovery_snippet+html_recovery_checks+'}\n')

    command_factory_snippet=re.search(r'```rust\n(.*?)```',text.split('{#command-component-factory}',1)[1],re.S).group(1)
    command_factory_checks = r'''
#[tokio::test]
async fn formal_command_component_factory_example() {
    let root = std::env::temp_dir().join(format!("artisan-command-factory-{}-{}", std::process::id(), module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app = Application::configure(root.clone())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(register_component_factory))
        .create()
        .unwrap();
    bind_component_factory(&app).unwrap();
    app.boot().await.unwrap();
    use openrust::support::facades::Facade as _;
    openrust::support::facades::App::set_facade_application(Some(&app), async {
        let mut output = openrust::console::Output::default();
        let code = Artisan::call("tools:component-factory", &["--no-ansi".into()], &mut output)
            .await.unwrap();
        assert_eq!(code, 0);
        assert_eq!(output.stdout, "Result: ready\n");
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
'''
    parts.append('mod '+locale.replace('-','_').lower()+'_command_component_factory{\n'+command_factory_snippet+command_factory_checks+'}\n')

    exit_codes_snippet=re.search(r'```rust\n(.*?)```',text.split('{#exit-codes}',1)[1],re.S).group(1)
    exit_codes_checks=r'''
#[tokio::test]
async fn formal_exit_codes_example() {
    use openrust::foundation::{Application, configuration::Routing};
    use openrust::support::facades::{App, Facade as _};
    let root = std::env::temp_dir().join(format!("artisan-exit-{}-{}", std::process::id(), module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app = Application::configure(root.clone())
        .with_routing(Routing::default().commands(register_exit_codes)).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let mut good = Output::default();
        assert_eq!(Artisan::call("jobs:check", &["ready".into(),"--no-ansi".into()],&mut good).await.unwrap(),0);
        assert_eq!(good.stdout, "Ready\n");
        let mut bad = Output::default();
        assert_eq!(Artisan::call("jobs:check", &["waiting".into(),"--no-ansi".into()],&mut bad).await.unwrap(),1);
        assert!(bad.stdout.contains("The job is not ready."));
        assert!(bad.stderr.is_empty());
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
'''
    parts.append('mod '+locale.replace('-','_').lower()+'_exit_codes{\n'+exit_codes_snippet+exit_codes_checks+'}\n')

    command_type_snippet=re.search(r'```rust\n(.*?)```',text.split('{#command-structure}',1)[1],re.S).group(1)
    command_type_checks=r'''
#[tokio::test]
async fn formal_command_type_example() {
    use openrust::support::facades::{App, Facade as _};
    let root = std::env::temp_dir().join(format!("artisan-command-type-{}-{}", std::process::id(), module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app = configure_commands(Application::configure(root.clone())).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let mut output = Output::default();
        assert_eq!(report_from_handler("Ada", &mut output).await.unwrap(),0);
        assert_eq!(output.stdout,"Report ready for Ada\n");
        let mut help = Output::default();
        assert_eq!(Artisan::call("reports:send", &["--help".into(),"--no-ansi".into()],&mut help).await.unwrap(),0);
        assert!(help.stdout.contains("Send a report"));
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
'''
    parts.append('mod '+locale.replace('-','_').lower()+'_command_type{\n'+command_type_snippet+command_type_checks+'}\n')

    call_args_snippet=re.search(r'```rust\n(.*?)```',text.split('{#passing-boolean-values}',1)[1],re.S).group(1)
    call_args_checks=r'''
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "reports:deliver {user} {--id=*} {--urgent}",
        |input: &openrust::console::Input, output: &mut Output| {
            input.io(output).line(&json!({
                "user":input.argument("user"),
                "id":input.option("id"),
                "urgent":input.option("urgent"),
            }).to_string())?;
            Ok(0)
        },
    )?;
    Ok(())
}

#[tokio::test]
async fn formal_named_call_arguments_example() {
    use openrust::foundation::{Application, configuration::Routing};
    use openrust::support::facades::{App, Facade as _};
    let root = std::env::temp_dir().join(format!("artisan-call-args-{}-{}", std::process::id(), module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app = Application::configure(root.clone()).with_routing(Routing::default().commands(register)).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let mut named = Output::default();
        let mut line = Output::default();
        assert_eq!(send_reports(&mut named).await.unwrap(), 0);
        assert_eq!(send_from_line(&mut line).await.unwrap(), 0);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&named.stdout).unwrap(),
            json!({"user":"Ada Lovelace","id":["5","13"],"urgent":false}));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&line.stdout).unwrap(),
            json!({"user":"Ada Lovelace","id":["5","13"],"urgent":true}));
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
'''
    parts.append('mod '+locale.replace('-','_').lower()+'_call_arguments{\n'+call_args_snippet+call_args_checks+'}\n')

    nested_calls_snippet=re.search(r'```rust\n(.*?)```',text.split('{#calling-commands-from-other-commands}',1)[1],re.S).group(1)
    nested_calls_checks=r'''
#[tokio::test]
async fn formal_nested_commands_example() {
    use openrust::foundation::{Application, configuration::Routing};
    use openrust::support::facades::{App, Facade as _};
    let root = std::env::temp_dir().join(format!("artisan-nested-{}-{}", std::process::id(), module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app = Application::configure(root.clone()).with_routing(Routing::default().commands(register_nested_commands)).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let (status, captured) = capture_report().await.unwrap();
        assert_eq!(status, 0);
        assert_eq!(captured.stdout, "Preparing\nReady\n");
        assert!(captured.stderr.is_empty());
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
'''
    parts.append('mod '+locale.replace('-','_').lower()+'_nested_calls{\n'+nested_calls_snippet+nested_calls_checks+'}\n')

    queue_named_snippet=re.search(r'```rust\n(.*?)```',text.split('{#queue-named-commands}',1)[1],re.S).group(1)
    queue_named_checks=r'''
use openrust::{
    config::{ConfigError,Environment},
    console::{AsyncCommandCallback,ConsoleError},
    contracts::foundation::Application as _,
    foundation::{Application, configuration::Routing},
    support::facades::{App, Facade as _},
};
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:deliver {user} {--id=*} {--urgent}", AsyncCommandCallback::new(|app,input,_| {
        Box::pin(async move {
            let data=json!({"user":input.argument("user"),"id":input.option("id"),"urgent":input.option("urgent")});
            std::fs::write(app.base_path().join("queue.json"),data.to_string())
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
fn configure(_: &std::path::Path, _: &Environment)
    -> Result<std::collections::BTreeMap<String,serde_json::Value>,ConfigError> {
    Ok(std::collections::BTreeMap::from([
        ("queue".into(),json!({"default":"sync","connections":{"sync":{"driver":"sync"}}}))
    ]))
}
#[tokio::test]
async fn formal_queued_artisan_arguments_example() {
    let root = std::env::temp_dir().join(format!("artisan-queue-named-{}-{}",std::process::id(),module_path!()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app = Application::configure(root.clone()).with_config(configure)
        .with_routing(Routing::default().commands(register)).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app),async {
        queue_report("sync").await.unwrap();
        let data:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("queue.json")).unwrap()).unwrap();
        assert_eq!(data,json!({"user":"Ada Lovelace","id":["5","13"],"urgent":false}));
    }).await;
    std::fs::remove_dir_all(root).unwrap();
}
'''
    parts.append('mod '+locale.replace('-','_').lower()+'_queue_named{\n'+queue_named_snippet+queue_named_checks+'}\n')


target=source/'tests/artisan_docs_generated.rs';target.write_text('\n'.join(parts));subprocess.run(['rustfmt','+1.94.0','--edition','2024',str(target)],check=True)
print('generated',len(parts),'direct formal consumers')
