//! 可复制到新生成应用的 Provider；只使用公开注册、装饰和路由 API。

use openrust::Result;
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::http::{Inject, Json};
use openrust::support::{BoxFuture, ServiceProvider};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct GeneratedMessage {
    text: String,
    extended: bool,
    registrations: Arc<AtomicUsize>,
}

#[derive(serde::Serialize)]
struct GeneratedResponse {
    message: String,
    extended: bool,
    same_boot_instance: bool,
    registrations: usize,
}

pub struct GeneratedExtensionProvider;

impl ServiceProvider for GeneratedExtensionProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let registrations = Arc::new(AtomicUsize::new(0));
            app.register_provider(GeneratedExtensionChild(Arc::clone(&registrations)))
                .await?;
            app.register_provider(GeneratedExtensionChild(registrations))
                .await?;
            Ok(())
        })
    }
}

struct GeneratedExtensionChild(Arc<AtomicUsize>);

impl ServiceProvider for GeneratedExtensionChild {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            let registrations = Arc::clone(&self.0);
            app.singleton::<GeneratedMessage, _>(move |_| {
                let registrations = Arc::clone(&registrations);
                Box::pin(async move {
                    Ok(Arc::new(GeneratedMessage {
                        text: "from original factory".to_owned(),
                        extended: false,
                        registrations,
                    }))
                })
            })
            .await?;
            app.extend::<GeneratedMessage, _>(|original, _| {
                Box::pin(async move {
                    Ok(Arc::new(GeneratedMessage {
                        text: original.text.clone(),
                        extended: true,
                        registrations: Arc::clone(&original.registrations),
                    }))
                })
            })
            .await?;
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let boot_message = app.resolve::<GeneratedMessage>().await?;
            app.routes(move |routes| {
                let boot_message = Arc::clone(&boot_message);
                routes.get(
                    "/api/generated/container-extension",
                    move |Inject(message): Inject<GeneratedMessage>| {
                        let boot_message = Arc::clone(&boot_message);
                        async move {
                            Json(GeneratedResponse {
                                message: message.text.clone(),
                                extended: message.extended,
                                same_boot_instance: Arc::ptr_eq(&message, &boot_message),
                                registrations: message.registrations.load(Ordering::SeqCst),
                            })
                        }
                    },
                )
            })?;
            Ok(())
        })
    }
}
