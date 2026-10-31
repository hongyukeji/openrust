//! 可复制到新生成应用的 Provider；只使用公开注册、装饰和路由 API。

use openrust::Result;
use openrust::core::{BootContext, BoxFuture, RegisterContext, ServiceProvider};
use openrust::http::{Inject, Json};
use std::sync::Arc;

struct GeneratedMessage {
    text: String,
    extended: bool,
}

#[derive(serde::Serialize)]
struct GeneratedResponse {
    message: String,
    extended: bool,
    same_boot_instance: bool,
}

pub struct GeneratedExtensionProvider;

impl ServiceProvider for GeneratedExtensionProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.singleton::<GeneratedMessage, _>(|_| {
                Box::pin(async {
                    Ok(Arc::new(GeneratedMessage {
                        text: "from original factory".to_owned(),
                        extended: false,
                    }))
                })
            })
            .await?;
            app.extend::<GeneratedMessage, _>(|original, _| {
                Box::pin(async move {
                    Ok(Arc::new(GeneratedMessage {
                        text: original.text.clone(),
                        extended: true,
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
                            })
                        }
                    },
                )
            })?;
            Ok(())
        })
    }
}
