//! Filesystem storage backend: objects live under a root directory
//! (`STORAGE_LOCAL_DIR`, default `{DATA_DIR}/files`), keys map 1:1 to paths.

use std::path::PathBuf;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use super::{Storage, StorageError, StorageStream};

pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Map a key to a path, rejecting anything that tries to escape the root.
    fn path_for(&self, key: &str) -> Result<PathBuf, StorageError> {
        if key
            .split('/')
            .any(|seg| seg == ".." || seg.is_empty() || seg == ".")
        {
            return Err(StorageError::Backend(format!("invalid key: {key}")));
        }
        Ok(self.root.join(key))
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StorageError> {
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::Io(format!("create dir failed: {e}")))?;
        }
        tokio::fs::write(&path, data)
            .await
            .map_err(|e| StorageError::Io(format!("write failed: {e}")))
    }

    async fn get(&self, key: &str) -> Result<StorageStream, StorageError> {
        let path = self.path_for(key)?;
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => StorageError::NotFound(key.to_string()),
                _ => StorageError::Io(format!("open failed: {e}")),
            })?;
        let stream = tokio_util::io::ReaderStream::new(file)
            .map(|r| r.map_err(|e| StorageError::Io(format!("read failed: {e}"))));
        Ok(Box::pin(stream))
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.path_for(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Io(format!("delete failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_storage() -> (LocalStorage, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (LocalStorage::new(dir.path().to_path_buf()), dir)
    }

    #[tokio::test]
    async fn put_get_delete_roundtrip() {
        let (storage, _dir) = test_storage().await;
        let key = "files/00000000-0000-0000-0000-000000000001/hello.txt";

        storage
            .put(key, Bytes::from_static(b"hello world"))
            .await
            .unwrap();

        let mut stream = storage.get(key).await.unwrap();
        let mut buf = Vec::new();
        while let Some(chunk) = stream.next().await {
            buf.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(buf, b"hello world");

        storage.delete(key).await.unwrap();
        assert!(matches!(
            storage.get(key).await,
            Err(StorageError::NotFound(_))
        ));
        // Deleting again is idempotent.
        storage.delete(key).await.unwrap();
    }

    #[tokio::test]
    async fn traversal_key_rejected() {
        let (storage, _dir) = test_storage().await;
        assert!(storage.put("../evil.txt", Bytes::new()).await.is_err());
        assert!(storage.get("../evil.txt").await.is_err());
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let (storage, _dir) = test_storage().await;
        let err = match storage.get("files/x/missing.bin").await {
            Err(e) => e,
            Ok(_) => panic!("expected NotFound"),
        };
        assert!(matches!(err, StorageError::NotFound(_)));
    }
}
