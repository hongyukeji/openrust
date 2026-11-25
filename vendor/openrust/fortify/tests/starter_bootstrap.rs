//! 配置预检失败必须从真实Artisan输出保留具体原因，而不是只显示Provider名称。
use openrust::{
    console::{Application as Artisan, Output},
    contracts::console::Kernel as _,
    foundation::{Application, console::Kernel},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
struct Reject<const REGISTER: bool>;
impl<const REGISTER: bool> ServiceProvider for Reject<REGISTER> {
    fn register<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            if REGISTER {
                Err(std::io::Error::other("验收：配置的队列连接不存在").into())
            } else {
                Ok(())
            }
        })
    }
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            Err(std::io::Error::other("验收：用户清理队列必须使用同库数据库").into())
        })
    }
}
async fn verify<const REGISTER: bool>() {
    let app = Application::configure(std::env::temp_dir())
        .with_providers(vec![Box::new(Reject::<REGISTER>)])
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app, Artisan::new());
    let mut output = Output::default();
    assert_eq!(kernel.handle(&["list".into()], &mut output).await, 1);
    assert!(output.stderr.contains("Reject"), "{}", output.stderr);
    assert!(
        output.stderr.contains(if REGISTER {
            "配置的队列连接不存在"
        } else {
            "用户清理队列必须使用同库数据库"
        }),
        "{}",
        output.stderr
    );
}
#[tokio::test]
async fn provider_registration_errors_reach_the_artisan_caller() {
    verify::<true>().await;
}
#[tokio::test]
async fn provider_boot_errors_reach_the_artisan_caller() {
    verify::<false>().await;
}
