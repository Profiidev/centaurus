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

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema, aide::OperationIo))]
#[cfg_attr(feature = "backend", derive(axum::extract::FromRequestParts))]
#[cfg_attr(feature = "backend", from_request(via(axum::extract::Extension)))]
#[cfg_attr(feature = "db", derive(crate::Settings))]
#[cfg_attr(feature = "db", settings(id = 3))]
pub struct MailSettings {
  pub smtp_enabled: Option<bool>,
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
    if self.smtp_enabled.unwrap_or(false) {
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

#[derive(Debug)]
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_mail_settings_none() {
    let settings = MailSettings::default();
    assert!(settings.smtp().is_none());
  }

  #[test]
  fn test_mail_settings_some() {
    let settings = MailSettings {
      smtp_enabled: Some(true),
      smtp_server: Some("smtp.example.com".into()),
      smtp_port: Some(587),
      smtp_username: Some("user".into()),
      smtp_password: Some("pass".into()),
      smtp_from_address: Some("test@example.com".into()),
      smtp_from_name: Some("Test".into()),
      smtp_use_tls: Some(true),
    };

    let smtp = settings.smtp().unwrap();
    assert_eq!(smtp.server, "smtp.example.com");
    assert_eq!(smtp.port, 587);
    assert!(smtp.use_tls);
  }

  #[test]
  fn test_mail_settings_enabled_but_incomplete() {
    // Enabled but missing required fields must not yield an SmtpSettings.
    // port/username/password/from_* deliberately left unset
    let settings = MailSettings {
      smtp_enabled: Some(true),
      smtp_server: Some("smtp.example.com".into()),
      ..Default::default()
    };
    assert!(settings.smtp().is_none());
  }

  #[test]
  fn test_mail_settings_disabled_ignores_fields() {
    // With smtp_enabled = false, fully-populated fields are still ignored.
    let settings = MailSettings {
      smtp_enabled: Some(false),
      smtp_server: Some("smtp.example.com".into()),
      smtp_port: Some(587),
      smtp_username: Some("user".into()),
      smtp_password: Some("pass".into()),
      smtp_from_address: Some("test@example.com".into()),
      smtp_from_name: Some("Test".into()),
      smtp_use_tls: Some(true),
    };
    assert!(settings.smtp().is_none());
  }

  #[tokio::test]
  async fn test_mailer_inactive_without_config() {
    // A mailer built from default (disabled) settings is inactive and
    // refuses to send.
    let mailer = Mailer::new(MailSettings::default()).await;
    assert!(!mailer.is_active().await);
    assert!(
      mailer
        .send_mail("u".into(), "u@example.com".into(), "s".into(), "b".into())
        .await
        .is_err()
    );
  }

  #[tokio::test]
  async fn test_mailer_init_and_deactivate() {
    let smtp = SmtpSettings {
      server: "smtp.example.com".into(),
      port: 587,
      username: "user".into(),
      password: "pass".into(),
      from_address: "test@example.com".into(),
      from_name: "Test".into(),
      use_tls: true,
    };
    let mailer = Mailer(Arc::new(Mutex::new(None)));
    mailer.try_init(&smtp).await.unwrap();
    assert!(mailer.is_active().await);

    mailer.deactivate().await;
    assert!(!mailer.is_active().await);
  }

  #[test]
  fn test_mail_config_rejects_invalid_from_address() {
    let smtp = SmtpSettings {
      server: "smtp.example.com".into(),
      port: 587,
      username: "user".into(),
      password: "pass".into(),
      from_address: "not-an-email".into(),
      from_name: "Test".into(),
      use_tls: true,
    };
    assert!(MailConfig::new(&smtp).is_err());
  }
}
