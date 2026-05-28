use std::{
  collections::HashMap,
  sync::Arc,
  time::{Duration, Instant},
};

use crate::{
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
use argon2::password_hash::SaltString;
use axum::{
  Extension, Json,
  extract::{FromRequestParts, Query},
};
use axum_extra::extract::{CookieJar, cookie::Cookie};
use dashmap::DashMap;
use http::{StatusCode, header::LOCATION};
use jsonwebtoken::{
  DecodingKey, Validation,
  jwk::{AlgorithmParameters, JwkSet},
};
use reqwest::{Client, redirect::Policy};
use rsa::rand_core::OsRng;
use serde::{Deserialize, Serialize};
use tokio::{spawn, sync::Mutex, time::sleep};
use tower_governor::GovernorLayer;
use tracing::{debug, info, warn};
use url::Url;
use uuid::Uuid;

pub const OIDC_STATE: &str = "oidc_state";

pub fn router<T: UpdateMessage>(rate_limiter: &mut RateLimiter) -> BackendRouter {
  #[cfg(feature = "openapi")]
  use aide::axum::routing::get;
  #[cfg(not(feature = "openapi"))]
  use axum::routing::get;

  BackendRouter::new()
    .route("/url", get(oidc_url))
    .layer(GovernorLayer::new(rate_limiter.create_limiter()))
    .route("/callback", get(oidc_callback::<T>))
}

#[derive(Clone, FromRequestParts, Debug, OperationIo)]
#[from_request(via(Extension))]
pub struct OidcState {
  config: Arc<Mutex<Option<OidcConfig>>>,
  state: Arc<DashMap<Uuid, Instant>>,
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
      sso_create_user,
      sso_instant_redirect
    );

    if let Some(oidc_settings) = &settings.oidc_settings() {
      spawn({
        let state = state.clone();
        let oidc_settings = oidc_settings.clone();

        async move {
          if let Err(e) = state.try_init(&oidc_settings).await {
            warn!("Failed to initialize OIDC: {:?}", e);
          }
        }
      });
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
            .retain(|_, &mut instant| now.duration_since(instant) < expiration_duration);
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

    let client = Client::builder().redirect(Policy::none()).build()?;
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
      validation.set_audience(&[self.client_id.to_string()]);
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

async fn oidc_url(
  state: OidcState,
  jwt: JwtState,
  mut cookies: CookieJar,
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

  if !config.scope.is_empty() {
    form.insert("scope", config.scope.join(" "));
  }

  let req = config
    .client
    .post(config.authorization_endpoint.clone())
    .form(&form)
    .build()?;

  let res = config.client.execute(req).await?;

  if !res.status().is_redirection() {
    let body = res.text().await.unwrap_or_default();
    bail!(
      INTERNAL_SERVER_ERROR,
      "OIDC authorization request failed: {}",
      body
    );
  }
  let Some(location) = res.headers().get(LOCATION).and_then(|h| h.to_str().ok()) else {
    bail!(
      INTERNAL_SERVER_ERROR,
      "OIDC authorization response missing location header"
    );
  };

  state.state.insert(state_id, Instant::now());
  cookies = cookies.add(jwt.create_cookie(OIDC_STATE, state_id.to_string()));

  state.nonce.insert(nonce, Instant::now());

  Ok((
    cookies,
    Json(OidcResponse {
      url: location.to_string(),
    }),
  ))
}

#[derive(Deserialize, Clone)]
#[cfg_attr(feature = "openapi", derive(schemars::JsonSchema))]
struct OidcCallbackQuery {
  code: Option<String>,
  state: Uuid,
  error: Option<String>,
}

#[derive(Deserialize)]
struct TokenRes {
  id_token: String,
}

#[derive(Deserialize)]
pub struct AuthInfo {
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
  let lock = oidc_state.config.lock().await;
  let Some(config) = lock.clone() else {
    bail!(BAD_REQUEST, "OIDC not configured");
  };
  drop(lock);

  if oidc_state.state.remove(&state).is_none() {
    bail!(BAD_REQUEST, "Invalid OIDC state");
  }
  let Some(cookie) = cookies.get(OIDC_STATE) else {
    bail!(BAD_REQUEST, "Missing OIDC state cookie");
  };
  if cookie.value() != state.to_string() {
    bail!(BAD_REQUEST, "OIDC state mismatch");
  }

  let (path, error, mut cookies) = check_code(
    error,
    code,
    &config,
    &db,
    cookies,
    &jwt,
    &oidc_state.nonce,
    updater,
  )
  .await?;

  cookies = cookies.remove(Cookie::from(OIDC_STATE));

  let mut url = oidc_config.site_url;
  url.set_path(path);
  url.set_query(error.map(|e| format!("error={e}")).as_deref());

  Ok((cookies, Redirect::found(url.to_string())))
}

#[allow(clippy::too_many_arguments)]
async fn check_code<T: UpdateMessage>(
  error: Option<String>,
  code: Option<String>,
  config: &OidcConfig,
  db: &Connection,
  mut cookies: CookieJar,
  jwt: &JwtState,
  nonce_map: &DashMap<Uuid, Instant>,
  updater: Updater<T>,
) -> Result<(&'static str, Option<String>, CookieJar)> {
  if let Some(error) = error {
    return Ok(("/login", Some(error), cookies));
  }
  let Some(code) = code else {
    return Ok(("/login", Some("missing_code".to_string()), cookies));
  };

  let mut form = HashMap::new();
  form.insert("grant_type", "authorization_code".to_string());
  form.insert("code", code);

  let req = config
    .client
    .post(config.token_endpoint.clone())
    .basic_auth(config.client_id.clone(), Some(config.client_secret.clone()))
    .form(&form)
    .build()?;

  let res = config.client.execute(req).await?;
  if !res.status().is_success() {
    let body = res.text().await.unwrap_or_default();
    bail!(INTERNAL_SERVER_ERROR, "OIDC token request failed: {}", body);
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
    bail!(
      INTERNAL_SERVER_ERROR,
      "OIDC userinfo request failed: {}",
      body
    );
  }
  let res: AuthInfo = res.json().await?;

  if let Some(user) = db.user().try_get_user_by_email(&res.email).await? {
    sync_groups(user.id, &res, config, db, updater.clone()).await?;
    #[cfg(feature = "avatar")]
    if config.image_sync {
      sync_image(
        user.id,
        res.picture,
        db.clone(),
        token,
        config.client.clone(),
        updater,
      )
      .await;
    }

    debug!("OIDC user authenticated: {}", user.id);
    cookies = cookies.add(jwt.create_token(user.id)?);

    return Ok(("/", None, cookies));
  }

  let user_settings = db.settings().get_settings::<UserSettings>().await?;
  if !user_settings.sso_create_user.unwrap_or(false) {
    return Ok(("/login", Some("user_not_found".to_string()), cookies));
  }

  let user = db
    .user()
    .create_user(
      res.name.clone(),
      res.email.clone(),
      String::new(),
      SaltString::generate(OsRng {}).to_string(),
      true,
    )
    .await?;
  sync_groups(user, &res, config, db, updater.clone()).await?;
  #[cfg(feature = "avatar")]
  if config.image_sync {
    sync_image(
      user,
      res.picture,
      db.clone(),
      token,
      config.client.clone(),
      updater,
    )
    .await;
  }

  if !db.setup().is_setup().await? {
    let Some(admin_group_id) = db.setup().get_admin_group_id().await? else {
      bail!(
        INTERNAL_SERVER_ERROR,
        "Admin group has not been created yet, cannot create initial user"
      );
    };

    db.group()
      .add_user_to_groups(user, vec![admin_group_id])
      .await?;

    db.setup().mark_completed().await?;
    info!("Setup completed via OIDC, created user with ID {}", user);
  }

  debug!("OIDC user authenticated: {}", user);
  cookies = cookies.add(jwt.create_token(user)?);

  Ok(("/", None, cookies))
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
