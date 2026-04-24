use crate::{
  backend::{
    auth::{jwt_auth::JwtAuth, permission::SettingsEdit},
    config::SiteConfig,
  },
  db::{init::Connection, tables::ConnectionExt},
  error::Result,
  mail::Mailer,
};
use aide::axum::routing::post_with;
use aide::axum::{ApiRouter, routing::ApiMethodRouter};

use crate::backend::endpoints::mail::template;

pub fn router() -> ApiRouter {
  ApiRouter::new().api_route("/", test_mail_route())
}

pub fn test_mail_route() -> ApiMethodRouter<()> {
  post_with(test_mail, |op| op.id("testMail"))
}

async fn test_mail(
  auth: JwtAuth<SettingsEdit>,
  mailer: Mailer,
  config: SiteConfig,
  db: Connection,
) -> Result<()> {
  let user = db.user().get_user_by_id(auth.user_id).await?;
  let link = config.site_url;

  mailer
    .send_mail(
      user.name,
      user.email,
      "Test Email".to_string(),
      template::test_email(link.as_str()),
    )
    .await?;

  Ok(())
}
