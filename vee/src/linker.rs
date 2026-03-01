use std::collections::HashMap;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::{Context, Result};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use crate::cache::CacheStore;
use crate::resolver::{PackageInfo, ResolveResult};

pub fn fingerprint(result: &ResolveResult) -> String {
    let mut hasher = Sha256::new();
    let mut keys: Vec<&String> = result.packages.keys().collect();
    keys.sort();
    for key in keys {
        let package = &result.packages[key];
        hasher.update(format!("{}:{}\n", key, package.integrity).as_bytes());
    }
    hex::encode(hasher.finalize())
}

pub fn check_fingerprint(node_modules: &Path, expected: &str) -> bool {
    let fingerprint_path = node_modules.join(".vee").join(".fingerprint");
    match fs::read_to_string(fingerprint_path) {
        Ok(content) => content.trim() == expected,
        Err(_) => false,
    }
}

pub fn write_fingerprint(node_modules: &Path, fingerprint: &str) -> Result<()> {
    let vee_dir = node_modules.join(".vee");
    fs::create_dir_all(&vee_dir)?;
    fs::write(vee_dir.join(".fingerprint"), fingerprint)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn try_clonefile(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int};
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        unsafe fn clonefile(src: *const c_char, dst: *const c_char, flags: c_int) -> c_int;
    }
    

    let src_c = CString::new(src.as_os_str().as_bytes())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let dst_c = CString::new(dst.as_os_str().as_bytes())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;

    let result = unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn copy_package_dir(src: &Path, dst: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        match try_clonefile(src, dst) {
            Ok(()) => return Ok(()),
            Err(error) if error.raw_os_error() == Some(17) => return Ok(()), 
            Err(_) => {}
        }
    }
    fs::create_dir_all(dst)?;
    hardlink_dir(src, dst)
}

pub struct Linker { project_dir: PathBuf, cache: Arc<CacheStore>}
impl Linker {
    pub fn new(project_dir: PathBuf, cache: Arc<CacheStore>) -> Self {Self { project_dir, cache }}
    pub fn link(&self, result: &ResolveResult, root_deps: &HashMap<String, String>) -> Result<()> {
        let node_modules = self.project_dir.join("node_modules");
        let vee_dir = node_modules.join(".vee");

        if node_modules.exists() {fs::remove_dir_all(&node_modules)?;}
        fs::create_dir_all(&vee_dir)?;
        let cache = &self.cache;
        result.packages.par_iter()
            .try_for_each(|(key, package)| -> Result<()> {
                let cache_path = cache.get(&package.integrity)?.context(format!("package {} not in cache", key))?;
                let package_vstore = vee_dir.join(format!("{}@{}", package.name, package.version)).join("node_modules").join(&package.name);


                if let Some(parent) = package_vstore.parent() {
                    fs::create_dir_all(parent)?;
                }
                copy_package_dir(&cache_path, &package_vstore)?;
                Ok(())
            })?;

        for package in result.packages.values() {
            let package_node_modules = vee_dir.join(format!("{}@{}", package.name, package.version)).join("node_modules");
            for dep_name in package.dependencies.keys().chain(package.optional_dependencies.keys())
            {let dep_key = match package.resolved_deps.get(dep_name) {
                    Some(dep_key) => dep_key,
                    None => continue,
                };
                let dep_package = match result.packages.get(dep_key) {
                    Some(dep_package) => dep_package,
                    None => continue,
                };

                let target = vee_dir.join(format!("{}@{}", dep_package.name, dep_package.version)).join("node_modules").join(&dep_package.name);
                let link_path = package_node_modules.join(dep_name);
                if !link_path.exists() {
                    if let Some(parent) = link_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let link_parent = link_path.parent().unwrap_or(&package_node_modules);
                    let relative_sym =
                        pathdiff::diff_paths(&target, link_parent).unwrap_or(target);
                    unix_fs::symlink(&relative_sym, &link_path)?;
                }
            }

            for peer_name in package.peer_dependencies.keys() {
                let link_path = package_node_modules.join(peer_name);
                if link_path.exists() {continue;}
                let peer_key = match package.resolved_deps.get(peer_name) {
                    Some(peer_key) => peer_key,
                    None => continue,
                };
                let peer_package = match result.packages.get(peer_key) {
                    Some(peer_package) => peer_package,
                    None => continue,
                };

                let target = vee_dir.join(format!("{}@{}", peer_package.name, peer_package.version)).join("node_modules").join(&peer_package.name);
                if let Some(parent) = link_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let link_parent = link_path.parent().unwrap_or(&package_node_modules);
                let relative_sym =
                    pathdiff::diff_paths(&target, link_parent).unwrap_or(target);
                unix_fs::symlink(&relative_sym, &link_path)?;
            }
        }

        for name in root_deps.keys() {
            let result_key = match result.root_resolved.get(name) {
                Some(result_key) => result_key,
                None => continue,
            };
            let package = match result.packages.get(result_key) {
                Some(package) => package,
                None => continue,
            };

            let target = vee_dir.join(format!("{}@{}", package.name, package.version)).join("node_modules").join(&package.name);
            let link_path = node_modules.join(name);
            if let Some(parent) = link_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let link_parent = link_path.parent().unwrap_or(&node_modules);
            let relative_sym = pathdiff::diff_paths(&target, link_parent).unwrap_or(target);
            unix_fs::symlink(&relative_sym, &link_path)?;
        }

        self.link_bins(&node_modules, result, root_deps)?;
        Ok(())
    }

    pub fn link_flat(
        &self,
        result: &ResolveResult,
        root_deps: &HashMap<String, String>,
    ) -> Result<()> {
        let node_modules = self.project_dir.join("node_modules");
        if node_modules.exists() {
            fs::remove_dir_all(&node_modules)?;
        }
        fs::create_dir_all(&node_modules)?;
        let cache = &self.cache;
        let mut by_name: HashMap<&str, &PackageInfo> = HashMap::new();
        for package in result.packages.values() {
            match by_name.get(package.name.as_str()) {
                Some(existing) if existing.version > package.version => {}
                _ => {by_name.insert(&package.name, package);}
            }
        }

        let packages: Vec<_> = by_name.into_values().collect();
        packages.par_iter().try_for_each(|package| -> Result<()> {
                let cache_path = cache.get(&package.integrity)?.context(format!("package {} not in cache", package.name))?;

                let package_dir = node_modules.join(&package.name);
                if let Some(parent) = package_dir.parent() {
                    fs::create_dir_all(parent)?;
                }
                copy_package_dir(&cache_path, &package_dir)?;
                Ok(())
            })?;

        self.link_bins(&node_modules, result, root_deps)?;
        Ok(())
    }

    fn link_bins(
        &self,
        node_modules: &Path,
        result: &ResolveResult,
        root_deps: &HashMap<String, String>,
    ) -> Result<()> {
        let bin_dir = node_modules.join(".bin");

        for name in root_deps.keys() {
            let result_key = match result.root_resolved.get(name) {
                Some(result_key) => result_key,
                None => continue,
            };
            if !result.packages.contains_key(result_key) {
                continue;
            }

            let package_dir = node_modules.join(name);
            let package_json_path = package_dir.join("package.json");
            if !package_json_path.exists() {
                continue;
            }
            let raw: serde_json::Value =serde_json::from_str(&std::fs::read_to_string(&package_json_path)?)?;
            match raw.get("bin") {
                Some(serde_json::Value::String(bin_path_str)) => {
                    fs::create_dir_all(&bin_dir)?;
                    let target = package_dir.join(bin_path_str);
                    let link = bin_dir.join(name);
                    let relative_sym =
                        pathdiff::diff_paths(&target, &bin_dir).unwrap_or(target);
                    unix_fs::symlink(&relative_sym, &link)?;
                    set_executable(&link)?;
                }
                Some(serde_json::Value::Object(map)) => {
                    fs::create_dir_all(&bin_dir)?;
                    for (bin_name, bin_path) in map {
                        if let Some(bin_path_str) = bin_path.as_str() {
                            let target = package_dir.join(bin_path_str);
                            let link = bin_dir.join(bin_name);
                            let relative_sym =
                                pathdiff::diff_paths(&target, &bin_dir).unwrap_or(target);
                            unix_fs::symlink(&relative_sym, &link)?;
                            set_executable(&link)?;
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}

fn hardlink_dir(src: &Path, dst: &Path) -> Result<()> {
    let entries: Vec<_> = fs::read_dir(src)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.par_iter().try_for_each(|entry| -> Result<()> {
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_symlink() {return Ok(());}
        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            hardlink_dir(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::hard_link(&src_path, &dst_path)?;
        }
        Ok(())
    })
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let target = fs::canonicalize(path)?;
    let mut perms = fs::metadata(&target)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(target, perms)?;
    Ok(())
}
