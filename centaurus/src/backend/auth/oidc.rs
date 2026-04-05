use std::{
  collections::HashMap,
  sync::Arc,
  time::{Duration, Instant},
};

use crate::{
  backend::{BackendRouter, auth::settings::{OidcSettings, UserSettings}, middleware::rate_limiter::RateLimiter},
  bail,
  db::{init::Connection, tables::ConnectionExt},
  error::{ErrorReportStatusExt, Result},
  req::redirect::Redirect,
};
use aide::OperationIo;
use argon2::password_hash::SaltString;
use axum::{
  Extension, Json,
  extract::{FromRequestParts, Query},
};
use axum_extra::extract::{CookieJar, cookie::Cookie};
use http::{StatusCode, header::LOCATION};
use jsonwebtoken::{
  DecodingKey, Validation,
  jwk::{AlgorithmParameters, JwkSet},
};
use reqwest::{Client, redirect::Policy};
use rsa::rand_core::OsRng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::{spawn, sync::Mutex, time::sleep};
use tower_governor::GovernorLayer;
use tracing::{debug, info};
use url::Url;
use uuid::Uuid;

pub const OIDC_STATE: &str = "oidc_state";

pub fn router(rate_limiter: &mut RateLimiter) -> BackendRouter {
  #[cfg(feature = "openapi")]
  use aide::axum::routing::get;
  #[cfg(not(feature = "openapi"))]
  use axum::routing::get;

  BackendRouter::new()
    .route("/url", get(oidc_url))
    .layer(GovernorLayer::new(rate_limiter.create_limiter()))
    .route("/callback", get(oidc_callback))
}

#[derive(Clone, FromRequestParts, Debug, OperationIo)]
#[from_request(via(Extension))]
pub struct OidcState(Arc<Mutex<Option<OidcConfig>>>);

#[derive(Debug)]
struct OidcConfig {
  state: HashMap<Uuid, Instant>,
  nonce: HashMap<Uuid, Instant>,
  issuer: String,
  authorization_endpoint: Url,
  token_endpoint: Url,
  userinfo_endpoint: Url,
  jwk_set: JwkSet,
  client_id: String,
  client_secret: String,
  client: Client,
  scope: Vec<String>,
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
  pub async fn new(db: &Connection) -> Self {
    let state = Self(Arc::new(Mutex::new(None)));
    let settings: UserSettings = db.settings().get_settings().await.unwrap_or_default();
    if let Some(oidc_settings) = &settings.oidc {
      let _ = state.try_init(oidc_settings).await;
    }

    spawn({
      let state = state.clone();
      async move {
        let cleanup_interval = Duration::from_secs(600);
        let expiration_duration = Duration::from_secs(600);
        loop {
          sleep(cleanup_interval).await;
          let mut lock = state.0.lock().await;
          if let Some(config) = lock.as_mut() {
            let now = Instant::now();
            config
              .state
              .retain(|_, &mut instant| now.duration_since(instant) < expiration_duration);
            config
              .nonce
              .retain(|_, &mut instant| now.duration_since(instant) < expiration_duration);
          }
        }
      }
    });

    state
  }

  pub async fn try_init(&self, settings: &OidcSettings) -> Result<()> {
    let config = OidcConfig::new(settings).await?;
    let mut lock = self.0.lock().await;
    *lock = Some(config);
    Ok(())
  }

  pub async fn deactivate(&self) {
    let mut lock = self.0.lock().await;
    *lock = None;
  }

  pub async fn is_enabled(&self) -> bool {
    let lock = self.0.lock().await;
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

    Ok(Self {
      state: Default::default(),
      nonce: Default::default(),
      issuer: config.issuer,
      authorization_endpoint: config.authorization_endpoint,
      token_endpoint: config.token_endpoint,
      userinfo_endpoint: config.userinfo_endpoint,
      jwk_set,
      client_id: oidc_settings.client_id.clone(),
      client_secret: oidc_settings.client_secret.clone(),
      client,
      scope: oidc_settings.scopes.clone(),
    })
  }
}

impl OidcConfig {
  async fn validate_jwk(&mut self, token: &str) -> Result<()> {
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
    if self.nonce.remove(&nonce).is_none() {
      bail!(INTERNAL_SERVER_ERROR, "Invalid nonce");
    }

    Ok(())
  }
}

#[derive(Serialize, JsonSchema)]
struct OidcResponse {
  url: String,
}

async fn oidc_url(
  state: OidcState,
  jwt: JwtState,
  mut cookies: CookieJar,
) -> Result<(CookieJar, Json<OidcResponse>)> {
  if let Some(config) = state.0.lock().await.as_mut() {
    let state = Uuid::new_v4();
    let nonce = Uuid::new_v4();

    let mut form = HashMap::new();
    form.insert("response_type", "code".to_string());
    form.insert("client_id", config.client_id.clone());
    form.insert("state", state.to_string());
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

    config.state.insert(state, Instant::now());
    cookies = cookies.add(jwt.create_cookie(OIDC_STATE, state.to_string()));

    config.nonce.insert(nonce, Instant::now());

    Ok((
      cookies,
      Json(OidcResponse {
        url: location.to_string(),
      }),
    ))
  } else {
    bail!(BAD_REQUEST, "OIDC not configured");
  }
}

#[derive(Deserialize, JsonSchema)]
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
}

async fn oidc_callback(
  Query(OidcCallbackQuery { code, state, error }): Query<OidcCallbackQuery>,
  oidc_state: OidcState,
  cookies: CookieJar,
  db: Connection,
  app_config: Config,
  jwt: JwtState,
) -> Result<(CookieJar, Redirect)> {
  let mut lock = oidc_state.0.lock().await;
  let Some(config) = lock.as_mut() else {
    bail!(BAD_REQUEST, "OIDC not configured");
  };

  if config.state.remove(&state).is_none() {
    bail!(BAD_REQUEST, "Invalid OIDC state");
  }
  let Some(cookie) = cookies.get(OIDC_STATE) else {
    bail!(BAD_REQUEST, "Missing OIDC state cookie");
  };
  if cookie.value() != state.to_string() {
    bail!(BAD_REQUEST, "OIDC state mismatch");
  }

  let (path, error, mut cookies) = check_code(error, code, config, &db, cookies, &jwt).await?;

  cookies = cookies.remove(Cookie::from(OIDC_STATE));

  let mut url = app_config.site_url;
  url.set_path(path);
  url.set_query(error.map(|e| format!("error={e}")).as_deref());

  Ok((cookies, Redirect::found(url.to_string())))
}

async fn check_code(
  error: Option<String>,
  code: Option<String>,
  config: &mut OidcConfig,
  db: &Connection,
  mut cookies: CookieJar,
  jwt: &JwtState,
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
  config.validate_jwk(&res.id_token).await?;

  let req = config
    .client
    .get(config.userinfo_endpoint.clone())
    .bearer_auth(res.id_token)
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
    debug!("OIDC user authenticated: {}", user.id);
    cookies = cookies.add(jwt.create_token(user.id)?);

    return Ok(("/", None, cookies));
  }

  let user_settings = db.settings().get_settings::<UserSettings>().await?;
  if !user_settings.sso_create_user {
    return Ok(("/login", Some("user_not_found".to_string()), cookies));
  }

  let user = db
    .user()
    .create_user(
      res.name,
      res.email,
      String::new(),
      SaltString::generate(OsRng {}).to_string(),
    )
    .await?;

  debug!("OIDC user authenticated: {}", user);
  cookies = cookies.add(jwt.create_token(user)?);

  Ok(("/", None, cookies))
}
