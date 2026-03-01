use crate::resolver::{PackageInfo, ResolveResult};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
const LOCKFILE_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
pub struct Lockfile {
    pub lockfile_version: u32,
    pub root_deps: BTreeMap<String, String>,
    #[serde(default)]
    pub root_resolved: BTreeMap<String, String>,
    pub packages: BTreeMap<String, LockedPackage>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub tarball_url: String,
    pub integrity: String,
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub optional_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub peer_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub optional_peers: Vec<String>,
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(default)]
    pub cpu: Option<Vec<String>>,
    #[serde(default)]
    pub has_install_script: bool,
    #[serde(default)]
    pub resolved_deps: BTreeMap<String, String>,
}

impl Lockfile {
    pub fn from_resolve_result(result: &ResolveResult, root_deps: &BTreeMap<String, String>,) -> Self {
        let packages = result.packages.iter().map(|(key, info)| (key.clone(), 
        LockedPackage { 
            name: info.name.clone(), 
            version: info.version.to_string(), 
            tarball_url: info.tarball_url.clone(), 
            integrity: info.integrity.clone(), dependencies: info.dependencies.iter().map(|(name, range)| (name.clone(), range.clone())).collect(), 
            optional_dependencies: info.optional_dependencies.iter().map(|(name, range)| (name.clone(), range.clone())).collect(), 
            peer_dependencies: info.peer_dependencies.iter().map(|(name, range)| (name.clone(), range.clone())).collect(), 
            optional_peers: info.optional_peers.iter().cloned().collect(), os: info.os.clone(), cpu: info.cpu.clone(), has_install_script: info.has_install_script, 
            resolved_deps: info.resolved_deps.iter().map(|(name, version)| (name.clone(), version.clone())).collect() })).collect();
        let root_resolved = result.root_resolved.iter().map(|(name, result_key)| (name.clone(), result_key.clone())).collect();
        Lockfile { lockfile_version: LOCKFILE_VERSION, root_deps: root_deps.clone(), root_resolved, packages }
    }


    pub fn write(&self, dir: &Path) -> Result<()> {
        let path = dir.join("vee.lock");
        let contents = serde_json::to_string_pretty(self).context("failed to serialise lockfile")?;
        std::fs::write(&path, contents).context("failed to write vee.lock")?;
        Ok(())
    }
}

impl Lockfile {
    pub fn read(dir: &Path) -> Result<Option<Self>> {
        let path = dir.join("vee.lock");
        if !path.exists() {return Ok(None);}
        let contents = std::fs::read_to_string(&path).context("failed to read vee.lock")?;
        let lockfile: Lockfile = serde_json::from_str(&contents).context("failed to parse vee.lock")?;
        Ok(Some(lockfile))
    }

    pub fn to_resolve_result(&self) -> Result<ResolveResult> {
        let packages = self.packages.iter().map(|(key, locked)| {
            let info = PackageInfo { 
                name: locked.name.clone(), 
                version: semver::Version::parse(&locked.version)?, 
                tarball_url: locked.tarball_url.clone(), 
                integrity: locked.integrity.clone(), 
                dependencies: locked.dependencies.iter().map(|(name, range)| (name.clone(), range.clone())).collect(), 
                optional_dependencies: locked.optional_dependencies.iter().map(|(name, range)| (name.clone(), range.clone())).collect(), 
                peer_dependencies: locked.peer_dependencies.iter().map(|(name, range)| (name.clone(), range.clone())).collect(), 
                optional_peers: locked.optional_peers.iter().cloned().collect(), os: locked.os.clone(), cpu: locked.cpu.clone(), 
                has_install_script: locked.has_install_script, 
                resolved_deps: locked.resolved_deps.iter().map(|(dep_name, result_key)| (dep_name.clone(), result_key.clone())).collect() };
            Ok((key.clone(), info))
        }).collect::<Result<HashMap<_, _>>>()?;
        let root_resolved = self.root_resolved.iter().map(|(name, result_key)| (name.clone(), result_key.clone())).collect();
        Ok(ResolveResult { packages, peer_warnings: Vec::new(), conflict_warnings: Vec::new(), root_resolved })
    }
}

impl Lockfile {pub fn is_current(&self, current_deps: &BTreeMap<String, String>) -> bool {&self.root_deps == current_deps}}
