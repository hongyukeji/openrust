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
target=source/'tests/artisan_docs_generated.rs';target.write_text('\n'.join(parts));subprocess.run(['rustfmt','+1.94.0','--edition','2024',str(target)],check=True)
print('generated',len(parts),'direct formal consumers')
