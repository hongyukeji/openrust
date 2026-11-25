#[path = "support/email_verification_identity.rs"]
mod fixture;
struct Core;
impl fixture::Flow for Core {}
#[tokio::test]
async fn verification_uses_primary_keys_not_email_auth_identifiers_with_real_http() {
    fixture::verify::<0, Core>().await;
    fixture::verify::<1, Core>().await;
    fixture::verify::<2, Core>().await;
}
