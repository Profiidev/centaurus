#[cfg(feature = "backend")]
use std::convert::Infallible;
use std::sync::Arc;

#[cfg(feature = "backend")]
use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use eyre::Context;
#[cfg(feature = "http")]
use http::StatusCode;
use lettre::{
  AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
  message::{Mailbox, header::ContentType},
  transport::smtp::authentication::Credentials,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[cfg(feature = "http")]
use crate::error::ErrorReportStatusExt;
use crate::{bail, error::Result};

#[derive(Serialize, Deserialize, Default, Clone)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema, aide::OperationIo))]
#[cfg_attr(feature = "backend", derive(axum::extract::FromRequestParts))]
#[cfg_attr(feature = "backend", from_request(via(axum::extract::Extension)))]
#[cfg_attr(feature = "db", derive(crate::Settings))]
#[cfg_attr(feature = "db", settings(id = 3))]
pub struct MailSettings {
  pub smtp_enabled: bool,
  pub smtp_server: Option<String>,
  pub smtp_port: Option<u16>,
  pub smtp_username: Option<String>,
  pub smtp_password: Option<String>,
  pub smtp_from_address: Option<String>,
  pub smtp_from_name: Option<String>,
  pub smtp_use_tls: Option<bool>,
}

#[cfg(feature = "backend")]
impl<S: Send + Sync> OptionalFromRequestParts<S> for MailSettings {
  type Rejection = Infallible;

  async fn from_request_parts(
    parts: &mut http::request::Parts,
    state: &S,
  ) -> std::result::Result<Option<Self>, Self::Rejection> {
    Ok(
      <Self as FromRequestParts<S>>::from_request_parts(parts, state)
        .await
        .ok(),
    )
  }
}

impl MailSettings {
  pub fn smtp(&self) -> Option<SmtpSettings> {
    if self.smtp_enabled {
      Some(SmtpSettings {
        server: self.smtp_server.clone()?,
        port: self.smtp_port?,
        username: self.smtp_username.clone()?,
        password: self.smtp_password.clone()?,
        from_address: self.smtp_from_address.clone()?,
        from_name: self.smtp_from_name.clone()?,
        use_tls: self.smtp_use_tls?,
      })
    } else {
      None
    }
  }
}

pub struct SmtpSettings {
  pub server: String,
  pub port: u16,
  pub username: String,
  pub password: String,
  pub from_address: String,
  pub from_name: String,
  pub use_tls: bool,
}

#[derive(Clone)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
#[cfg_attr(feature = "backend", derive(axum::extract::FromRequestParts))]
#[cfg_attr(feature = "backend", from_request(via(axum::extract::Extension)))]
pub struct Mailer(Arc<Mutex<Option<MailConfig>>>);

struct MailConfig {
  sender: Mailbox,
  transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl Mailer {
  pub async fn new(settings: MailSettings) -> Self {
    let state = Mailer(Arc::new(Mutex::new(None)));
    if let Some(smtp_config) = settings.smtp() {
      state.try_init(&smtp_config).await.ok();
    }
    state
  }

  pub async fn try_init(&self, smtp_config: &SmtpSettings) -> Result<()> {
    let mut guard = self.0.lock().await;
    let config = MailConfig::new(smtp_config)?;
    *guard = Some(config);
    Ok(())
  }

  pub async fn deactivate(&self) {
    let mut guard = self.0.lock().await;
    *guard = None;
  }

  pub async fn is_active(&self) -> bool {
    let guard = self.0.lock().await;
    guard.is_some()
  }

  pub async fn send_mail(
    &self,
    username: String,
    email: String,
    subject: String,
    body: String,
  ) -> Result<()> {
    let lock = self.0.lock().await;
    if let Some(config) = &*lock {
      config.send_mail(username, email, subject, body).await
    } else {
      bail!("Mail service is not configured");
    }
  }
}

impl MailConfig {
  fn new(smtp_config: &SmtpSettings) -> Result<Self> {
    let credentials = Credentials::new(smtp_config.username.clone(), smtp_config.password.clone());

    let relay = if smtp_config.use_tls {
      AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp_config.server)
    } else {
      AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp_config.server)
    };
    #[cfg(feature = "http")]
    let transport_builder =
      relay.status_context(StatusCode::BAD_REQUEST, "Failed to create SMTP transport")?;
    #[cfg(not(feature = "http"))]
    let transport_builder = relay.context("Failed to create SMTP transport")?;

    let transport = transport_builder
      .port(smtp_config.port)
      .credentials(credentials)
      .build();

    let email_result = smtp_config.from_address.clone().parse();

    #[cfg(feature = "http")]
    let email = email_result.status_context(StatusCode::NOT_ACCEPTABLE, "Invalid from address")?;
    #[cfg(not(feature = "http"))]
    let email = email_result.context("Invalid from address")?;

    let sender = Mailbox::new(Some(smtp_config.from_name.clone()), email);

    Ok(MailConfig { sender, transport })
  }

  pub async fn send_mail(
    &self,
    username: String,
    email: String,
    subject: String,
    body: String,
  ) -> Result<()> {
    let receiver = Mailbox::new(
      Some(username),
      email.parse().with_context(|| "Invalid email")?,
    );

    let mail = Message::builder()
      .from(self.sender.clone())
      .to(receiver)
      .subject(subject)
      .header(ContentType::TEXT_HTML)
      .body(body)?;

    self
      .transport
      .send(mail)
      .await
      .with_context(|| "Failed to send email")?;

    Ok(())
  }
}
