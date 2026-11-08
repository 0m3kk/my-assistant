//! S3 storage backend (any S3-compatible API: AWS S3, MinIO, R2, …),
//! built on the `object_store` crate.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use object_store::ObjectStore;

use super::{Storage, StorageError, StorageStream};
use crate::config::Config;

pub struct S3Storage {
    store: Arc<dyn ObjectStore>,
}

impl S3Storage {
    /// Build from `S3_*` configuration. Missing required settings fail loudly
    /// at startup (see `Config::validate`).
    pub fn from_config(cfg: &Config) -> Result<Self, String> {
        let bucket = cfg
            .s3_bucket
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or("S3_BUCKET is required when STORAGE_BACKEND=s3")?;
        let access_key = cfg
            .s3_access_key_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or("S3_ACCESS_KEY_ID is required when STORAGE_BACKEND=s3")?;
        let secret_key = cfg
            .s3_secret_access_key
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or("S3_SECRET_ACCESS_KEY is required when STORAGE_BACKEND=s3")?;

        let mut builder = object_store::aws::AmazonS3Builder::new()
            .with_bucket_name(bucket)
            .with_access_key_id(access_key)
            .with_secret_access_key(secret_key);
        if let Some(endpoint) = cfg.s3_endpoint.as_deref().filter(|s| !s.trim().is_empty()) {
            builder = builder.with_endpoint(endpoint);
        }
        if let Some(region) = cfg.s3_region.as_deref().filter(|s| !s.trim().is_empty()) {
            builder = builder.with_region(region);
        }
        if cfg.s3_allow_http {
            // Plain HTTP for local MinIO-style endpoints.
            builder = builder.with_allow_http(true);
        }
        let store: Arc<dyn ObjectStore> = Arc::new(
            builder
                .build()
                .map_err(|e| format!("invalid S3 config: {e}"))?,
        );
        Ok(Self { store })
    }

    fn map_err(key: &str, e: object_store::Error) -> StorageError {
        if matches!(e, object_store::Error::NotFound { .. }) {
            StorageError::NotFound(key.to_string())
        } else {
            StorageError::Backend(e.to_string())
        }
    }
}

#[async_trait]
impl Storage for S3Storage {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StorageError> {
        let path = object_store::path::Path::from(key);
        self.store
            .put(&path, data.into())
            .await
            .map_err(|e| Self::map_err(key, e))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<StorageStream, StorageError> {
        let key_owned = key.to_string();
        let path = object_store::path::Path::from(key);
        let result = self
            .store
            .get(&path)
            .await
            .map_err(|e| Self::map_err(&key_owned, e))?;
        let stream = result.into_stream().map(move |r| {
            r.map_err(|e| {
                if matches!(e, object_store::Error::NotFound { .. }) {
                    StorageError::NotFound(key_owned.clone())
                } else {
                    StorageError::Backend(e.to_string())
                }
            })
        });
        Ok(Box::pin(stream))
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = object_store::path::Path::from(key);
        match self.store.delete(&path).await {
            Ok(()) => Ok(()),
            // object_store's delete is idempotent in most builds, but older
            // versions/servers may surface NotFound — treat it as success.
            Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(Self::map_err(key, e)),
        }
    }
}
