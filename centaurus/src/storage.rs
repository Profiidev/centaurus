use std::{path::PathBuf, sync::Arc};

use axum::body::Body;
use futures_util::StreamExt;
use object_store::{
  GetOptions, GetRange, ObjectStore, ObjectStoreExt, aws::AmazonS3Builder, buffered::BufWriter,
  local::LocalFileSystem, path::Path,
};
use serde::{Deserialize, Serialize};
use tokio::{
  fs,
  io::{self, AsyncRead, AsyncWriteExt},
};
use tracing::{info, warn};

use crate::{anyhow, bail, error::Result};

pub use object_store::path::Path as StoragePath;

#[derive(Clone)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
#[cfg_attr(feature = "backend", derive(axum::extract::FromRequestParts))]
#[cfg_attr(feature = "backend", from_request(via(axum::extract::Extension)))]
pub struct FileStorage(Arc<dyn ObjectStore>, &'static str);

impl FileStorage {
  pub async fn init(config: &StorageConfig) -> Result<Self> {
    if !config.use_s3() {
      let path = PathBuf::from(&config.storage_path);

      // Setup and check read and write permissions for the local storage path
      fs::create_dir_all(&path).await?;
      // Unique name so concurrent inits on the same path do not delete each other's probe file
      let test_file = path.join(format!(
        "test_permission_{}_{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap_or_default()
          .as_nanos()
      ));
      let test_content = b"test";
      fs::write(&test_file, test_content).await?;
      let read_content = fs::read(&test_file).await?;
      fs::remove_file(&test_file).await?;
      if read_content != test_content {
        bail!("Failed to verify access permission on storage path");
      }

      let fs = LocalFileSystem::new_with_prefix(&path)?
        .with_fsync(true)
        .with_automatic_cleanup(true);

      info!("Using local file storage at {}", path.display());
      return Ok(Self(Arc::new(fs), "Local"));
    }

    // unwrap is safe here because the presence of these fields is already checked in config.use_s3()
    let host = config.s3_host.as_ref().unwrap();
    let s3 = AmazonS3Builder::default()
      .with_access_key_id(config.s3_access_key.as_ref().unwrap())
      .with_secret_access_key(config.s3_secret_key.as_ref().unwrap())
      .with_region(config.s3_region.as_ref().unwrap())
      .with_bucket_name(config.s3_bucket.as_ref().unwrap())
      .with_endpoint(host)
      .with_allow_http(host.starts_with("http://"))
      .with_virtual_hosted_style_request(!config.s3_force_path_style)
      .build()?;

    let mut stream = s3.list(None);
    if let Some(Err(e)) = stream.next().await {
      bail!("connection/auth error: {e}");
    }

    // unwrap is safe here because the presence of these fields is already checked in config.use_s3()
    let bucket = config.s3_bucket.clone().unwrap();

    info!("Using S3 file storage with bucket {}", bucket);
    Ok(Self(Arc::new(s3), "S3"))
  }

  pub fn name(&self) -> &'static str {
    self.1
  }

  pub async fn save_file<R: AsyncRead + Unpin + Send>(
    &self,
    reader: &mut R,
    path: Path,
  ) -> Result<()> {
    let mut writer = BufWriter::new(self.0.clone(), path);
    io::copy(reader, &mut writer).await?;
    writer.shutdown().await?;

    Ok(())
  }

  pub async fn get_file(&self, path: &Path, range: Option<(u64, u64)>) -> Result<Body> {
    if !self.exists(path).await? {
      bail!(NOT_FOUND, "File file not found");
    }

    let opts = GetOptions {
      range: range.map(|(start, end)| GetRange::Bounded(start..end + 1)),
      ..Default::default()
    };

    let result = self.0.get_opts(path, opts).await?;
    let stream = result.into_stream();
    let body = Body::from_stream(stream);
    Ok(body)
  }

  pub async fn exists(&self, path: &Path) -> Result<bool> {
    match self.0.head(path).await {
      Err(object_store::Error::NotFound { .. }) => Ok(false),
      Ok(_) => Ok(true),
      Err(e) => Err(anyhow!("Failed to check if file exists: {e}")),
    }
  }

  pub async fn delete_file(&self, path: &Path) -> Result<()> {
    if !self.exists(path).await? {
      return Ok(());
    }

    self.0.delete(path).await?;

    Ok(())
  }
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct StorageConfig {
  pub storage_path: String,
  pub s3_bucket: Option<String>,
  pub s3_region: Option<String>,
  pub s3_host: Option<String>,
  pub s3_access_key: Option<String>,
  pub s3_secret_key: Option<String>,
  pub s3_force_path_style: bool,
}

impl StorageConfig {
  pub fn validate(&self) {
    if (self.s3_bucket.is_some()
      || self.s3_region.is_some()
      || self.s3_access_key.is_some()
      || self.s3_secret_key.is_some()
      || self.s3_host.is_some())
      && !self.use_s3()
    {
      warn!(
        "Only some S3 config options are set: Bucket: {}, Region: {}, Host: {}, Access Key: {}, Secret Key: {}",
        self.s3_bucket.is_some(),
        self.s3_region.is_some(),
        self.s3_host.is_some(),
        self.s3_access_key.is_some(),
        self.s3_secret_key.is_some()
      );
    }

    if !self.use_s3() && self.storage_path.is_empty() {
      panic!("STORAGE_PATH is not set and S3 config is incomplete");
    }
  }

  pub fn use_s3(&self) -> bool {
    self.s3_bucket.is_some()
      && self.s3_region.is_some()
      && self.s3_access_key.is_some()
      && self.s3_secret_key.is_some()
      && self.s3_host.is_some()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use http::StatusCode;
  use tempfile::tempdir;

  async fn local(dir: &std::path::Path) -> FileStorage {
    let config = StorageConfig {
      storage_path: dir.to_str().unwrap().to_string(),
      ..Default::default()
    };
    FileStorage::init(&config).await.unwrap()
  }

  async fn read_body(body: Body) -> Vec<u8> {
    axum::body::to_bytes(body, usize::MAX)
      .await
      .unwrap()
      .to_vec()
  }

  #[tokio::test]
  async fn test_local_storage() {
    let dir = tempdir().unwrap();
    let storage = local(dir.path()).await;
    assert_eq!(storage.name(), "Local");

    let mut content = b"hello world" as &[u8];
    storage
      .save_file(&mut content, Path::from("test.txt"))
      .await
      .unwrap();
    assert!(storage.exists(&Path::from("test.txt")).await.unwrap());

    storage.delete_file(&Path::from("test.txt")).await.unwrap();
    assert!(!storage.exists(&Path::from("test.txt")).await.unwrap());
  }

  #[tokio::test]
  async fn test_local_save_file_creates_nested_dirs() {
    let dir = tempdir().unwrap();
    let storage = local(dir.path()).await;

    // A name containing "/" must create the intermediate directories.
    let mut content = b"nested" as &[u8];
    storage
      .save_file(&mut content, Path::from("a/b/c/file.txt"))
      .await
      .unwrap();

    // The directory tree was created on disk.
    assert!(dir.path().join("a/b/c").is_dir());
    assert!(dir.path().join("a/b/c/file.txt").is_file());

    // The file is reachable through the normal API surface.
    assert!(storage.exists(&Path::from("a/b/c/file.txt")).await.unwrap());
    let body = storage
      .get_file(&Path::from("a/b/c/file.txt"), None)
      .await
      .unwrap();
    assert_eq!(read_body(body).await, b"nested");

    storage
      .delete_file(&Path::from("a/b/c/file.txt"))
      .await
      .unwrap();
    assert!(!storage.exists(&Path::from("a/b/c/file.txt")).await.unwrap());
  }

  #[tokio::test]
  async fn test_local_get_file_full_and_range() {
    let dir = tempdir().unwrap();
    let storage = local(dir.path()).await;

    let mut content = b"0123456789" as &[u8];
    storage
      .save_file(&mut content, Path::from("data.bin"))
      .await
      .unwrap();

    // Full read returns the whole file.
    let body = storage
      .get_file(&Path::from("data.bin"), None)
      .await
      .unwrap();
    assert_eq!(read_body(body).await, b"0123456789");

    // A byte range returns only the requested slice (inclusive bounds).
    let body = storage
      .get_file(&Path::from("data.bin"), Some((2, 5)))
      .await
      .unwrap();
    assert_eq!(read_body(body).await, b"2345");
  }

  #[tokio::test]
  async fn test_local_get_missing_file_is_not_found() {
    let dir = tempdir().unwrap();
    let storage = local(dir.path()).await;
    let err = storage
      .get_file(&Path::from("nope"), None)
      .await
      .unwrap_err();
    assert_eq!(err.status, StatusCode::NOT_FOUND);
  }

  #[tokio::test]
  async fn test_delete_missing_file_is_ok() {
    let dir = tempdir().unwrap();
    let storage = local(dir.path()).await;
    // Deleting a non-existent file is a no-op success.
    assert!(storage.delete_file(&Path::from("ghost")).await.is_ok());
  }

  #[test]
  fn test_storage_config_use_s3() {
    let mut config = StorageConfig {
      storage_path: "/tmp".into(),
      ..Default::default()
    };
    assert!(!config.use_s3());
    // Partial S3 config is still not "use s3".
    config.s3_bucket = Some("b".into());
    assert!(!config.use_s3());

    // Fully specified S3 config flips the switch.
    config.s3_region = Some("r".into());
    config.s3_host = Some("h".into());
    config.s3_access_key = Some("a".into());
    config.s3_secret_key = Some("s".into());
    assert!(config.use_s3());
    // validate() must not panic on a complete config.
    config.validate();
  }

  #[test]
  #[should_panic(expected = "STORAGE_PATH is not set")]
  fn test_storage_config_validate_panics_without_path() {
    let config = StorageConfig::default();
    config.validate();
  }

  #[tokio::test]
  async fn test_s3_init_unreachable_endpoint_errors() {
    // A fully-specified but unreachable S3 endpoint exercises the S3 init path
    // (credentials, path-style, client build) and fails at the bucket check.
    let config = StorageConfig {
      storage_path: String::new(),
      s3_bucket: Some("bucket".into()),
      s3_region: Some("us-east-1".into()),
      s3_host: Some("http://127.0.0.1:9".into()),
      s3_access_key: Some("key".into()),
      s3_secret_key: Some("secret".into()),
      s3_force_path_style: true,
    };
    assert!(config.use_s3());
    assert!(FileStorage::init(&config).await.is_err());
  }
}
