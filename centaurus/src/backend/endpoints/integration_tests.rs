//! Full-stack endpoint integration tests.
//!
//! These build a real axum application wired with every extension the handlers
//! depend on (database, JWT/password state, websocket updater, mailer, storage,
//! …) and drive it with `tower::ServiceExt::oneshot`, mirroring the official
//! axum testing example. Authentication is performed by minting JWTs directly
//! through [`JwtState`] and replaying them as bearer tokens.

use aide::axum::ApiRouter;
use axum::{Extension, Router, body::Body};
use base64::prelude::*;
use http::{Method, Request, StatusCode, header::CONTENT_TYPE};
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs1::DecodeRsaPublicKey, rand_core::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use crate::backend::auth::jwt_state::{JWT_COOKIE_NAME, JwtInvalidState, JwtState};
use crate::backend::auth::oidc::OidcState;
use crate::backend::auth::permission::permissions;
use crate::backend::auth::pw_state::PasswordState;
use crate::backend::auth::settings::{AuthConfig, UserSettings};
use crate::backend::config::SiteConfig;
use crate::backend::endpoints::mail::state::ResetPasswordState;
use crate::backend::endpoints::user::email::EmailChangeState;
use crate::backend::endpoints::websocket::state::UpdateState;
use crate::backend::endpoints::{group, mail, settings, setup, user, websocket};
use crate::backend::middleware::rate_limiter::RateLimiter;
use crate::backend::{self, auth};
use crate::db::config::DBConfig;
use crate::db::init::{Connection, connect_db};
use crate::db::migrations::Migrator;
use crate::db::tables::ConnectionExt;
use crate::mail::{MailSettings, Mailer};
#[cfg(feature = "storage")]
use crate::storage::FileStorage;
use sea_orm_migration::MigratorTrait;

/// A minimal [`UpdateMessage`] enum so the websocket-aware handlers can be
/// instantiated in tests.
#[derive(Serialize, Deserialize, Clone, Debug, crate::UpdateMessage)]
enum TestMsg {
  #[update_message(settings)]
  Settings,
  #[update_message(group)]
  Group { uuid: Uuid },
  #[update_message(user)]
  User { uuid: Uuid },
  #[update_message(user_permissions)]
  Permissions,
}

const SALT: &str = "c2FsdHNhbHQ"; // base64 (no pad) of "saltsalt"

struct TestApp {
  app: Router,
  conn: Connection,
  jwt: JwtState,
  pw: PasswordState,
  pw_pub: RsaPublicKey,
}

impl TestApp {
  async fn new() -> Self {
    let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();

    let auth_config = AuthConfig::default();
    let jwt = JwtState::init(&auth_config, &conn).await;
    let pw = auth::init_pw_state(&auth_config, &conn).await;
    let pw_pub = RsaPublicKey::from_pkcs1_pem(&pw.pub_key).unwrap();

    let (update_state, updater) = UpdateState::<TestMsg>::init().await;
    let oidc = OidcState::new(&conn, None).await;
    let mailer = Mailer::new(MailSettings::default()).await;

    let mut rl = RateLimiter::default();
    let api: ApiRouter = ApiRouter::new()
      .nest("/setup", setup::router())
      .nest("/settings", settings::router::<TestMsg>())
      .nest("/group", group::router::<TestMsg>())
      .nest("/user", user::router::<TestMsg>(&mut rl))
      .nest("/mail", mail::router(&mut rl))
      .nest("/auth", auth::router::<TestMsg>(&mut rl))
      .nest("/ws", websocket::router::<TestMsg>())
      .merge(backend::endpoints::health::router())
      .layer(Extension(conn.clone()))
      .layer(Extension(jwt.clone()))
      .layer(Extension(pw.clone()))
      .layer(Extension(JwtInvalidState::default()))
      .layer(Extension(oidc))
      .layer(Extension(update_state))
      .layer(Extension(updater))
      .layer(Extension(EmailChangeState::init()))
      .layer(Extension(ResetPasswordState::default()))
      .layer(Extension(mailer))
      .layer(Extension(SiteConfig::default()))
      .layer(Extension(UserSettings::default()))
      .layer(Extension(MailSettings::default()));
    #[cfg(feature = "storage")]
    let api = api.layer(Extension(FileStorage::Local(std::env::temp_dir())));
    rl.init();

    let mut openapi = aide::openapi::OpenApi::default();
    let app = api.finish_api(&mut openapi);

    Self {
      app,
      conn,
      jwt,
      pw,
      pw_pub,
    }
  }

  fn token(&self, user: Uuid) -> String {
    self.jwt.create_raw_token(user).unwrap()
  }

  /// base64(RSA-encrypted) password, the wire format the handlers expect.
  fn encrypt(&self, plain: &str) -> String {
    let ct = self
      .pw_pub
      .encrypt(&mut OsRng, Pkcs1v15Encrypt, plain.as_bytes())
      .unwrap();
    BASE64_STANDARD.encode(ct)
  }

  /// Create the admin group with every permission and return a user in it.
  async fn admin_user(&self, name: &str) -> Uuid {
    let perms: Vec<String> = permissions().into_iter().map(|p| p.to_string()).collect();
    let group = self
      .conn
      .group()
      .create_group("Admin".into())
      .await
      .unwrap();
    self
      .conn
      .setup()
      .set_admin_group_created(group)
      .await
      .unwrap();
    self
      .conn
      .group()
      .add_permissions_to_group(group, perms)
      .await
      .unwrap();
    let uid = self.local_user(name, "password").await;
    self
      .conn
      .group()
      .add_users_to_group(group, vec![uid])
      .await
      .unwrap();
    uid
  }

  /// Create a local (non-OIDC) user whose stored hash matches `plain` when sent
  /// through the login flow (i.e. encrypted on the wire).
  async fn local_user(&self, name: &str, plain: &str) -> Uuid {
    let enc = self.encrypt(plain);
    let hash = self.pw.pw_hash(SALT, &enc).unwrap();
    self
      .conn
      .user()
      .create_user(
        name.into(),
        format!("{name}@example.com"),
        hash,
        SALT.into(),
        false,
      )
      .await
      .unwrap()
  }

  async fn send(
    &self,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
  ) -> (StatusCode, Value) {
    let mut builder = Request::builder()
      .method(method)
      .uri(uri)
      .header("x-real-ip", "127.0.0.1");
    if let Some(t) = token {
      builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let req = match &body {
      Some(v) => builder
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(v.to_string()))
        .unwrap(),
      None => builder.body(Body::empty()).unwrap(),
    };

    let resp = self.app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
      .await
      .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
  }
}

// ---------------------------------------------------------------------------
// health
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_ok() {
  let app = TestApp::new().await;
  let (status, _) = app.send(Method::GET, "/health", None, None).await;
  assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------

#[tokio::test]
async fn setup_is_setup_reports_backends() {
  let app = TestApp::new().await;
  let (status, body) = app.send(Method::GET, "/setup", None, None).await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(body["is_setup"], json!(false));
  assert_eq!(body["db_backend"], json!("SQLite"));
  assert_eq!(body["storage_backend"], json!("Local"));
}

#[tokio::test]
async fn setup_complete_flow_and_invariants() {
  let app = TestApp::new().await;
  let pw = app.encrypt("admin-pw");

  // No admin group yet → internal error.
  let (status, _) = app
    .send(
      Method::POST,
      "/setup",
      None,
      Some(json!({"admin_username":"a","admin_password":pw,"admin_email":"a@x.com"})),
    )
    .await;
  assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

  // Create the admin group (as the real setup bootstrap would).
  let group = app.conn.group().create_group("Admin".into()).await.unwrap();
  app
    .conn
    .setup()
    .set_admin_group_created(group)
    .await
    .unwrap();

  // Empty username is rejected.
  let (status, _) = app
    .send(
      Method::POST,
      "/setup",
      None,
      Some(json!({"admin_username":"  ","admin_password":pw,"admin_email":"a@x.com"})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);

  // Valid completion.
  let (status, body) = app
    .send(
      Method::POST,
      "/setup",
      None,
      Some(json!({"admin_username":"admin","admin_password":pw,"admin_email":"a@x.com"})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  assert!(body["user"].is_string());
  assert!(app.conn.setup().is_setup().await.unwrap());

  // Second completion conflicts.
  let (status, _) = app
    .send(
      Method::POST,
      "/setup",
      None,
      Some(json!({"admin_username":"admin","admin_password":pw,"admin_email":"a@x.com"})),
    )
    .await;
  assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn setup_oidc_settings_gate_on_completion() {
  let app = TestApp::new().await;

  // Before setup, OIDC settings are readable.
  let (status, _) = app.send(Method::GET, "/setup/oidc", None, None).await;
  assert_eq!(status, StatusCode::OK);

  // init_oidc with OIDC disabled just deactivates + saves.
  let (status, _) = app
    .send(Method::POST, "/setup/oidc", None, Some(json!({})))
    .await;
  assert_eq!(status, StatusCode::OK);

  // After completion the endpoints are forbidden.
  app.conn.setup().mark_completed().await.unwrap();
  let (status, _) = app.send(Method::GET, "/setup/oidc", None, None).await;
  assert_eq!(status, StatusCode::FORBIDDEN);
  let (status, _) = app
    .send(Method::POST, "/setup/oidc", None, Some(json!({})))
    .await;
  assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// auth: password (login + key), logout, test_token, config
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_key_returns_public_key() {
  let app = TestApp::new().await;
  let (status, body) = app.send(Method::GET, "/auth/password", None, None).await;
  assert_eq!(status, StatusCode::OK);
  assert!(
    body["key"]
      .as_str()
      .unwrap()
      .contains("BEGIN RSA PUBLIC KEY")
  );
}

#[tokio::test]
async fn auth_login_success_and_failures() {
  let app = TestApp::new().await;
  let uid = app.local_user("bob", "s3cret").await;
  let enc = app.encrypt("s3cret");

  // Correct credentials.
  let (status, body) = app
    .send(
      Method::POST,
      "/auth/password",
      None,
      Some(json!({"email":"bob@example.com","password":enc})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(body["user"], json!(uid.to_string()));

  // Wrong password → unauthorized.
  let wrong = app.encrypt("nope");
  let (status, _) = app
    .send(
      Method::POST,
      "/auth/password",
      None,
      Some(json!({"email":"bob@example.com","password":wrong})),
    )
    .await;
  assert_eq!(status, StatusCode::UNAUTHORIZED);

  // Unknown user → the lookup fails and surfaces as a 400.
  let (status, _) = app
    .send(
      Method::POST,
      "/auth/password",
      None,
      Some(json!({"email":"ghost@example.com","password":enc})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn auth_test_token_reflects_validity() {
  let app = TestApp::new().await;
  let uid = app.local_user("tt", "pw").await;
  let token = app.token(uid);

  let (status, body) = app
    .send(Method::GET, "/auth/test_token", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(body["valid"], json!(true));

  let (status, body) = app.send(Method::GET, "/auth/test_token", None, None).await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(body["valid"], json!(false));
}

#[tokio::test]
async fn auth_config_defaults() {
  let app = TestApp::new().await;
  let (status, body) = app.send(Method::GET, "/auth/config", None, None).await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(body["sso_type"], json!("None"));
  assert_eq!(body["instant_redirect"], json!(false));
  assert_eq!(body["mail_enabled"], json!(false));
}

#[tokio::test]
async fn auth_logout_invalidates_token() {
  let app = TestApp::new().await;
  let uid = app.local_user("lo", "pw").await;
  let token = app.token(uid);

  // Logout requires the JWT cookie to be present.
  let req = Request::builder()
    .method(Method::POST)
    .uri("/auth/logout")
    .header("x-real-ip", "127.0.0.1")
    .header("authorization", format!("Bearer {token}"))
    .header("cookie", format!("{JWT_COOKIE_NAME}={token}"))
    .body(Body::empty())
    .unwrap();
  let resp = app.app.clone().oneshot(req).await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // The invalidated token is now rejected on protected routes.
  assert!(!app.conn.invalid_jwt().is_token_valid(&token).await.unwrap());
  let (status, _) = app
    .send(Method::GET, "/user/info", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_logout_without_cookie_is_unauthorized() {
  let app = TestApp::new().await;
  let uid = app.local_user("lo2", "pw").await;
  let token = app.token(uid);
  let (status, _) = app
    .send(Method::POST, "/auth/logout", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// user/info
// ---------------------------------------------------------------------------

#[tokio::test]
async fn user_info_requires_auth() {
  let app = TestApp::new().await;
  // No credentials at all ⇒ the token cannot be located ⇒ 400.
  let (status, _) = app.send(Method::GET, "/user/info", None, None).await;
  assert_eq!(status, StatusCode::BAD_REQUEST);

  // A syntactically present but bogus bearer token ⇒ 401.
  let (status, _) = app
    .send(Method::GET, "/user/info", Some("not-a-real-jwt"), None)
    .await;
  assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn user_info_returns_profile() {
  let app = TestApp::new().await;
  let uid = app.local_user("ui", "pw").await;
  let token = app.token(uid);
  let (status, body) = app
    .send(Method::GET, "/user/info", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(body["email"], json!("ui@example.com"));
  assert_eq!(body["oidc_user"], json!(false));
}

#[tokio::test]
async fn user_avatar_missing_is_not_found() {
  let app = TestApp::new().await;
  let uid = app.local_user("av", "pw").await;
  let token = app.token(uid);
  let other = Uuid::new_v4();
  let (status, _) = app
    .send(
      Method::GET,
      &format!("/user/info/avatar/{other}"),
      Some(&token),
      None,
    )
    .await;
  assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// user/account
// ---------------------------------------------------------------------------

#[tokio::test]
async fn account_update_name() {
  let app = TestApp::new().await;
  let uid = app.local_user("acc", "pw").await;
  let token = app.token(uid);

  let (status, _) = app
    .send(
      Method::POST,
      "/user/account/update",
      Some(&token),
      Some(json!({"username":"  "})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);

  let (status, _) = app
    .send(
      Method::POST,
      "/user/account/update",
      Some(&token),
      Some(json!({"username":"renamed"})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(
    app.conn.user().get_user_by_id(uid).await.unwrap().name,
    "renamed"
  );
}

#[tokio::test]
async fn account_change_password_paths() {
  let app = TestApp::new().await;
  let uid = app.local_user("pwch", "old-pw").await;
  let token = app.token(uid);
  let old = app.encrypt("old-pw");
  let new = app.encrypt("new-pw");

  // Wrong old password.
  let (status, _) = app
    .send(
      Method::POST,
      "/user/account/password",
      Some(&token),
      Some(json!({"old_password":app.encrypt("bad"),"new_password":new})),
    )
    .await;
  assert_eq!(status, StatusCode::FORBIDDEN);

  // Correct rotation.
  let (status, _) = app
    .send(
      Method::POST,
      "/user/account/password",
      Some(&token),
      Some(json!({"old_password":old,"new_password":new})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// user/management
// ---------------------------------------------------------------------------

#[tokio::test]
async fn management_requires_permission() {
  let app = TestApp::new().await;
  // A plain user without UserView cannot list users.
  let uid = app.local_user("plain", "pw").await;
  let token = app.token(uid);
  let (status, _) = app
    .send(Method::GET, "/user/management", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn management_crud_flow() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);

  // List users (admin only at first).
  let (status, body) = app
    .send(Method::GET, "/user/management", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(body.as_array().unwrap().len(), 1);

  // mailActive / groups / users-simple.
  let (status, body) = app
    .send(Method::GET, "/user/management/mail", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(body["active"], json!(false));
  let (status, _) = app
    .send(Method::GET, "/user/management/groups", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);

  // Create a user (mailer inactive ⇒ password required, encrypted).
  let pw = app.encrypt("created-pw");
  let (status, body) = app
    .send(
      Method::POST,
      "/user/management",
      Some(&token),
      Some(json!({"name":"created","email":"created@example.com","password":pw})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  let new_id = body["uuid"].as_str().unwrap().to_string();

  // Duplicate email conflicts.
  let (status, _) = app
    .send(
      Method::POST,
      "/user/management",
      Some(&token),
      Some(json!({"name":"created2","email":"created@example.com","password":pw})),
    )
    .await;
  assert_eq!(status, StatusCode::CONFLICT);

  // Empty name rejected.
  let (status, _) = app
    .send(
      Method::POST,
      "/user/management",
      Some(&token),
      Some(json!({"name":" ","email":"x@example.com","password":pw})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);

  // Missing password (mailer inactive) rejected.
  let (status, _) = app
    .send(
      Method::POST,
      "/user/management",
      Some(&token),
      Some(json!({"name":"nopw","email":"nopw@example.com","password":null})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);

  // User info by id.
  let (status, _) = app
    .send(
      Method::GET,
      &format!("/user/management/{new_id}"),
      Some(&token),
      None,
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  let (status, _) = app
    .send(
      Method::GET,
      &format!("/user/management/{}", Uuid::new_v4()),
      Some(&token),
      None,
    )
    .await;
  assert_eq!(status, StatusCode::NOT_FOUND);

  // Edit user (rename, no groups).
  let (status, _) = app
    .send(
      Method::PUT,
      "/user/management",
      Some(&token),
      Some(json!({"uuid":new_id,"name":"edited","groups":[]})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);

  // Reset the created user's password (mailer inactive).
  let (status, _) = app
    .send(
      Method::PUT,
      "/user/management/password",
      Some(&token),
      Some(json!({"uuid":new_id,"new_password":app.encrypt("reset-pw")})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);

  // Delete the created user.
  let (status, _) = app
    .send(
      Method::DELETE,
      "/user/management",
      Some(&token),
      Some(json!({"uuid":new_id})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn management_cannot_delete_last_admin() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);
  let (status, _) = app
    .send(
      Method::DELETE,
      "/user/management",
      Some(&token),
      Some(json!({"uuid":admin.to_string()})),
    )
    .await;
  assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn management_convert_oidc_user() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);

  // An OIDC user can be converted to local; a local one cannot.
  let oidc_uid = app
    .conn
    .user()
    .create_user(
      "oidc".into(),
      "oidc@example.com".into(),
      "h".into(),
      SALT.into(),
      true,
    )
    .await
    .unwrap();
  let (status, _) = app
    .send(
      Method::PUT,
      "/user/management/convert-oidc",
      Some(&token),
      Some(json!({"uuid":oidc_uid.to_string(),"new_password":app.encrypt("pw")})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  assert!(
    !app
      .conn
      .user()
      .get_user_by_id(oidc_uid)
      .await
      .unwrap()
      .oidc_user
  );

  let (status, _) = app
    .send(
      Method::PUT,
      "/user/management/convert-oidc",
      Some(&token),
      Some(json!({"uuid":oidc_uid.to_string(),"new_password":app.encrypt("pw")})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);
}

// A genuinely valid 1x1 PNG, base64-encoded.
#[cfg(feature = "avatar")]
const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAEElEQVR4AQEFAPr/AAoUHv8BpAE8tOS4KAAAAABJRU5ErkJggg==";

#[cfg(feature = "avatar")]
#[tokio::test]
async fn account_avatar_update_and_invalid() {
  let app = TestApp::new().await;
  let uid = app.local_user("avup", "pw").await;
  let token = app.token(uid);

  // A valid PNG is decoded, resized to 128x128, re-encoded to WebP and stored.
  let (status, _) = app
    .send(
      Method::POST,
      "/user/account/avatar",
      Some(&token),
      Some(json!({"avatar": PNG_1X1})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  assert!(
    app
      .conn
      .user()
      .get_user_avatar(uid)
      .await
      .unwrap()
      .is_some()
  );

  // Garbage that is not valid base64 is rejected as a bad request (not a 500).
  let (status, _) = app
    .send(
      Method::POST,
      "/user/account/avatar",
      Some(&token),
      Some(json!({"avatar": "!!!not base64!!!"})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);

  // Valid base64 that is not a decodable image is also a bad request.
  let (status, _) = app
    .send(
      Method::POST,
      "/user/account/avatar",
      Some(&token),
      Some(json!({"avatar": BASE64_STANDARD.encode("not an image")})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// user/email change flows
// ---------------------------------------------------------------------------

#[tokio::test]
async fn management_change_email_invariants() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);
  let target = app.local_user("target", "pw").await;

  // Empty email rejected (mailer is inactive so the handler proceeds past the
  // active-mailer guard).
  let (status, _) = app
    .send(
      Method::POST,
      "/user/management/email",
      Some(&token),
      Some(json!({"uuid":target.to_string(),"new_email":""})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);

  // Valid change succeeds and is persisted (lower-cased).
  let (status, _) = app
    .send(
      Method::POST,
      "/user/management/email",
      Some(&token),
      Some(json!({"uuid":target.to_string(),"new_email":"Moved@Example.com"})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(
    app.conn.user().get_user_by_id(target).await.unwrap().email,
    "moved@example.com"
  );

  // Changing to an address already in use conflicts.
  let (status, _) = app
    .send(
      Method::POST,
      "/user/management/email",
      Some(&token),
      Some(json!({"uuid":target.to_string(),"new_email":"admin@example.com"})),
    )
    .await;
  assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn account_email_change_confirm_without_request_is_not_found() {
  let app = TestApp::new().await;
  let uid = app.local_user("ec", "pw").await;
  let token = app.token(uid);
  let (status, _) = app
    .send(
      Method::POST,
      "/user/account/email_change_confirm",
      Some(&token),
      Some(json!({"new_code":"000000","old_code":"000000"})),
    )
    .await;
  assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// permission invariants
// ---------------------------------------------------------------------------

#[tokio::test]
async fn edit_user_cannot_grant_unheld_permissions() {
  let app = TestApp::new().await;
  app.admin_user("admin").await; // sets up the admin group

  // An editor that only holds `user:edit`.
  let editor = app.local_user("editor", "pw").await;
  let editor_group = app
    .conn
    .group()
    .create_group("editors".into())
    .await
    .unwrap();
  app
    .conn
    .group()
    .add_permissions_to_group(editor_group, vec!["user:edit".into()])
    .await
    .unwrap();
  app
    .conn
    .group()
    .add_users_to_group(editor_group, vec![editor])
    .await
    .unwrap();

  // A privileged group the editor must not be able to assign.
  let priv_group = app.conn.group().create_group("priv".into()).await.unwrap();
  app
    .conn
    .group()
    .add_permissions_to_group(priv_group, vec!["settings:edit".into()])
    .await
    .unwrap();

  let victim = app.local_user("victim", "pw").await;
  let token = app.token(editor);

  let (status, _) = app
    .send(
      Method::PUT,
      "/user/management",
      Some(&token),
      Some(json!({"uuid":victim.to_string(),"name":"v","groups":[priv_group.to_string()]})),
    )
    .await;
  assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// group
// ---------------------------------------------------------------------------

#[tokio::test]
async fn group_crud_flow() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);

  // Create.
  let (status, body) = app
    .send(
      Method::POST,
      "/group",
      Some(&token),
      Some(json!({"name":"team"})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
  let gid = body["uuid"].as_str().unwrap().to_string();

  // Duplicate name.
  let (status, _) = app
    .send(
      Method::POST,
      "/group",
      Some(&token),
      Some(json!({"name":"team"})),
    )
    .await;
  assert_eq!(status, StatusCode::CONFLICT);

  // Empty name.
  let (status, _) = app
    .send(
      Method::POST,
      "/group",
      Some(&token),
      Some(json!({"name":"  "})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);

  // List + info + users-simple.
  let (status, _) = app.send(Method::GET, "/group", Some(&token), None).await;
  assert_eq!(status, StatusCode::OK);
  let (status, _) = app
    .send(Method::GET, &format!("/group/{gid}"), Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);
  let (status, _) = app
    .send(Method::GET, "/group/users", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);

  // Edit (no privileged permissions involved).
  let (status, _) = app
    .send(
      Method::PUT,
      "/group",
      Some(&token),
      Some(json!({"uuid":gid,"name":"team2","permissions":[],"users":[]})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);

  // Delete.
  let (status, _) = app
    .send(
      Method::DELETE,
      "/group",
      Some(&token),
      Some(json!({"uuid":gid})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn group_cannot_delete_admin_group() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);
  let admin_group = app
    .conn
    .setup()
    .get_admin_group_id()
    .await
    .unwrap()
    .unwrap();
  let (status, _) = app
    .send(
      Method::DELETE,
      "/group",
      Some(&token),
      Some(json!({"uuid":admin_group.to_string()})),
    )
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// settings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn settings_user_get_and_save() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);

  let (status, body) = app
    .send(Method::GET, "/settings/user", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);
  // Client secret is never returned.
  assert!(body["settings"]["oidc_client_secret"].is_null());

  let (status, _) = app
    .send(
      Method::POST,
      "/settings/user",
      Some(&token),
      Some(json!({"oidc_enabled":false})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn settings_mail_get_and_save() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);

  let (status, body) = app
    .send(Method::GET, "/settings/mail", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::OK);
  assert!(body["settings"]["smtp_password"].is_null());

  // Saving disabled SMTP settings deactivates the mailer.
  let (status, _) = app
    .send(
      Method::POST,
      "/settings/mail",
      Some(&token),
      Some(json!({"smtp_enabled":false})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn settings_requires_permission() {
  let app = TestApp::new().await;
  let uid = app.local_user("plain", "pw").await;
  let token = app.token(uid);
  let (status, _) = app
    .send(Method::GET, "/settings/user", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// mail endpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mail_reset_endpoints_are_timing_safe() {
  let app = TestApp::new().await;
  // Both endpoints always return 200 regardless of input to avoid leaking
  // whether an account exists.
  let (status, _) = app
    .send(
      Method::POST,
      "/mail/reset/send",
      None,
      Some(json!({"email":"ghost@example.com"})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);

  let (status, _) = app
    .send(
      Method::POST,
      "/mail/reset/confirm",
      None,
      Some(json!({"token":"nope","new_password":"x"})),
    )
    .await;
  assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mail_test_requires_active_mailer() {
  let app = TestApp::new().await;
  let admin = app.admin_user("admin").await;
  let token = app.token(admin);
  // Mailer is inactive ⇒ sending fails with a 400 ("not configured").
  let (status, _) = app
    .send(Method::POST, "/mail/test", Some(&token), None)
    .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// websocket
// ---------------------------------------------------------------------------

#[tokio::test]
async fn websocket_requires_auth() {
  let app = TestApp::new().await;
  // The websocket route is auth-gated: without credentials the JWT extractor
  // rejects the request before any upgrade is attempted.
  let (status, _) = app.send(Method::GET, "/ws/updater", None, None).await;
  assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn websocket_delivers_targeted_update() {
  use futures_util::StreamExt;

  // A dedicated app served over a real TCP socket so a websocket client can
  // upgrade and receive pushed updates.
  let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
  Migrator::up(&*conn, None).await.unwrap();
  let jwt = JwtState::init(&AuthConfig::default(), &conn).await;
  let (update_state, updater) = UpdateState::<TestMsg>::init().await;

  let uid = conn
    .user()
    .create_user(
      "ws".into(),
      "ws@example.com".into(),
      "h".into(),
      SALT.into(),
      false,
    )
    .await
    .unwrap();
  let token = jwt.create_raw_token(uid).unwrap();

  let router = websocket::router::<TestMsg>()
    .layer(Extension(conn.clone()))
    .layer(Extension(jwt.clone()))
    .layer(Extension(JwtInvalidState::default()))
    .layer(Extension(update_state))
    .layer(Extension(updater.clone()));
  let app = router.finish_api(&mut aide::openapi::OpenApi::default());

  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();
  tokio::spawn(async move {
    axum::serve(listener, app).await.unwrap();
  });

  // The JWT is supplied via the query string, which the extractor accepts.
  let url = format!("ws://{addr}/updater?token={token}");
  let (mut ws, _resp) = tokio_tungstenite::connect_async(url).await.unwrap();

  // A targeted update to this user is delivered as a text frame.
  updater.send_to(uid, TestMsg::User { uuid: uid }).await;

  let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
    .await
    .expect("timed out waiting for update")
    .expect("stream ended")
    .expect("websocket error");
  assert!(msg.is_text());
  let text = msg.into_text().unwrap();
  // The frame carries the serialized update message for this user.
  assert!(text.contains(&uid.to_string()));
}

// ---------------------------------------------------------------------------
// build_router: the top-level composition (nesting under /api, CORS, logging,
// frontend proxy wiring, metrics bootstrap).
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TestConfig {
  base: crate::backend::config::BaseConfig,
  metrics: crate::backend::config::MetricsConfig,
  site: SiteConfig,
  auth: AuthConfig,
}

impl crate::backend::config::Config for TestConfig {
  fn base(&self) -> &crate::backend::config::BaseConfig {
    &self.base
  }
  fn metrics(&self) -> &crate::backend::config::MetricsConfig {
    &self.metrics
  }
  fn site(&self) -> &SiteConfig {
    &self.site
  }
  fn auth(&self) -> &AuthConfig {
    &self.auth
  }
}

#[tokio::test]
async fn build_router_serves_nested_health() {
  let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
  Migrator::up(&*conn, None).await.unwrap();

  let config = TestConfig {
    base: Default::default(),
    metrics: Default::default(), // metrics disabled ⇒ no server / middleware
    site: SiteConfig::default(),
    auth: AuthConfig::default(),
  };

  let app = backend::router::build_router(
    |_rl| ApiRouter::new(),
    |router: ApiRouter, _config: TestConfig| async move {
      // Provide just enough state for the health/nested router to build.
      router.layer(Extension(conn.clone()))
    },
    config,
  )
  .await;

  // Health is mounted under /api by build_router.
  let resp = app
    .clone()
    .oneshot(
      Request::builder()
        .uri("/api/health")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // The OpenAPI document is exposed.
  let resp = app
    .oneshot(
      Request::builder()
        .uri("/openapi.json")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn build_router_with_metrics_enabled_exposes_metrics() {
  let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
  Migrator::up(&*conn, None).await.unwrap();

  let mut metrics = crate::backend::config::MetricsConfig {
    metrics_enabled: true,
    metrics_name: "test".into(),
    ..Default::default()
  };
  // No dedicated port ⇒ the /metrics route is mounted on the main router and
  // the metrics middleware wraps every request.
  metrics.metrics_port = None;

  let config = TestConfig {
    base: Default::default(),
    metrics,
    site: SiteConfig::default(),
    auth: AuthConfig::default(),
  };

  let app = backend::router::build_router(
    |_rl| ApiRouter::new(),
    |router: ApiRouter, _config: TestConfig| async move { router.layer(Extension(conn.clone())) },
    config,
  )
  .await;

  // The request flows through the metrics middleware.
  let resp = app
    .clone()
    .oneshot(
      Request::builder()
        .uri("/api/health")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // The Prometheus scrape endpoint is mounted under /api.
  let resp = app
    .oneshot(
      Request::builder()
        .uri("/api/metrics")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
}
