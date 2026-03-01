use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use super::npmrc::NpmrcConfig;

#[derive(Clone)]
pub struct NpmRegistry {
    pub client: reqwest::Client,
    base_url: String,
    metadata_cache_dir: PathBuf,
    config: NpmrcConfig,
}

fn build_client(strict_ssl: bool) -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(200)
        .tcp_nodelay(true)
        .http2_adaptive_window(true)
        .http2_initial_stream_window_size(2 * 1024 * 1024)
        .http2_initial_connection_window_size(4 * 1024 * 1024)
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .danger_accept_invalid_certs(!strict_ssl)
        .build()
        .expect("failed to build HTTP client")
}

impl NpmRegistry {
    pub fn from_project_dir(project_dir: &Path) -> Result<Self> {
        let home = std::env::var("HOME").unwrap_or_default();
        let metadata_cache_dir = PathBuf::from(home).join(".vee").join("metadata");
        let _ = std::fs::create_dir_all(&metadata_cache_dir);
        let config = NpmrcConfig::load(project_dir)?;
        Ok(Self::with_config(config, metadata_cache_dir))
    }

    pub fn with_config(config: NpmrcConfig, metadata_cache_dir: PathBuf) -> Self {
        let client = build_client(config.strict_ssl);
        Self { client, base_url: config.default_registry.clone(), metadata_cache_dir, config }
    }

    fn registry_url_for(&self, package_name: &str) -> &str {
        if package_name.starts_with('@') {
            if let Some(scope) = package_name.split('/').next() {
                if let Some(registry) = self.config.scoped_registries.get(scope) {
                    return registry.as_str();
                }
            }
        }
        &self.base_url
    }

    pub fn auth_header_for_url(&self, url: &str) -> Option<(String, String)> {
        self.config.auth_header_for_url(url)
    }

    fn cache_key_for(package_name: &str) -> String {package_name.replace('/', "+")}

    pub async fn fetch_metadata(&self, package_name: &str) -> Result<PackageMetadata> {
        let cache_key = Self::cache_key_for(package_name);
        let json_path = self.metadata_cache_dir.join(format!("{}.json", cache_key));
        let etag_path = self.metadata_cache_dir.join(format!("{}.etag", cache_key));
        let registry = self.registry_url_for(package_name);
        let url = format!("{}/{}", registry, package_name);
        let mut request = self.client.get(&url).header("Accept", "application/vnd.npm.install-v1+json");

        if let Some((header_name, header_value)) = self.auth_header_for_url(registry) {
            request = request.header(header_name, header_value);
        }

        let cached_etag = std::fs::read_to_string(&etag_path).ok();
        if let Some(ref etag) = cached_etag {request = request.header("If-None-Match", etag.trim());}
        let response = request.send().await.context("failed to fetch package metadata")?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            if let Ok(cached_json) = std::fs::read_to_string(&json_path) {
                if let Ok(metadata) = serde_json::from_str::<PackageMetadata>(&cached_json) {
                    return Ok(metadata);
                }
            }
        }

        if !response.status().is_success() {
            anyhow::bail!(
                "failed to fetch package metadata: {} {}",
                package_name,
                response.status()
            );
        }

        if let Some(etag) = response.headers().get("etag") {
            if let Ok(etag_str) = etag.to_str() {
                let _ = std::fs::write(&etag_path, etag_str);
            }
        }

        let body = response.text().await.with_context(|| format!("failed to read metadata for '{}'", package_name))?;
        let _ = std::fs::write(&json_path, &body);
        let metadata: PackageMetadata = serde_json::from_str(&body).with_context(|| format!("failed to parse package metadata for '{}'", package_name))?;
        Ok(metadata)
    }

    pub async fn latest_version(&self, package_name: &str) -> Result<String> {
        let metadata = self.fetch_metadata(package_name).await?;
        Ok(metadata.dist_tags.latest.clone())
    }

    pub async fn latest_version_with_metadata(
        &self,
        package_name: &str,
    ) -> Result<(String, PackageMetadata)> {
        let metadata = self.fetch_metadata(package_name).await?;
        let latest = metadata.dist_tags.latest.clone();
        Ok((latest, metadata))
    }
}

fn deserialize_versions<'de, D>(
    deserializer: D,
) -> std::result::Result<HashMap<String, VersionMetadata>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (version_str, version_value) in raw {
        if let Ok(meta) = serde_json::from_value::<VersionMetadata>(version_value) {
            result.insert(version_str, meta);
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageMetadata {
    #[allow(dead_code)]
    pub name: String,
    #[serde(rename = "dist-tags")]
    pub dist_tags: DistTags,
    #[serde(deserialize_with = "deserialize_versions")]
    pub versions: HashMap<String, VersionMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DistTags {pub latest: String}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionMetadata {
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub version: String,
    pub dist: Distribution,
    #[serde(default, rename = "dependencies")]
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(default, rename = "optionalDependencies")]
    pub optional_dependencies: Option<HashMap<String, String>>,
    #[serde(default, rename = "peerDependencies")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(default, rename = "peerDependenciesMeta")]
    pub peer_dependencies_meta: Option<HashMap<String, PeerDependencyMeta>>,
    #[serde(default)]
    #[allow(dead_code)]
    pub bin: Option<serde_json::Value>,
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(default)]
    pub cpu: Option<Vec<String>>,
    #[serde(default, rename = "hasInstallScript")]
    pub has_install_script: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PeerDependencyMeta {
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Distribution {
    pub tarball: String,
    #[serde(default)]
    pub integrity: String,
}
