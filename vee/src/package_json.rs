use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageJson {
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    pub dev_dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "optionalDependencies")]
    pub optional_dependencies: BTreeMap<String, String>,
    #[serde(skip)]
    raw: serde_json::Value,
    #[serde(skip)]
    path: PathBuf,
}

impl PackageJson {
    pub fn load(dir: &Path) -> Result<Self> {
        let path = if dir.ends_with("package.json") {dir.to_path_buf()} 
        else {dir.join("package.json")};
        let contents = std::fs::read_to_string(&path).context(format!("failed to read {}", path.display()))?;
        let raw: serde_json::Value = serde_json::from_str(&contents)
            .context(format!("failed to parse {}", path.display()))?;
        let mut package: PackageJson  = serde_json::from_value(raw.clone())
            .context(format!("failed to deserialise {}", path.display()))?;
        package.raw = raw;
        package.path = path;
        Ok(package)
    }
}

impl PackageJson {
    pub fn add_dep(&mut self, name: &str, version: &str, dev: bool) {
        if dev {
            self.dev_dependencies.insert(name.to_string(), version.to_string());
            self.raw["devDependencies"][name] = serde_json::Value::String(version.to_string());
        } else {
            self.dependencies.insert(name.to_string(), version.to_string());
            self.raw["dependencies"][name] = serde_json::Value::String(version.to_string());
        }
    }
    pub fn remove_dep(&mut self, name: &str) -> bool {
        let mut removed = false;
        if self.dependencies.remove(name).is_some() {
            if let Some(deps) = self.raw.get_mut("dependencies").and_then(|value| value.as_object_mut())
            {deps.remove(name);}
            removed = true;
        }
        if self.dev_dependencies.remove(name).is_some() {
            if let Some(deps) = self.raw.get_mut("devDependencies").and_then(|value| value.as_object_mut())
            {deps.remove(name);}

            removed = true;
        }
        if self.optional_dependencies.remove(name).is_some() {
            if let Some(deps) = self.raw.get_mut("optionalDependencies").and_then(|value| value.as_object_mut())
            {deps.remove(name);}
            removed = true;
        }
        removed
    }
    pub fn has_dep(&self, name: &str) -> bool { self.dependencies.contains_key(name) || self.dev_dependencies.contains_key(name) || self.optional_dependencies.contains_key(name)}
}

impl PackageJson {
    pub fn save(&self) -> Result<()> {
        let json =
            serde_json::to_string_pretty(&self.raw).context("failed to serialise package.json")?;
        let contents = format!("{}\n", json);
        std::fs::write(&self.path, contents)
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        Ok(())
    }
    pub fn directory(&self) -> &Path {
        self.path.parent().unwrap_or(&self.path)
    }
}

#[derive(Debug, Deserialize)]
pub struct ScriptsManifest {
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
    #[serde(skip)]
    directory: PathBuf,
}

impl ScriptsManifest {
    pub fn load(dir: &Path) -> Result<Self> {
        let path = if dir.ends_with("package.json") {dir.to_path_buf()} 
        else {dir.join("package.json")
        };
        let contents = std::fs::read_to_string(&path)
            .context(format!("failed to read {}", path.display()))?;
        let mut manifest: ScriptsManifest = serde_json::from_str(&contents)
            .context(format!("failed to parse {}", path.display()))?;
        manifest.directory = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        Ok(manifest)
    }

    pub fn directory(&self) -> &Path {&self.directory}
}

