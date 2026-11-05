//! 应用注册、邮件与密码重置路由；登录与因子挑战由所选Fortify Provider登记。
use crate::app::http::controllers::home::index as root;
use openrust::prelude::*;

pub fn register(routes: Routes) -> Routes {
    routes
        .get("/", root)
        .post("/auth/register", auth_register)
        .post(
            "/auth/request-email-verification",
            auth_request_email_verification,
        )
        .post("/auth/verify-email", auth_verify_email)
        .post("/auth/password-reset", auth_request_password_reset)
        .post("/auth/password-reset/confirm", auth_reset_password)
}

use openrust::auth::AuthFlows;
use openrust::http::ClientIp;
use openrust::mail::{
    Email, Mailer,
    lettre::{Message, message::Mailbox},
};

#[derive(serde::Deserialize)]
struct EmailPasswordInput {
    email: String,
    password: String,
}

#[derive(serde::Deserialize)]
struct VerificationInput {
    email: String,
    code: String,
}

#[derive(serde::Deserialize)]
struct EmailInput {
    email: String,
}

#[derive(serde::Deserialize)]
struct PasswordResetInput {
    email: String,
    code: String,
    password: String,
}

struct AuthCodeEmail {
    recipient: String,
    code: String,
    subject: String,
}

impl Email for AuthCodeEmail {
    fn message(&self) -> openrust::Result<Message> {
        let from: Mailbox = "OpenRust <no-reply@localhost>"
            .parse()
            .map_err(openrust::Error::internal)?;
        let to: Mailbox = self.recipient.parse().map_err(openrust::Error::internal)?;
        Message::builder()
            .from(from)
            .to(to)
            .subject(&self.subject)
            .body(format!("Your OpenRust verification code is: {}", self.code))
            .map_err(openrust::Error::internal)
    }
}

async fn send_auth_code(
    mailer: &Mailer,
    recipient: String,
    code: &str,
    subject: &str,
) -> openrust::Result<()> {
    mailer
        .send(&AuthCodeEmail {
            recipient,
            code: code.to_owned(),
            subject: subject.to_owned(),
        })
        .await
}

async fn auth_register(
    Inject(flows): Inject<AuthFlows>,
    Inject(mailer): Inject<Mailer>,
    client_ip: ClientIp,
    Json(input): Json<EmailPasswordInput>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, &'static str>>> {
    if let Some(code) = flows
        .register(&input.email, &input.password, &client_ip.ip().to_string())
        .await?
    {
        send_auth_code(&mailer, input.email, code.code(), "Verify your email").await?;
    }
    Ok(Json(std::collections::BTreeMap::from([(
        "status", "accepted",
    )])))
}

async fn auth_request_email_verification(
    Inject(flows): Inject<AuthFlows>,
    Inject(mailer): Inject<Mailer>,
    client_ip: ClientIp,
    Json(input): Json<EmailInput>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, &'static str>>> {
    if let Some(code) = flows
        .request_email_verification(&input.email, &client_ip.ip().to_string())
        .await?
    {
        send_auth_code(&mailer, input.email, code.code(), "Verify your email").await?;
    }
    Ok(Json(std::collections::BTreeMap::from([(
        "status", "accepted",
    )])))
}

async fn auth_verify_email(
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<VerificationInput>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, &'static str>>> {
    if !flows
        .verify_email(&input.email, &input.code, &client_ip.ip().to_string())
        .await?
    {
        return Err(openrust::Error::Unauthorized);
    }
    Ok(Json(std::collections::BTreeMap::from([(
        "status", "verified",
    )])))
}

async fn auth_request_password_reset(
    Inject(flows): Inject<AuthFlows>,
    Inject(mailer): Inject<Mailer>,
    client_ip: ClientIp,
    Json(input): Json<EmailInput>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, &'static str>>> {
    if let Some(code) = flows
        .request_password_reset(&input.email, &client_ip.ip().to_string())
        .await?
    {
        send_auth_code(&mailer, input.email, code.code(), "Reset your password").await?;
    }
    Ok(Json(std::collections::BTreeMap::from([(
        "status", "accepted",
    )])))
}

async fn auth_reset_password(
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<PasswordResetInput>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, &'static str>>> {
    if !flows
        .reset_password(
            &input.email,
            &input.code,
            &input.password,
            &client_ip.ip().to_string(),
        )
        .await?
    {
        return Err(openrust::Error::Unauthorized);
    }
    Ok(Json(std::collections::BTreeMap::from([(
        "status",
        "password_reset",
    )])))
}
