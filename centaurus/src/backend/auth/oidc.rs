use std::{
  collections::HashMap,
  sync::Arc,
  time::{Duration, Instant},
};

use crate::{
  USER_AGENT,
  backend::{
    BackendRouter,
    auth::{
      jwt_state::JwtState,
      settings::{OidcSettings, UserSettings},
    },
    config::SiteConfig,
    endpoints::websocket::state::{UpdateMessage, Updater},
    middleware::rate_limiter::RateLimiter,
    request::redirect::Redirect,
  },
  bail,
  db::{init::Connection, tables::ConnectionExt},
  error::{ErrorReportStatusExt, Result},
  overwrite_with_env_config,
};
use aide::OperationIo;
use argon2::password_hash::generate_salt;
use axum::{
  Extension, Json,
  extract::{FromRequestParts, Query},
};
use axum_extra::extract::{CookieJar, cookie::Cookie};
use base64::prelude::*;
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use dashmap::DashMap;
use http::StatusCode;
use jsonwebtoken::{
  DecodingKey, Validation,
  jwk::{AlgorithmParameters, JwkSet},
};
use rand::seq::IndexedRandom;
use reqwest::{Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{spawn, sync::Mutex, time::sleep};
use tracing::{debug, info, warn};
use url::Url;
use uuid::Uuid;

pub const OIDC_STATE: &str = "oidc_state";
pub const SKIP_SETUP_ENV: &str = "SKIP_SETUP";
pub const URL_SAFE_CHARS: &[u8] =
  b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

pub fn router<T: UpdateMessage>(rate_limiter: &mut RateLimiter) -> BackendRouter {
  #[cfg(feature = "openapi")]
  use aide::axum::routing::get;
  #[cfg(not(feature = "openapi"))]
  use axum::routing::get;

  BackendRouter::new()
    .route("/url", get(oidc_url))
    .layer(rate_limiter.create_limiter())
    .route("/callback", get(oidc_callback::<T>))
}

type State = (Instant, Option<String>, Option<String>);

#[derive(Clone, FromRequestParts, Debug, OperationIo)]
#[from_request(via(Extension))]
pub struct OidcState {
  config: Arc<Mutex<Option<OidcConfig>>>,
  state: Arc<DashMap<Uuid, State>>,
  nonce: Arc<DashMap<Uuid, Instant>>,
}

#[derive(Debug, Clone)]
struct OidcConfig {
  issuer: String,
  authorization_endpoint: Url,
  token_endpoint: Url,
  userinfo_endpoint: Url,
  jwk_set: JwkSet,
  client_id: String,
  client_secret: String,
  client: Client,
  scope: Vec<String>,
  group_sync: bool,
  group_claim: String,
  #[allow(unused)]
  image_sync: bool,
  create_user: bool,
  pkce: bool,
}

#[derive(Deserialize, Debug)]
struct OidcConfiguration {
  issuer: String,
  authorization_endpoint: Url,
  token_endpoint: Url,
  userinfo_endpoint: Url,
  jwks_uri: Url,
}

impl OidcState {
  pub async fn new(db: &Connection, oidc: Option<&UserSettings>) -> Self {
    let state = Self {
      config: Arc::new(Mutex::new(None)),
      state: Arc::new(DashMap::new()),
      nonce: Arc::new(DashMap::new()),
    };

    let mut settings: UserSettings = db.settings().get_settings().await.unwrap_or_default();
    let mut db_settings = settings.clone();

    overwrite_with_env_config!(
      settings,
      oidc,
      oidc_issuer,
      oidc_client_id,
      oidc_client_secret,
      oidc_scopes,,
      oidc_enabled,
      oidc_group_sync,
      oidc_image_sync,
      oidc_pkce,
      sso_create_user,
      sso_instant_redirect
    );

    let is_setup = db.setup().is_setup().await.unwrap_or(false);
    let skip_setup = std::env::var(SKIP_SETUP_ENV)
      .map(|v| v == "true")
      .unwrap_or(false);

    if let Some(oidc_settings) = &settings.oidc_settings() {
      spawn({
        let state = state.clone();
        let mut oidc_settings = oidc_settings.clone();
        let db = db.clone();

        async move {
          if skip_setup && !is_setup {
            info!("Trying to init oidc and skip setup");

            if !oidc_settings.create_user {
              info!("Enabling sso user creation to make setup skip work");

              oidc_settings.create_user = true;
              db_settings.sso_create_user = Some(true);

              db.settings()
                .save_settings(&db_settings)
                .await
                .unwrap_or_else(|e| {
                  warn!("Failed to save settings: {:?}", e);
                });
            }
          }

          if let Err(e) = state.try_init(&oidc_settings).await {
            if skip_setup && !is_setup {
              info!(
                "Failed to init Oidc. Setup will not be skipped. Error: {:?}",
                e
              );
            } else {
              warn!("Failed to initialize OIDC: {:?}", e);
            }
          } else if skip_setup && !is_setup {
            info!("Oidc initialized successfully, setup will be skipped");
            db.setup().mark_completed().await.unwrap_or_else(|e| {
              warn!("Failed to mark setup as completed: {:?}", e);
            });
          } else {
            info!("Oidc initialized successfully");
          }
        }
      });
    } else if skip_setup && !is_setup {
      info!("Could not skip setup, OIDC is not configured");
    }

    spawn({
      let state = state.clone();
      async move {
        let cleanup_interval = Duration::from_secs(600);
        let expiration_duration = Duration::from_secs(600);
        loop {
          sleep(cleanup_interval).await;
          let now = Instant::now();
          state
            .nonce
            .retain(|_, &mut instant| now.duration_since(instant) < expiration_duration);
          state
            .state
            .retain(|_, &mut (instant, _, _)| now.duration_since(instant) < expiration_duration);
        }
      }
    });

    state
  }

  pub async fn try_init(&self, settings: &OidcSettings) -> Result<()> {
    let config = OidcConfig::new(settings).await?;
    let mut lock = self.config.lock().await;
    *lock = Some(config);
    Ok(())
  }

  pub async fn deactivate(&self) {
    let mut lock = self.config.lock().await;
    *lock = None;
  }

  pub async fn is_enabled(&self) -> bool {
    let lock = self.config.lock().await;
    lock.is_some()
  }
}

impl OidcConfig {
  async fn new(oidc_settings: &OidcSettings) -> Result<Self> {
    let mut url = oidc_settings.issuer.clone();
    url
      .path_segments_mut()
      .ok()
      .status_context(StatusCode::BAD_REQUEST, "Failed to add path to url")?
      .pop_if_empty()
      .push(".well-known")
      .push("openid-configuration");

    info!("Configuring OIDC with URL: {}", url);
    let res = reqwest::get(url.clone()).await?;
    if !res.status().is_success() {
      let body = res.text().await.unwrap_or_default();
      bail!(
        "Failed to retrieve OIDC configuration from {}: {}",
        url,
        body
      );
    }
    let config: OidcConfiguration = res.json().await?;

    info!("Retrieving JWKs from: {}", config.jwks_uri);
    let res = reqwest::get(config.jwks_uri.clone()).await?;
    if !res.status().is_success() {
      let body = res.text().await.unwrap_or_default();
      bail!("Failed to retrieve JWKs from {}: {}", config.jwks_uri, body);
    }
    let jwk_set: JwkSet = res.json().await?;

    let client = Client::builder()
      .redirect(Policy::none())
      .user_agent(USER_AGENT)
      .build()?;
    info!(
      "OIDC configured successfully with issuer: {}",
      config.issuer
    );

    Ok(Self {
      issuer: config.issuer,
      authorization_endpoint: config.authorization_endpoint,
      token_endpoint: config.token_endpoint,
      userinfo_endpoint: config.userinfo_endpoint,
      jwk_set,
      client_id: oidc_settings.client_id.clone(),
      client_secret: oidc_settings.client_secret.clone(),
      client,
      scope: oidc_settings.scopes.clone(),
      group_claim: oidc_settings.group_claim.clone(),
      group_sync: oidc_settings.group_sync,
      image_sync: oidc_settings.image_sync,
      create_user: oidc_settings.create_user,
      pkce: oidc_settings.pkce,
    })
  }
}

impl OidcConfig {
  async fn validate_jwk(&self, token: &str, nonce_map: &DashMap<Uuid, Instant>) -> Result<()> {
    let header = jsonwebtoken::decode_header(token)?;

    let Some(kid) = header.kid else {
      bail!(INTERNAL_SERVER_ERROR, "Missing kid in JWK header");
    };

    let Some(jwk) = self.jwk_set.find(&kid) else {
      bail!(INTERNAL_SERVER_ERROR, "JWK not found");
    };

    let decoding_key = match &jwk.algorithm {
      AlgorithmParameters::RSA(rsa) => DecodingKey::from_rsa_components(&rsa.n, &rsa.e)
        .status(StatusCode::INTERNAL_SERVER_ERROR)?,
      _ => {
        bail!(INTERNAL_SERVER_ERROR, "Unsupported JWK algorithm");
      }
    };

    let validation = {
      let mut validation = Validation::new(header.alg);
      validation.set_audience(std::slice::from_ref(&self.client_id));
      validation.set_issuer(&[&self.issuer]);
      validation.validate_exp = false;
      validation
    };

    let data = jsonwebtoken::decode::<HashMap<String, serde_json::Value>>(
      token,
      &decoding_key,
      &validation,
    )?;

    let Some(Some(Ok(nonce))) = data
      .claims
      .get("nonce")
      .map(|nonce| nonce.as_str().map(|nonce| nonce.parse()))
    else {
      bail!(INTERNAL_SERVER_ERROR, "Missing nonce in JWK claims");
    };
    if nonce_map.remove(&nonce).is_none() {
      bail!(INTERNAL_SERVER_ERROR, "Invalid nonce");
    }

    Ok(())
  }
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
struct OidcResponse {
  url: String,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
struct OidcUrlQuery {
  redirect_to: Option<String>,
}

async fn oidc_url(
  state: OidcState,
  jwt: JwtState,
  mut cookies: CookieJar,
  Query(OidcUrlQuery { mut redirect_to }): Query<OidcUrlQuery>,
) -> Result<(CookieJar, Json<OidcResponse>)> {
  let lock = state.config.lock().await;
  let Some(config) = lock.clone() else {
    bail!(BAD_REQUEST, "OIDC not configured");
  };
  drop(lock);

  let state_id = Uuid::new_v4();
  let nonce = Uuid::new_v4();

  let mut form = HashMap::new();
  form.insert("response_type", "code".to_string());
  form.insert("client_id", config.client_id.clone());
  form.insert("state", state_id.to_string());
  form.insert("nonce", nonce.to_string());

  let code_verifier = if config.pkce {
    let code_verifier: String = {
      let mut rng = rand::rng();
      (0..64)
        .map(|_| *URL_SAFE_CHARS.choose(&mut rng).unwrap() as char)
        .collect()
    };

    let code_challenge = {
      let ascii_bytes = code_verifier.as_bytes();

      let mut hasher = Sha256::new();
      hasher.update(ascii_bytes);
      BASE64_URL_SAFE_NO_PAD.encode(hasher.finalize())
    };
    form.insert("code_challenge", code_challenge);
    form.insert("code_challenge_method", "S256".to_string());

    Some(code_verifier)
  } else {
    None
  };

  if !config.scope.is_empty() {
    form.insert("scope", config.scope.join(" "));
  }

  if let Some(redirect_to) = &mut redirect_to {
    let dummy_base = Url::parse("http://localhost").unwrap();
    match dummy_base.join(redirect_to) {
      Ok(redirect_url) => {
        if redirect_url.host_str() != Some("localhost") {
          bail!("Invalid redirect url");
        }

        let path = redirect_url.path();
        let query = redirect_url
          .query()
          .map(|q| format!("?{}", q))
          .unwrap_or_default();
        let fragment = redirect_url
          .fragment()
          .map(|f| format!("#{}", f))
          .unwrap_or_default();
        *redirect_to = format!("{}{}{}", path, query, fragment);
      }
      Err(_) => {
        bail!("Invalid redirect url");
      }
    }
  }

  state
    .state
    .insert(state_id, (Instant::now(), code_verifier, redirect_to));
  cookies = cookies.add(jwt.create_cookie(OIDC_STATE, state_id.to_string()));

  state.nonce.insert(nonce, Instant::now());

  let mut url = config.authorization_endpoint.clone();
  url.query_pairs_mut().extend_pairs(&form);

  Ok((
    cookies,
    Json(OidcResponse {
      url: url.to_string(),
    }),
  ))
}

#[derive(Deserialize, Clone)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
struct OidcCallbackQuery {
  code: Option<String>,
  state: Option<Uuid>,
  error: Option<String>,
}

#[derive(Deserialize)]
struct TokenRes {
  id_token: String,
}

#[derive(Deserialize)]
pub struct AuthInfo {
  pub sub: String,
  pub email: String,
  pub name: String,
  pub picture: Option<String>,
  #[serde(flatten)]
  pub extra: HashMap<String, serde_json::Value>,
}

async fn oidc_callback<T: UpdateMessage>(
  Query(OidcCallbackQuery { code, state, error }): Query<OidcCallbackQuery>,
  oidc_state: OidcState,
  cookies: CookieJar,
  db: Connection,
  oidc_config: SiteConfig,
  jwt: JwtState,
  updater: Updater<T>,
) -> Result<(CookieJar, Redirect)> {
  let (path, error, mut cookies) = check_code(
    error,
    state,
    code,
    &db,
    cookies,
    &jwt,
    &oidc_state.nonce,
    updater,
    &oidc_state,
  )
  .await?;

  cookies = cookies.remove(Cookie::from(OIDC_STATE));

  // `path` is a sanitized same-origin ref (/path?query#frag). Re-split it and
  // apply onto the trusted site_url via host-safe setters; set_path alone would
  // percent-encode the '?' and swallow the query/fragment.
  let mut url = oidc_config.site_url;
  let target = Url::parse("http://localhost")
    .unwrap()
    .join(&path)
    .unwrap_or_else(|_| Url::parse("http://localhost/").unwrap());
  url.set_path(target.path());
  url.set_fragment(target.fragment());

  let mut query = target.query().map(str::to_string).unwrap_or_default();
  if let Some(e) = error {
    if !query.is_empty() {
      query.push('&');
    }
    query.push_str("error=");
    query.push_str(&e);
  }
  url.set_query((!query.is_empty()).then_some(query.as_str()));

  Ok((cookies, Redirect::found(url.to_string())))
}

#[derive(Deserialize)]
struct TokenError {
  error: String,
}

#[allow(clippy::too_many_arguments)]
async fn check_code<T: UpdateMessage>(
  error: Option<String>,
  state: Option<Uuid>,
  code: Option<String>,
  db: &Connection,
  mut cookies: CookieJar,
  jwt: &JwtState,
  nonce_map: &DashMap<Uuid, Instant>,
  updater: Updater<T>,
  oidc_state: &OidcState,
) -> Result<(String, Option<String>, CookieJar)> {
  let lock = oidc_state.config.lock().await;
  let Some(config) = lock.clone() else {
    return Ok((
      "/login".to_string(),
      Some("oidc_not_configured".to_string()),
      cookies,
    ));
  };
  drop(lock);

  if let Some(error) = error {
    return Ok(("/login".to_string(), Some(error), cookies));
  }

  let Some(state) = state else {
    return Ok((
      "/login".to_string(),
      Some("invalid_state".to_string()),
      cookies,
    ));
  };

  let Some((_, (_, code_verifier, redirect_to))) = oidc_state.state.remove(&state) else {
    return Ok((
      "/login".to_string(),
      Some("invalid_state".to_string()),
      cookies,
    ));
  };
  let Some(cookie) = cookies.get(OIDC_STATE) else {
    return Ok((
      "/login".to_string(),
      Some("invalid_state".to_string()),
      cookies,
    ));
  };
  if cookie.value() != state.to_string() {
    return Ok((
      "/login".to_string(),
      Some("invalid_state".to_string()),
      cookies,
    ));
  }

  let Some(code) = code else {
    return Ok((
      "/login".to_string(),
      Some("missing_code".to_string()),
      cookies,
    ));
  };

  let mut form = HashMap::new();
  form.insert("grant_type", "authorization_code".to_string());
  form.insert("code", code);

  if let Some(code_verifier) = code_verifier {
    form.insert("code_verifier", code_verifier);
  }

  let req = config
    .client
    .post(config.token_endpoint.clone())
    .basic_auth(config.client_id.clone(), Some(config.client_secret.clone()))
    .form(&form)
    .build()?;

  let res = config.client.execute(req).await?;
  if !res.status().is_success() {
    let body = res.text().await.unwrap_or_default();
    let Ok(error) = serde_json::from_str::<TokenError>(&body) else {
      tracing::error!("OIDC token request failed: {}", body);
      return Ok((
        "/login".to_string(),
        Some("invalid_code".to_string()),
        cookies,
      ));
    };
    tracing::error!("OIDC token request failed: {}", error.error);
    return Ok(("/login".to_string(), Some(error.error), cookies));
  }

  let res: TokenRes = res.json().await?;
  config.validate_jwk(&res.id_token, nonce_map).await?;
  let token = res.id_token.clone();

  let req = config
    .client
    .get(config.userinfo_endpoint.clone())
    .bearer_auth(&token)
    .build()?;

  let res = config.client.execute(req).await?;
  if !res.status().is_success() {
    let body = res.text().await.unwrap_or_default();
    tracing::error!("OIDC userinfo request failed: {}", body);
    return Ok((
      "/login".to_string(),
      Some("invalid_token".to_string()),
      cookies,
    ));
  }
  let res: AuthInfo = res.json().await?;

  let redirect_to = redirect_to.unwrap_or("/".to_string());

  if let Some(user) = db.user().resolve_oidc_user(&res.sub, &res.email).await? {
    sync_oidc_user(user.id, &res, &config, db, token, updater).await?;

    debug!("OIDC user authenticated: {}", user.id);
    cookies = cookies.add(jwt.create_token(user.id)?);

    return Ok((redirect_to, None, cookies));
  }

  if !config.create_user {
    return Ok((
      "/login".to_string(),
      Some("user_not_found".to_string()),
      cookies,
    ));
  }

  let user = db
    .user()
    .create_user(
      res.name.clone(),
      res.email.clone(),
      String::new(),
      BASE64_STANDARD_NO_PAD.encode(generate_salt()),
      true,
      Some(res.sub.clone()),
    )
    .await?;
  sync_oidc_user(user, &res, &config, db, token, updater).await?;

  if !db.setup().is_setup().await? || db.user().count_users().await? == 1 {
    let Some(admin_group_id) = db.setup().get_admin_group_id().await? else {
      bail!(
        INTERNAL_SERVER_ERROR,
        "Admin group has not been created yet, cannot create initial user"
      );
    };

    let users = db.group().get_group_users_ids(admin_group_id).await?;

    // oidc group sync could have already added the user to the admin group, so only add if not already present
    if !users.contains(&user) {
      db.group()
        .add_user_to_groups(user, vec![admin_group_id])
        .await?;
    }

    db.setup().mark_completed().await?;
    info!("Setup completed via OIDC, created user with ID {}", user);
  }

  debug!("OIDC user authenticated: {}", user);
  cookies = cookies.add(jwt.create_token(user)?);

  Ok((redirect_to, None, cookies))
}

impl AuthInfo {
  pub fn groups(&self, group_claim: &str) -> Vec<String> {
    if let Some(groups) = self.extra.get(group_claim) {
      if let Some(groups) = groups.as_array() {
        return groups
          .iter()
          .filter_map(|g| g.as_str().map(|s| s.to_string()))
          .collect();
      } else if let Some(group) = groups.as_str() {
        return vec![group.to_string()];
      }
    }
    Vec::new()
  }
}

async fn sync_oidc_user<T: UpdateMessage>(
  user_id: Uuid,
  auth: &AuthInfo,
  config: &OidcConfig,
  db: &Connection,
  #[allow(unused)] token: String,
  updater: Updater<T>,
) -> Result<()> {
  if db
    .user()
    .sync_from_oidc(user_id, &auth.name, &auth.email)
    .await?
  {
    updater.send_to(user_id, T::user(user_id)).await;
  }

  sync_groups(user_id, auth, config, db, updater.clone()).await?;

  #[cfg(feature = "avatar")]
  if config.image_sync {
    sync_image(
      user_id,
      auth.picture.clone(),
      db.clone(),
      token,
      config.client.clone(),
      updater,
    )
    .await;
  }

  Ok(())
}

async fn sync_groups<T: UpdateMessage>(
  user: Uuid,
  auth: &AuthInfo,
  config: &OidcConfig,
  db: &Connection,
  updater: Updater<T>,
) -> Result<()> {
  if !config.group_sync {
    return Ok(());
  }

  let groups = auth.groups(&config.group_claim);
  let mut group_ids = db.group().group_ids(&groups).await?;

  let Some(admin_group) = db.setup().get_admin_group_id().await? else {
    return Ok(());
  };

  if db.group().is_last_admin(admin_group, user).await? && !group_ids.contains(&admin_group) {
    group_ids.push(admin_group);
  }

  db.user().clear_user_groups(user).await?;
  db.group().add_user_to_groups(user, group_ids).await?;

  updater.send_to(user, T::user(user)).await;
  updater.send_to(user, T::user_permissions()).await;

  Ok(())
}

#[cfg(feature = "avatar")]
async fn sync_image<T: UpdateMessage>(
  user: Uuid,
  picture: Option<String>,
  db: Connection,
  id_token: String,
  client: Client,
  updater: Updater<T>,
) {
  let Some(picture) = picture else {
    return;
  };

  spawn(async move {
    let req = client.get(picture).bearer_auth(id_token).build()?;
    let res = client.execute(req).await?;
    if !res.status().is_success() {
      return Result::Ok(());
    }

    let bytes = res.bytes().await?;
    db.user().update_user_avatar(user, bytes.to_vec()).await?;

    updater.send_to(user, T::user(user)).await;

    Ok(())
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::backend::auth::settings::AuthConfig;
  use crate::backend::endpoints::websocket::state::{UpdateMessage, UpdateState, Updater};
  use crate::db::{config::DBConfig, init::connect_db, migrations::Migrator};
  use axum::response::IntoResponse;
  use axum::routing::{get, post};
  use http::header::LOCATION;
  use rsa::rand_core::OsRng;
  use sea_orm_migration::MigratorTrait;
  use serde::{Deserialize, Serialize};
  use serde_json::json;
  use std::sync::{Arc, Mutex};

  #[derive(Serialize, Deserialize, Clone, Debug)]
  enum Msg {
    A,
  }
  impl UpdateMessage for Msg {
    fn settings() -> Self {
      Msg::A
    }
    fn group(_: Uuid) -> Self {
      Msg::A
    }
    fn user(_: Uuid) -> Self {
      Msg::A
    }
    fn user_permissions() -> Self {
      Msg::A
    }
  }

  /// A minimal mock OIDC provider: discovery + JWKS + a redirecting authorize
  /// endpoint. Returns its base URL.
  async fn mock_idp() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    let disco_base = base.clone();
    let app = axum::Router::new()
      .route(
        "/.well-known/openid-configuration",
        get(move || {
          let base = disco_base.clone();
          async move {
            axum::Json(json!({
              "issuer": base,
              "authorization_endpoint": format!("{base}/authorize"),
              "token_endpoint": format!("{base}/token"),
              "userinfo_endpoint": format!("{base}/userinfo"),
              "jwks_uri": format!("{base}/jwks"),
            }))
          }
        }),
      )
      .route(
        "/jwks",
        get(|| async {
          axum::Json(json!({"keys":[{"kty":"RSA","kid":"test","alg":"RS256","use":"sig","n":"AQAB","e":"AQAB"}]}))
        }),
      )
      .route(
        "/authorize",
        post(|| async { (StatusCode::FOUND, [(LOCATION, "https://idp.example/redirect")]) }),
      );

    tokio::spawn(async move {
      axum::serve(listener, app).await.unwrap();
    });

    base
  }

  fn settings(issuer: &str, pkce: bool) -> OidcSettings {
    OidcSettings {
      issuer: Url::parse(issuer).unwrap(),
      client_id: "client".into(),
      client_secret: "secret".into(),
      scopes: vec!["openid".into(), "profile".into()],
      group_sync: false,
      group_claim: "groups".into(),
      pkce,
      image_sync: false,
      create_user: false,
    }
  }

  async fn db() -> Connection {
    let conn = connect_db(&DBConfig::default(), "sqlite::memory:").await;
    Migrator::up(&*conn, None).await.unwrap();
    conn
  }

  struct SigningIdp {
    base: String,
    token_slot: Arc<Mutex<String>>,
    enc_key: jsonwebtoken::EncodingKey,
  }

  async fn signing_idp(userinfo: serde_json::Value) -> SigningIdp {
    use jsonwebtoken::EncodingKey;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::pkcs8::LineEnding;
    use rsa::traits::PublicKeyParts;
    use rsa::{RsaPrivateKey, RsaPublicKey};

    let priv_key = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);
    let n = BASE64_URL_SAFE_NO_PAD.encode(pub_key.n().to_bytes_be());
    let e = BASE64_URL_SAFE_NO_PAD.encode(pub_key.e().to_bytes_be());
    let enc_key =
      EncodingKey::from_rsa_pem(priv_key.to_pkcs1_pem(LineEnding::LF).unwrap().as_bytes()).unwrap();
    let token_slot = Arc::new(Mutex::new(String::new()));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let disco_base = base.clone();
    let jwk = json!({"keys":[{"kty":"RSA","kid":"test","alg":"RS256","use":"sig","n":n,"e":e}]});
    let slot = token_slot.clone();
    let app = axum::Router::new()
      .route(
        "/.well-known/openid-configuration",
        get(move || {
          let base = disco_base.clone();
          async move {
            axum::Json(json!({
              "issuer": base,
              "authorization_endpoint": format!("{base}/authorize"),
              "token_endpoint": format!("{base}/token"),
              "userinfo_endpoint": format!("{base}/userinfo"),
              "jwks_uri": format!("{base}/jwks"),
            }))
          }
        }),
      )
      .route(
        "/jwks",
        get(move || {
          let jwk = jwk.clone();
          async move { axum::Json(jwk) }
        }),
      )
      .route(
        "/authorize",
        post(|| async {
          (
            StatusCode::FOUND,
            [(LOCATION, "https://idp.example/redirect")],
          )
        }),
      )
      .route(
        "/token",
        post(move || {
          let slot = slot.clone();
          async move { axum::Json(json!({"id_token": slot.lock().unwrap().clone()})) }
        }),
      )
      .route(
        "/userinfo",
        get(move || {
          let userinfo = userinfo.clone();
          async move { axum::Json(userinfo) }
        }),
      );
    tokio::spawn(async move {
      axum::serve(listener, app).await.unwrap();
    });

    SigningIdp {
      base,
      token_slot,
      enc_key,
    }
  }

  async fn run_oidc_callback(
    conn: &Connection,
    idp: &SigningIdp,
    create_user: bool,
    id_token_sub: &str,
    redirect_to: Option<&str>,
  ) -> String {
    use jsonwebtoken::{Algorithm, Header, encode};

    let oidc_settings = OidcSettings {
      issuer: Url::parse(&idp.base).unwrap(),
      client_id: "client".into(),
      client_secret: "secret".into(),
      scopes: vec!["openid".into()],
      group_sync: false,
      group_claim: "groups".into(),
      pkce: false,
      image_sync: false,
      create_user,
    };

    let state = OidcState::new(conn, None).await;
    state.try_init(&oidc_settings).await.unwrap();
    let jwt = JwtState::init(&AuthConfig::default(), conn).await;
    let (cookies, _) = oidc_url(
      state.clone(),
      jwt.clone(),
      CookieJar::new(),
      Query(OidcUrlQuery {
        redirect_to: redirect_to.map(str::to_string),
      }),
    )
    .await
    .unwrap();
    let state_id = *state.state.iter().next().unwrap().key();
    let nonce = *state.nonce.iter().next().unwrap().key();

    let exp = chrono::Utc::now().timestamp() + 3600;
    let claims = json!({
      "iss": idp.base,
      "aud": "client",
      "sub": id_token_sub,
      "nonce": nonce.to_string(),
      "exp": exp,
    });
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test".into());
    *idp.token_slot.lock().unwrap() = encode(&header, &claims, &idp.enc_key).unwrap();

    let updater: Updater<Msg> = UpdateState::<Msg>::init().await.1;
    let out = oidc_callback::<Msg>(
      axum::extract::Query(OidcCallbackQuery {
        code: Some("auth-code".into()),
        state: Some(state_id),
        error: None,
      }),
      state,
      cookies,
      conn.clone(),
      SiteConfig::default(),
      jwt,
      updater,
    )
    .await
    .unwrap();

    out
      .into_response()
      .headers()
      .get(LOCATION)
      .unwrap()
      .to_str()
      .unwrap()
      .to_string()
  }

  #[tokio::test]
  async fn test_try_init_is_enabled_deactivate() {
    let base = mock_idp().await;
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;

    assert!(!state.is_enabled().await);
    state.try_init(&settings(&base, true)).await.unwrap();
    assert!(state.is_enabled().await);
    state.deactivate().await;
    assert!(!state.is_enabled().await);
  }

  #[tokio::test]
  async fn test_try_init_unreachable_issuer_errors() {
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    // Nothing listens on port 9 ⇒ discovery fails ⇒ state stays disabled.
    assert!(
      state
        .try_init(&settings("http://127.0.0.1:9", false))
        .await
        .is_err()
    );
    assert!(!state.is_enabled().await);
  }

  #[tokio::test]
  async fn test_oidc_url_builds_redirect_with_pkce() {
    let base = mock_idp().await;
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    state.try_init(&settings(&base, true)).await.unwrap();

    let jwt = JwtState::init(&AuthConfig::default(), &conn).await;
    let (cookies, axum::Json(resp)) = oidc_url(
      state.clone(),
      jwt,
      CookieJar::new(),
      Query(OidcUrlQuery { redirect_to: None }),
    )
    .await
    .unwrap();

    // The authorize URL is built directly from the endpoint + form params
    // (fast track, no round-trip to the provider) and the state cookie is set.
    let url = Url::parse(&resp.url).unwrap();
    assert_eq!(url.path(), "/authorize");
    let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(q.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(q.get("client_id").map(String::as_str), Some("client"));
    let state_id = *state.state.iter().next().unwrap().key();
    let nonce = *state.nonce.iter().next().unwrap().key();
    assert_eq!(q.get("state"), Some(&state_id.to_string()));
    assert_eq!(q.get("nonce"), Some(&nonce.to_string()));
    // pkce=true ⇒ challenge params present.
    assert!(q.contains_key("code_challenge"));
    assert_eq!(
      q.get("code_challenge_method").map(String::as_str),
      Some("S256")
    );
    assert_eq!(q.get("scope").map(String::as_str), Some("openid profile"));
    assert!(cookies.get(OIDC_STATE).is_some());
    // A pending state + nonce were registered for the later callback.
    assert_eq!(state.state.len(), 1);
    assert_eq!(state.nonce.len(), 1);
  }

  #[tokio::test]
  async fn test_oidc_url_no_pkce_omits_challenge() {
    let base = mock_idp().await;
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    state.try_init(&settings(&base, false)).await.unwrap();

    let jwt = JwtState::init(&AuthConfig::default(), &conn).await;
    let (_, axum::Json(resp)) = oidc_url(
      state.clone(),
      jwt,
      CookieJar::new(),
      Query(OidcUrlQuery { redirect_to: None }),
    )
    .await
    .unwrap();

    let url = Url::parse(&resp.url).unwrap();
    let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert!(!q.contains_key("code_challenge"));
    assert!(!q.contains_key("code_challenge_method"));
    // The stored verifier is None when pkce is disabled.
    assert!(state.state.iter().next().unwrap().value().1.is_none());
  }

  #[tokio::test]
  async fn test_oidc_url_unconfigured_is_bad_request() {
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    let jwt = JwtState::init(&AuthConfig::default(), &conn).await;
    let err = match oidc_url(
      state,
      jwt,
      CookieJar::new(),
      Query(OidcUrlQuery { redirect_to: None }),
    )
    .await
    {
      Err(e) => e,
      Ok(_) => panic!("expected error when OIDC is unconfigured"),
    };
    assert_eq!(err.status, StatusCode::BAD_REQUEST);
  }

  // Runs the redirect_to value through oidc_url's sanitizer and returns the
  // value that was stored for the later callback (Ok), or the bail (Err).
  async fn sanitize_redirect(redirect_to: Option<&str>) -> Result<Option<String>> {
    let base = mock_idp().await;
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    state.try_init(&settings(&base, true)).await.unwrap();
    let jwt = JwtState::init(&AuthConfig::default(), &conn).await;

    let _ = oidc_url(
      state.clone(),
      jwt,
      CookieJar::new(),
      Query(OidcUrlQuery {
        redirect_to: redirect_to.map(str::to_string),
      }),
    )
    .await?;

    Ok(state.state.iter().next().unwrap().value().2.clone())
  }

  #[tokio::test]
  async fn test_sanitize_redirect_none_stays_none() {
    assert_eq!(sanitize_redirect(None).await.unwrap(), None);
  }

  #[tokio::test]
  async fn test_sanitize_redirect_keeps_relative_path() {
    assert_eq!(
      sanitize_redirect(Some("/dashboard")).await.unwrap(),
      Some("/dashboard".to_string())
    );
  }

  #[tokio::test]
  async fn test_sanitize_redirect_keeps_path_and_query() {
    assert_eq!(
      sanitize_redirect(Some("/dashboard?a=1&b=2")).await.unwrap(),
      Some("/dashboard?a=1&b=2".to_string())
    );
  }

  #[tokio::test]
  async fn test_sanitize_redirect_normalizes_dot_segments() {
    assert_eq!(
      sanitize_redirect(Some("/a/../b")).await.unwrap(),
      Some("/b".to_string())
    );
  }

  // An absolute URL pointing at localhost is reduced to just its path+query.
  #[tokio::test]
  async fn test_sanitize_redirect_strips_localhost_origin() {
    assert_eq!(
      sanitize_redirect(Some("http://localhost/foo?x=1"))
        .await
        .unwrap(),
      Some("/foo?x=1".to_string())
    );
  }

  #[tokio::test]
  async fn test_sanitize_redirect_rejects_absolute_other_host() {
    assert!(
      sanitize_redirect(Some("https://evil.com/phish"))
        .await
        .is_err()
    );
  }

  #[tokio::test]
  async fn test_sanitize_redirect_rejects_protocol_relative() {
    assert!(sanitize_redirect(Some("//evil.com")).await.is_err());
  }

  #[tokio::test]
  async fn test_sanitize_redirect_rejects_userinfo_host_confusion() {
    assert!(
      sanitize_redirect(Some("http://localhost@evil.com"))
        .await
        .is_err()
    );
  }

  // url crate follows WHATWG and treats backslashes as slashes for special
  // schemes, so these resolve to evil.com as host and are rejected.
  #[tokio::test]
  async fn test_sanitize_redirect_rejects_backslash_tricks() {
    assert!(sanitize_redirect(Some("/\\evil.com")).await.is_err());
    assert!(sanitize_redirect(Some("\\/\\/evil.com")).await.is_err());
  }

  // A non-http scheme like javascript: parses to no host and is rejected.
  #[tokio::test]
  async fn test_sanitize_redirect_rejects_other_scheme() {
    assert!(
      sanitize_redirect(Some("javascript:alert(1)"))
        .await
        .is_err()
    );
  }

  #[tokio::test]
  async fn test_sanitize_redirect_keeps_fragment() {
    assert_eq!(
      sanitize_redirect(Some("/dashboard#section")).await.unwrap(),
      Some("/dashboard#section".to_string())
    );
    assert_eq!(
      sanitize_redirect(Some("/dashboard?a=1#section"))
        .await
        .unwrap(),
      Some("/dashboard?a=1#section".to_string())
    );
  }

  async fn callback_location(
    state: OidcState,
    query: OidcCallbackQuery,
    cookies: CookieJar,
    conn: &Connection,
  ) -> String {
    let jwt = JwtState::init(&AuthConfig::default(), conn).await;
    let updater: Updater<Msg> = UpdateState::<Msg>::init().await.1;
    let out = oidc_callback::<Msg>(
      axum::extract::Query(query),
      state,
      cookies,
      conn.clone(),
      SiteConfig::default(),
      jwt,
      updater,
    )
    .await
    .unwrap();
    let resp = out.into_response();
    resp
      .headers()
      .get(LOCATION)
      .unwrap()
      .to_str()
      .unwrap()
      .to_string()
  }

  #[tokio::test]
  async fn test_callback_unconfigured_redirects_with_error() {
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    let loc = callback_location(
      state,
      OidcCallbackQuery {
        code: None,
        state: None,
        error: None,
      },
      CookieJar::new(),
      &conn,
    )
    .await;
    assert!(loc.contains("error=oidc_not_configured"));
  }

  #[tokio::test]
  async fn test_callback_provider_error_is_propagated() {
    let base = mock_idp().await;
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    state.try_init(&settings(&base, false)).await.unwrap();

    let loc = callback_location(
      state,
      OidcCallbackQuery {
        code: None,
        state: None,
        error: Some("access_denied".into()),
      },
      CookieJar::new(),
      &conn,
    )
    .await;
    assert!(loc.contains("error=access_denied"));
  }

  #[tokio::test]
  async fn test_callback_missing_state_is_invalid() {
    let base = mock_idp().await;
    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    state.try_init(&settings(&base, false)).await.unwrap();

    let loc = callback_location(
      state,
      OidcCallbackQuery {
        code: Some("abc".into()),
        state: None,
        error: None,
      },
      CookieJar::new(),
      &conn,
    )
    .await;
    assert!(loc.contains("error=invalid_state"));
  }

  #[tokio::test]
  async fn test_callback_full_login_creates_user() {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::pkcs8::LineEnding;
    use rsa::traits::PublicKeyParts;
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use std::sync::{Arc, Mutex};

    // Generate the IdP signing key and expose its public part as a JWK.
    let priv_key = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);
    let n = BASE64_URL_SAFE_NO_PAD.encode(pub_key.n().to_bytes_be());
    let e = BASE64_URL_SAFE_NO_PAD.encode(pub_key.e().to_bytes_be());
    let enc_key =
      EncodingKey::from_rsa_pem(priv_key.to_pkcs1_pem(LineEnding::LF).unwrap().as_bytes()).unwrap();

    // Shared slot so the /token endpoint can return an id_token we build *after*
    // learning the nonce from oidc_url.
    let token_slot = Arc::new(Mutex::new(String::new()));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let disco_base = base.clone();
    let jwk = json!({"keys":[{"kty":"RSA","kid":"test","alg":"RS256","use":"sig","n":n,"e":e}]});
    let slot = token_slot.clone();
    let app = axum::Router::new()
      .route(
        "/.well-known/openid-configuration",
        get(move || {
          let base = disco_base.clone();
          async move {
            axum::Json(json!({
              "issuer": base,
              "authorization_endpoint": format!("{base}/authorize"),
              "token_endpoint": format!("{base}/token"),
              "userinfo_endpoint": format!("{base}/userinfo"),
              "jwks_uri": format!("{base}/jwks"),
            }))
          }
        }),
      )
      .route(
        "/jwks",
        get(move || {
          let jwk = jwk.clone();
          async move { axum::Json(jwk) }
        }),
      )
      .route(
        "/authorize",
        post(|| async {
          (
            StatusCode::FOUND,
            [(LOCATION, "https://idp.example/redirect")],
          )
        }),
      )
      .route(
        "/token",
        post(move || {
          let slot = slot.clone();
          async move { axum::Json(json!({"id_token": slot.lock().unwrap().clone()})) }
        }),
      )
      .route(
        "/userinfo",
        get(|| async {
          axum::Json(json!({
            "sub": "subject-1",
            "email": "oidc@example.com",
            "name": "OIDC User"
          }))
        }),
      );
    tokio::spawn(async move {
      axum::serve(listener, app).await.unwrap();
    });

    let conn = db().await;
    // An admin group must exist for the initial OIDC user to be placed into.
    let group = conn.group().create_group("Admin".into()).await.unwrap();
    conn.setup().set_admin_group_created(group).await.unwrap();

    let oidc_settings = OidcSettings {
      issuer: Url::parse(&base).unwrap(),
      client_id: "client".into(),
      client_secret: "secret".into(),
      scopes: vec!["openid".into()],
      group_sync: false,
      group_claim: "groups".into(),
      pkce: false,
      image_sync: false,
      create_user: true,
    };

    let state = OidcState::new(&conn, None).await;
    state.try_init(&oidc_settings).await.unwrap();
    let jwt = JwtState::init(&AuthConfig::default(), &conn).await;

    // 1. Begin the flow: registers a state + nonce and sets the state cookie.
    let (cookies, _) = oidc_url(
      state.clone(),
      jwt.clone(),
      CookieJar::new(),
      Query(OidcUrlQuery { redirect_to: None }),
    )
    .await
    .unwrap();
    let state_id = *state.state.iter().next().unwrap().key();
    let nonce = *state.nonce.iter().next().unwrap().key();

    // 2. Mint an id_token carrying the matching nonce, signed by the IdP key.
    let exp = chrono::Utc::now().timestamp() + 3600;
    let claims = json!({
      "iss": base,
      "aud": "client",
      "sub": "subject-1",
      "nonce": nonce.to_string(),
      "exp": exp,
    });
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test".into());
    *token_slot.lock().unwrap() = encode(&header, &claims, &enc_key).unwrap();

    // 3. Complete the callback with the matching code + state + cookie.
    let updater: Updater<Msg> = UpdateState::<Msg>::init().await.1;
    let out = oidc_callback::<Msg>(
      axum::extract::Query(OidcCallbackQuery {
        code: Some("auth-code".into()),
        state: Some(state_id),
        error: None,
      }),
      state.clone(),
      cookies,
      conn.clone(),
      SiteConfig::default(),
      jwt,
      updater,
    )
    .await
    .unwrap();

    let resp = out.into_response();
    let loc = resp.headers().get(LOCATION).unwrap().to_str().unwrap();
    // Successful login redirects to the app root without an error.
    assert!(!loc.contains("error="), "unexpected error redirect: {loc}");

    // The user was provisioned and added to the admin group; setup is complete.
    let user = conn
      .user()
      .try_get_user_by_email("oidc@example.com")
      .await
      .unwrap()
      .expect("OIDC user should have been created");
    assert!(user.oidc_user);
    assert_eq!(user.oidc_subject, Some("subject-1".into()));
    assert!(conn.setup().is_setup().await.unwrap());
    assert!(conn.group().is_in_group(group, user.id).await.unwrap());
  }

  #[tokio::test]
  async fn test_callback_matches_existing_user_by_subject() {
    let conn = db().await;
    let user_id = conn
      .user()
      .create_user(
        "existing".into(),
        "old@example.com".into(),
        String::new(),
        "salt".into(),
        true,
        Some("subject-1".into()),
      )
      .await
      .unwrap();

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "new@example.com",
      "name": "OIDC User"
    }))
    .await;
    let loc = run_oidc_callback(&conn, &idp, false, "subject-1", None).await;
    assert!(!loc.contains("error="), "unexpected error redirect: {loc}");

    let user = conn.user().get_user_by_id(user_id).await.unwrap();
    assert_eq!(user.email, "new@example.com");
    assert_eq!(user.name, "OIDC User");
    assert_eq!(user.oidc_subject, Some("subject-1".into()));
  }

  // redirect_to from oidc_url is threaded through to the post-login redirect for
  // an existing user.
  #[tokio::test]
  async fn test_callback_existing_user_honors_redirect_to() {
    let conn = db().await;
    conn
      .user()
      .create_user(
        "existing".into(),
        "old@example.com".into(),
        String::new(),
        "salt".into(),
        true,
        Some("subject-1".into()),
      )
      .await
      .unwrap();

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "new@example.com",
      "name": "OIDC User"
    }))
    .await;
    let loc = run_oidc_callback(&conn, &idp, false, "subject-1", Some("/dashboard")).await;
    assert!(!loc.contains("error="), "unexpected error redirect: {loc}");
    assert_eq!(Url::parse(&loc).unwrap().path(), "/dashboard");
  }

  // redirect_to is also honored on the new-user provisioning path.
  #[tokio::test]
  async fn test_callback_new_user_honors_redirect_to() {
    let conn = db().await;
    let group = conn.group().create_group("Admin".into()).await.unwrap();
    conn.setup().set_admin_group_created(group).await.unwrap();

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "oidc@example.com",
      "name": "OIDC User"
    }))
    .await;
    let loc = run_oidc_callback(&conn, &idp, true, "subject-1", Some("/projects/42")).await;
    assert!(!loc.contains("error="), "unexpected error redirect: {loc}");
    assert_eq!(Url::parse(&loc).unwrap().path(), "/projects/42");
  }

  // The redirect_to query and fragment survive end-to-end (set_path used to
  // percent-encode the '?' and the fragment was dropped entirely).
  #[tokio::test]
  async fn test_callback_honors_redirect_to_query_and_fragment() {
    let conn = db().await;
    conn
      .user()
      .create_user(
        "existing".into(),
        "old@example.com".into(),
        String::new(),
        "salt".into(),
        true,
        Some("subject-1".into()),
      )
      .await
      .unwrap();

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "new@example.com",
      "name": "OIDC User"
    }))
    .await;
    let loc = run_oidc_callback(
      &conn,
      &idp,
      false,
      "subject-1",
      Some("/dashboard?tab=x#frag"),
    )
    .await;
    let url = Url::parse(&loc).unwrap();
    assert!(!loc.contains("error="), "unexpected error redirect: {loc}");
    assert_eq!(url.path(), "/dashboard");
    assert_eq!(url.query(), Some("tab=x"));
    assert_eq!(url.fragment(), Some("frag"));
  }

  #[tokio::test]
  async fn test_callback_subject_match_skips_email_taken_by_other_user() {
    let conn = db().await;
    let user_id = conn
      .user()
      .create_user(
        "existing".into(),
        "old@example.com".into(),
        String::new(),
        "salt".into(),
        true,
        Some("subject-1".into()),
      )
      .await
      .unwrap();
    conn
      .user()
      .create_user(
        "other".into(),
        "taken@example.com".into(),
        "pw".into(),
        "salt".into(),
        false,
        None,
      )
      .await
      .unwrap();

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "taken@example.com",
      "name": "OIDC User"
    }))
    .await;
    let loc = run_oidc_callback(&conn, &idp, false, "subject-1", None).await;
    assert!(!loc.contains("error="), "unexpected error redirect: {loc}");

    // Login succeeds, name syncs, but the conflicting email is left untouched.
    let user = conn.user().get_user_by_id(user_id).await.unwrap();
    assert_eq!(user.name, "OIDC User");
    assert_eq!(user.email, "old@example.com");
    assert_eq!(user.oidc_subject, Some("subject-1".into()));
  }

  #[tokio::test]
  async fn test_callback_email_match_backfills_subject() {
    let conn = db().await;
    let user_id = conn
      .user()
      .create_user(
        "existing".into(),
        "oidc@example.com".into(),
        String::new(),
        "salt".into(),
        true,
        None,
      )
      .await
      .unwrap();

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "oidc@example.com",
      "name": "OIDC User"
    }))
    .await;
    let loc = run_oidc_callback(&conn, &idp, false, "subject-1", None).await;
    assert!(!loc.contains("error="), "unexpected error redirect: {loc}");

    let user = conn.user().get_user_by_id(user_id).await.unwrap();
    assert_eq!(user.oidc_subject, Some("subject-1".into()));
    assert_eq!(user.name, "OIDC User");
  }

  #[tokio::test]
  async fn test_callback_email_match_marks_local_user_as_oidc() {
    let conn = db().await;
    let user_id = conn
      .user()
      .create_user(
        "local".into(),
        "local@example.com".into(),
        "pw".into(),
        "salt".into(),
        false,
        None,
      )
      .await
      .unwrap();

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "local@example.com",
      "name": "OIDC User"
    }))
    .await;
    let loc = run_oidc_callback(&conn, &idp, false, "subject-1", None).await;
    assert!(!loc.contains("error="), "unexpected error redirect: {loc}");

    let user = conn.user().get_user_by_id(user_id).await.unwrap();
    assert_eq!(user.oidc_subject, Some("subject-1".into()));
    assert_eq!(user.name, "OIDC User");
  }

  #[tokio::test]
  async fn test_callback_email_match_conflicting_subject_rejected() {
    let conn = db().await;
    conn
      .user()
      .create_user(
        "existing".into(),
        "oidc@example.com".into(),
        String::new(),
        "salt".into(),
        true,
        Some("other-subject".into()),
      )
      .await
      .unwrap();

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "oidc@example.com",
      "name": "OIDC User"
    }))
    .await;
    let loc = run_oidc_callback(&conn, &idp, false, "subject-1", None).await;
    assert!(
      loc.contains("error=user_not_found"),
      "expected user_not_found, got {loc}"
    );
  }

  #[tokio::test]
  async fn test_callback_no_match_create_user_disabled() {
    let conn = db().await;

    let idp = signing_idp(json!({
      "sub": "subject-1",
      "email": "nobody@example.com",
      "name": "OIDC User"
    }))
    .await;
    // No existing user matches by subject or email, and user creation is off.
    let loc = run_oidc_callback(&conn, &idp, false, "subject-1", None).await;
    assert!(
      loc.contains("error=user_not_found"),
      "expected user_not_found, got {loc}"
    );
    assert!(
      conn
        .user()
        .try_get_user_by_oidc_subject("subject-1")
        .await
        .unwrap()
        .is_none()
    );
  }

  #[tokio::test]
  async fn test_callback_token_endpoint_error() {
    // A mock IdP whose token endpoint rejects the exchange.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let disco_base = base.clone();
    let app = axum::Router::new()
      .route(
        "/.well-known/openid-configuration",
        get(move || {
          let base = disco_base.clone();
          async move {
            axum::Json(json!({
              "issuer": base,
              "authorization_endpoint": format!("{base}/authorize"),
              "token_endpoint": format!("{base}/token"),
              "userinfo_endpoint": format!("{base}/userinfo"),
              "jwks_uri": format!("{base}/jwks"),
            }))
          }
        }),
      )
      .route(
        "/jwks",
        get(|| async {
          axum::Json(json!({"keys":[{"kty":"RSA","kid":"test","alg":"RS256","use":"sig","n":"AQAB","e":"AQAB"}]}))
        }),
      )
      .route(
        "/authorize",
        post(|| async { (StatusCode::FOUND, [(LOCATION, "https://idp.example/redirect")]) }),
      )
      .route(
        "/token",
        post(|| async {
          (StatusCode::BAD_REQUEST, axum::Json(json!({"error":"bad_grant"})))
        }),
      );
    tokio::spawn(async move {
      axum::serve(listener, app).await.unwrap();
    });

    let conn = db().await;
    let state = OidcState::new(&conn, None).await;
    state.try_init(&settings(&base, false)).await.unwrap();
    let jwt = JwtState::init(&AuthConfig::default(), &conn).await;
    let (cookies, _) = oidc_url(
      state.clone(),
      jwt.clone(),
      CookieJar::new(),
      Query(OidcUrlQuery { redirect_to: None }),
    )
    .await
    .unwrap();
    let state_id = *state.state.iter().next().unwrap().key();

    let loc = callback_location(
      state,
      OidcCallbackQuery {
        code: Some("code".into()),
        state: Some(state_id),
        error: None,
      },
      cookies,
      &conn,
    )
    .await;
    // The provider's structured error is surfaced to the login page.
    assert!(loc.contains("error=bad_grant"), "got {loc}");
  }
}
