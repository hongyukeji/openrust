use openrust::prelude::*;
use openrust::queue::{Job, Queue};

pub fn register(routes: Routes) -> Routes {
    routes.get("/api", root).nest("/api", |api| {
        api.get("/me", current_user)
            .layer(openrust::auth::AuthLayer::required(
                openrust::auth::AuthMode::Jwt,
            ))
            .post("/auth/register", auth_register)
            .post(
                "/auth/request-email-verification",
                auth_request_email_verification,
            )
            .post("/auth/verify-email", auth_verify_email)
            .post("/auth/login", auth_login)
            .post("/auth/refresh", auth_refresh)
            .post("/auth/logout", auth_logout)
            .post("/auth/password-reset", auth_request_password_reset)
            .post("/auth/password-reset/confirm", auth_reset_password)
            .post("/jobs/example-heartbeat", enqueue_example_heartbeat)
            .merge(Routes::new().get("/me/pat", current_user).layer(
                openrust::auth::AuthLayer::required(openrust::auth::AuthMode::PersonalAccessToken),
            ))
    })
}

async fn enqueue_example_heartbeat(
    Inject(queue): Inject<Queue>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, String>>> {
    let id = queue
        .dispatch(
            "example",
            <crate::jobs::example_heartbeat::ExampleHeartbeatJob as Job>::NAME,
            &crate::jobs::example_heartbeat::ExampleHeartbeatJob,
        )
        .await?;
    Ok(Json(std::collections::BTreeMap::from([("job_id", id)])))
}
use openrust::auth::{AuthCredentials, AuthFlows};
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
struct RefreshInput {
    refresh_token: String,
}

#[derive(serde::Deserialize)]
struct PasswordResetInput {
    email: String,
    code: String,
    new_password: String,
}

#[derive(serde::Serialize)]
struct CredentialsResponse {
    user_id: i64,
    token_type: &'static str,
    access_token: String,
    refresh_token: String,
    refresh_expires_at: String,
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

fn credentials_response(credentials: AuthCredentials) -> Json<CredentialsResponse> {
    Json(CredentialsResponse {
        user_id: credentials.user_id(),
        token_type: "Bearer",
        access_token: credentials.access_token().to_owned(),
        refresh_token: credentials.refresh_token().expose().to_owned(),
        refresh_expires_at: credentials.refresh_expires_at().to_rfc3339(),
    })
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

async fn auth_login(
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<EmailPasswordInput>,
) -> openrust::Result<Json<CredentialsResponse>> {
    Ok(credentials_response(
        flows
            .login(&input.email, &input.password, &client_ip.ip().to_string())
            .await?,
    ))
}

async fn auth_refresh(
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<RefreshInput>,
) -> openrust::Result<Json<CredentialsResponse>> {
    Ok(credentials_response(
        flows
            .refresh(&input.refresh_token, &client_ip.ip().to_string())
            .await?,
    ))
}

async fn auth_logout(
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<RefreshInput>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, &'static str>>> {
    flows
        .logout(&input.refresh_token, &client_ip.ip().to_string())
        .await?;
    Ok(Json(std::collections::BTreeMap::from([(
        "status",
        "logged_out",
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
            &input.new_password,
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

async fn current_user(
    user: openrust::auth::CurrentUser,
) -> Json<std::collections::BTreeMap<&'static str, i64>> {
    Json(std::collections::BTreeMap::from([("user_id", user.id())]))
}

async fn root() -> Json<std::collections::BTreeMap<&'static str, &'static str>> {
    Json(std::collections::BTreeMap::from([(
        "name",
        env!("CARGO_PKG_NAME"),
    )]))
}
