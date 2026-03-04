pub mod integrity;
use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use integrity::Integrity;
use std::path::PathBuf;

pub struct CacheStore {store_dir: PathBuf, tmp_dir: PathBuf,}

impl CacheStore {
    pub fn new() -> Result<Self> {
        let home = std::env::var("HOME")?;
        let base = PathBuf::from(home).join(".vee");
        let store_dir = base.join("store");
        let tmp_dir = base.join("tmp");
        std::fs::create_dir_all(&store_dir)?;
        std::fs::create_dir_all(&tmp_dir)?;
        Ok(Self { store_dir, tmp_dir })
    }

    fn path_for(&self, integrity: &Integrity) -> (PathBuf, PathBuf) {
        let key = integrity.cache_key();
        let unique_suffix = format!("{}-{}", std::process::id(), std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0));
        (self.store_dir.join(&key), self.tmp_dir.join(format!("{}.{}", key, unique_suffix)))
    }

    pub fn get(&self, integrity_str: &str) -> Result<Option<PathBuf>> {
        let integrity = Integrity::parse(integrity_str)?;
        let (cached, _) = self.path_for(&integrity);
        if cached.exists() {
            Ok(Some(cached))
        } else { Ok(None) }
    }

    pub async fn ensure(
        &self,
        integrity_str: &str,
        tarball_url: &str,
        client: &reqwest::Client,
        auth_header: Option<(String, String)>,
    ) -> Result<PathBuf> {
        let integrity = Integrity::parse(integrity_str)?;
        let (final_dest, temp_dest) = self.path_for(&integrity);
        if final_dest.exists() { return Ok(final_dest);}
        let mut request = client.get(tarball_url);
        if let Some((name, value)) = auth_header {
            request = request.header(name, value);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            bail!("failed to download tarball: {}", response.status());
        }

        let bytes = response.bytes().await?;
        let mut verifier = integrity.verifier();
        verifier.update(&bytes);
        if !verifier.verify() {bail!("integrity check failed for {}", tarball_url);}
        let tmp_for_task = temp_dest.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {let gz = GzDecoder::new(std::io::Cursor::new(bytes));
            let mut archive = tar::Archive::new(gz);
            archive.set_overwrite(true);
            for entry in archive.entries()? {
                let mut entry = entry?;
                let entry_type = entry.header().entry_type();
                if entry_type.is_symlink() || entry_type.is_hard_link() { continue;}
                let path = entry.path()?.into_owned();
                let stripped: std::path::PathBuf = path.components().skip(1).collect();
                if stripped.components().count() == 0 {continue;}
                if stripped.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                }) {
                    continue;
                }
                let full = tmp_for_task.join(&stripped);
                if !full.starts_with(&tmp_for_task) {
                    continue;
                }
                if let Some(parent) = full.parent() {
                    if parent.exists() {
                        let meta = std::fs::metadata(parent)?;
                        if meta.is_file() {std::fs::remove_file(parent)?;}
                    }
                    std::fs::create_dir_all(parent)?;
                }
                if full.exists() {let _ = std::fs::remove_file(&full);}
                entry.unpack(&full).with_context(|| {format!("extract {} to {}", path.display(),full.display())})?;
            }
            Ok(())
        }).await??;
        match tokio::fs::rename(&temp_dest, &final_dest).await {
            Ok(()) => {}
            Err(error) if final_dest.exists() => {
                let _ = tokio::fs::remove_dir_all(&temp_dest).await;
                if !final_dest.exists() {
                    return Err(error.into());
                }
            }
            Err(error) => return Err(error.into()),
        }
        Ok(final_dest)
    }
}
