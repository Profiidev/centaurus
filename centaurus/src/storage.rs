use std::{io::SeekFrom, path::PathBuf, sync::Arc};

use aws_config::Region;
use aws_sdk_s3::{
  config::{Credentials, SharedCredentialsProvider},
  error::SdkError,
  primitives::ByteStream,
  types::{CompletedMultipartUpload, CompletedPart},
};
use axum::body::Body;
use eyre::Context;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::{
  fs,
  io::{self, AsyncRead, AsyncReadExt, AsyncSeekExt},
};
use tokio_util::io::ReaderStream;
use tracing::{info, warn};

use crate::{
  bail,
  error::{ErrorReportStatusExt, Result},
};

#[derive(Clone)]
#[cfg_attr(feature = "openapi", derive(aide::OperationIo))]
#[cfg_attr(feature = "backend", derive(axum::extract::FromRequestParts))]
#[cfg_attr(feature = "backend", from_request(via(axum::extract::Extension)))]
pub enum FileStorage {
  Local(PathBuf),
  S3 {
    client: Arc<aws_sdk_s3::Client>,
    bucket: String,
  },
}

impl FileStorage {
  pub async fn init(config: &StorageConfig) -> Result<Self> {
    if !config.use_s3() {
      let path = PathBuf::from(&config.storage_path);

      // Setup and check read and write permissions for the local storage path
      fs::create_dir_all(&path).await?;
      let test_file = path.join("test_permission.tmp");
      let test_content = b"test";
      fs::write(&test_file, test_content).await?;
      let read_content = fs::read(&test_file).await?;
      fs::remove_file(&test_file).await?;
      if read_content != test_content {
        bail!("Failed to verify access permission on storage path");
      }

      info!("Using local file storage at {}", path.display());
      return Ok(Self::Local(path));
    }

    let credentials = Credentials::new(
      // unwrap is safe here because the presence of these fields is already checked in config.use_s3()
      config.s3_access_key.as_ref().unwrap(),
      // unwrap is safe here because the presence of these fields is already checked in config.use_s3()
      config.s3_secret_key.as_ref().unwrap(),
      None,
      None,
      "file_storage",
    );

    // unwrap is safe here because the presence of these fields is already checked in config.use_s3()
    let mut builder = aws_sdk_s3::Config::builder()
      .region(Some(Region::new(config.s3_region.clone().unwrap())))
      .endpoint_url(config.s3_host.clone().unwrap())
      .credentials_provider(SharedCredentialsProvider::new(credentials));

    if config.s3_force_path_style {
      builder = builder.force_path_style(true);
    }

    // unwrap is safe here because the presence of these fields is already checked in config.use_s3()
    let bucket = config.s3_bucket.clone().unwrap();
    let config = builder.build();
    let client = aws_sdk_s3::Client::from_conf(config);

    let buckets = client
      .list_buckets()
      .send()
      .await
      .context("Failed to list S3 buckets")?;

    if !buckets
      .buckets()
      .iter()
      .any(|b| b.name().unwrap_or_default() == bucket)
    {
      bail!("S3 bucket does not exist");
    }

    info!("Using S3 file storage with bucket {}", bucket);
    Ok(Self::S3 {
      client: Arc::new(client),
      bucket,
    })
  }

  pub fn name(&self) -> &'static str {
    match self {
      Self::Local(_) => "Local",
      Self::S3 { .. } => "S3",
    }
  }

  pub async fn save_file<R: AsyncRead + Unpin + Send>(
    &self,
    reader: &mut R,
    name: &str,
  ) -> Result<()> {
    match self {
      Self::Local(path) => {
        let file_path = path.join(name);
        let mut file = fs::File::create(&file_path).await?;
        io::copy(reader, &mut file).await?;
      }
      Self::S3 { client, bucket } => {
        const CHUNK_SIZE: usize = 8 * 1024 * 1024; // 8MB

        async fn read_chunk<R: AsyncRead + Unpin + Send>(reader: &mut R) -> Result<Vec<u8>> {
          let mut buffer = vec![0; CHUNK_SIZE];
          let mut total_read = 0;
          while total_read < CHUNK_SIZE {
            let n = reader.read(&mut buffer[total_read..]).await?;
            if n == 0 {
              break;
            }
            total_read += n;
          }
          buffer.truncate(total_read);
          Ok(buffer)
        }

        let first_chunk = read_chunk(reader).await?;

        if first_chunk.len() < CHUNK_SIZE {
          // If the first chunk is smaller than the chunk size, we can upload it directly
          client
            .put_object()
            .bucket(bucket)
            .key(name)
            .body(ByteStream::from(first_chunk))
            .send()
            .await
            .context("Failed to upload file to S3 Bucket")?;
          return Ok(());
        }

        let multipart_upload = client
          .create_multipart_upload()
          .bucket(bucket)
          .key(name)
          .send()
          .await
          .context("Failed to create multipart upload for file in S3 Bucket")?;

        let upload_id = multipart_upload.upload_id().status_context(
          StatusCode::INTERNAL_SERVER_ERROR,
          "Failed to get upload ID for multipart upload",
        )?;
        let mut parts: Vec<CompletedPart> = Vec::new();

        loop {
          let chunk = if parts.is_empty() {
            first_chunk.clone()
          } else {
            read_chunk(reader).await?
          };

          let done = chunk.len() < CHUNK_SIZE;
          let part_number = (parts.len() + 1) as i32;

          let part = client
            .upload_part()
            .bucket(bucket)
            .key(name)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(ByteStream::from(chunk))
            .send()
            .await
            .context("Failed to upload part of file to S3 Bucket")?;
          let part = CompletedPart::builder()
            .set_e_tag(part.e_tag().map(|s| s.to_string()))
            .part_number(part_number)
            .build();
          parts.push(part);

          if done {
            break;
          }
        }

        let completed_mulipart_upload = CompletedMultipartUpload::builder()
          .set_parts(Some(parts))
          .build();
        client
          .complete_multipart_upload()
          .bucket(bucket)
          .key(name)
          .upload_id(upload_id)
          .multipart_upload(completed_mulipart_upload)
          .send()
          .await
          .context("Failed to complete multipart upload for file in S3 Bucket")?;
      }
    }

    Ok(())
  }

  pub async fn get_file(&self, name: &str, range: Option<(u64, u64)>) -> Result<Body> {
    if !self.exists(name).await? {
      bail!(NOT_FOUND, "File file not found");
    }

    match self {
      Self::Local(path) => {
        let file_path = path.join(name);
        let mut file = fs::File::open(file_path).await?;

        if let Some((start, end)) = range {
          if file.seek(SeekFrom::Start(start)).await.is_err() {
            bail!(RANGE_NOT_SATISFIABLE, "Invalid range header");
          }

          let reader = file.take(end - start + 1);
          let stream = ReaderStream::new(reader);
          return Ok(Body::from_stream(stream));
        }

        Ok(Body::from_stream(ReaderStream::new(file)))
      }
      Self::S3 { client, bucket } => {
        let res = client
          .get_object()
          .bucket(bucket)
          .key(name)
          .set_range(range.map(|(start, end)| format!("bytes={}-{}", start, end)))
          .send()
          .await
          .context("Failed to download file from S3 Bucket")?;

        Ok(Body::from_stream(ReaderStream::new(
          res.body.into_async_read(),
        )))
      }
    }
  }

  pub async fn exists(&self, name: &str) -> Result<bool> {
    match self {
      Self::Local(path) => {
        let file_path = path.join(name);
        Ok(file_path.exists())
      }
      Self::S3 { client, bucket } => {
        let res = client.head_object().bucket(bucket).key(name).send().await;

        match res {
          Ok(_) => Ok(true),
          Err(SdkError::ServiceError(e)) => {
            if e.err().is_not_found() {
              Ok(false)
            } else {
              bail!("Failed to check file existence in S3 Bucket: {}", e.err());
            }
          }
          Err(e) => Err(dbg!(e)).context("Failed to check file existence in S3 Bucket")?,
        }
      }
    }
  }

  pub async fn delete_file(&self, name: &str) -> Result<()> {
    if !self.exists(name).await? {
      return Ok(());
    }

    match self {
      Self::Local(path) => {
        let file_path = path.join(name);
        fs::remove_file(file_path).await?;
      }
      Self::S3 { client, bucket } => {
        client
          .delete_object()
          .bucket(bucket)
          .key(name)
          .send()
          .await
          .context("Failed to delete nar from S3 Bucket")?;
      }
    }

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
  use tempfile::tempdir;

  async fn read_body(body: Body) -> Vec<u8> {
    axum::body::to_bytes(body, usize::MAX).await.unwrap().to_vec()
  }

  #[tokio::test]
  async fn test_local_storage() {
    let dir = tempdir().unwrap();
    let config = StorageConfig {
      storage_path: dir.path().to_str().unwrap().to_string(),
      ..Default::default()
    };

    let storage = FileStorage::init(&config).await.unwrap();
    assert_eq!(storage.name(), "Local");

    let mut content = b"hello world" as &[u8];
    storage.save_file(&mut content, "test.txt").await.unwrap();
    assert!(storage.exists("test.txt").await.unwrap());

    storage.delete_file("test.txt").await.unwrap();
    assert!(!storage.exists("test.txt").await.unwrap());
  }

  #[tokio::test]
  async fn test_local_get_file_full_and_range() {
    let dir = tempdir().unwrap();
    let storage = FileStorage::Local(dir.path().to_path_buf());

    let mut content = b"0123456789" as &[u8];
    storage.save_file(&mut content, "data.bin").await.unwrap();

    // Full read returns the whole file.
    let body = storage.get_file("data.bin", None).await.unwrap();
    assert_eq!(read_body(body).await, b"0123456789");

    // A byte range returns only the requested slice (inclusive bounds).
    let body = storage.get_file("data.bin", Some((2, 5))).await.unwrap();
    assert_eq!(read_body(body).await, b"2345");
  }

  #[tokio::test]
  async fn test_local_get_missing_file_is_not_found() {
    let dir = tempdir().unwrap();
    let storage = FileStorage::Local(dir.path().to_path_buf());
    let err = storage.get_file("nope", None).await.unwrap_err();
    assert_eq!(err.status, StatusCode::NOT_FOUND);
  }

  #[tokio::test]
  async fn test_delete_missing_file_is_ok() {
    let dir = tempdir().unwrap();
    let storage = FileStorage::Local(dir.path().to_path_buf());
    // Deleting a non-existent file is a no-op success.
    assert!(storage.delete_file("ghost").await.is_ok());
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
