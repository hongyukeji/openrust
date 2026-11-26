use openrust::foundation::Application;
use openrust_passport::{ClientOwner, ClientRecord, ClientRepository};

pub async fn create_client(
    app: &Application,
    owner: Option<&ClientOwner>,
) -> Result<ClientRecord, Box<dyn std::error::Error + Send + Sync>> {
    let clients = app.make::<ClientRepository>().await?;
    Ok(clients
        .create_authorization_code_grant_client(
            "My application",
            &["https://example.com/callback".to_owned()],
            true,
            owner,
            false,
        )
        .await?)
}
