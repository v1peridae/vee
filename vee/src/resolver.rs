pub mod semver_npm;
use crate::registry::npm::{NpmRegistry, PackageMetadata};
use anyhow::{Context, Result};
use semver_npm::NpmVersionReq;
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::task::JoinSet;

const MAX_CONCURRENCY: usize = 128;

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub name: String,
    pub version: semver::Version,
    pub tarball_url: String,
    pub integrity: String,
    pub dependencies: HashMap<String, String>,
    pub optional_dependencies: HashMap<String, String>,
    pub peer_dependencies: HashMap<String, String>,
    pub optional_peers: HashSet<String>,
    pub os: Option<Vec<String>>,
    pub cpu: Option<Vec<String>>,
    pub has_install_script: bool,
    pub resolved_deps: HashMap<String, String>,
}

#[derive(Debug)]
pub struct ResolveResult {
    pub packages: HashMap<String, PackageInfo>,
    pub peer_warnings: Vec<PeerWarning>,
    pub conflict_warnings: Vec<String>,
    pub root_resolved: HashMap<String, String>,
}

#[derive(Debug)]
pub struct PeerWarning {
    pub package: String,
    pub peer_name: String,
    pub required_range: String,
    pub found_version: Option<semver::Version>,
}

fn record_dep(
    result: &mut HashMap<String, PackageInfo>,
    root_resolved: &mut HashMap<String, String>,
    requester_key: &Option<String>,
    dep_name: &str,
    result_key: &str,
) {
    match requester_key {
        Some(requester_key) => {if let Some(package) = result.get_mut(requester_key) {
                package.resolved_deps.insert(dep_name.to_string(), result_key.to_string());
            }
        }
        None => {
            root_resolved.insert(dep_name.to_string(), result_key.to_string());
        }
    }
}

fn enqueue_all_deps(
    package: &PackageInfo,
    pending: &mut VecDeque<(String, String, Option<String>)>,
    known_optional: &mut HashSet<String>,
    requester_key: &str,
) {
    for (dep_name, dep_range) in &package.dependencies {
        pending.push_back((dep_name.clone(), dep_range.clone(), Some(requester_key.to_string())));
    }
    for (dep_name, dep_range) in &package.optional_dependencies {
        known_optional.insert(dep_name.clone());
        pending.push_back((dep_name.clone(), dep_range.clone(), Some(requester_key.to_string())));
    }
    for (peer_name, peer_range) in &package.peer_dependencies {
        if !package.optional_peers.contains(peer_name) {
            pending.push_back((
                peer_name.clone(),
                peer_range.clone(),
                Some(requester_key.to_string()),
            ));
        }
    }
}

pub async fn resolve(
    dependencies: &HashMap<String, String>,
    root_optional_names: &HashSet<String>,
    registry: &NpmRegistry,
) -> Result<ResolveResult> {
    resolve_with_metadata(dependencies, root_optional_names, registry, HashMap::new()).await
}



pub async fn resolve_with_metadata(
    dependencies: &HashMap<String, String>,
    root_optional_names: &HashSet<String>,
    registry: &NpmRegistry,
    initial_metadata: HashMap<String, PackageMetadata>,
) -> Result<ResolveResult> {
    let mut result: HashMap<String, PackageInfo> = HashMap::new();
    let mut resolved_vers: HashMap<String, Vec<(semver::Version, String)>> = HashMap::new();
    let mut metadata_cache: HashMap<String, PackageMetadata> = initial_metadata;
    let mut pending: VecDeque<(String, String, Option<String>)> = VecDeque::new();
    let mut known_optional: HashSet<String> = root_optional_names.clone();
    let mut conflict_warnings: Vec<String> = Vec::new();
    let mut root_resolved: HashMap<String, String> = HashMap::new();
    for (name, range) in dependencies {pending.push_back((name.clone(), range.clone(), None));}
    let mut waiting: HashMap<String, Vec<(String, Option<String>)>> = HashMap::new();
    let mut tasks: JoinSet<Result<(String, PackageMetadata)>> = JoinSet::new();
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENCY));
    loop {
        while let Some((name, range, requester_key)) = pending.pop_front() {
            if let Some(versions) = resolved_vers.get(&name) {
                let req = NpmVersionReq::parse(&range)?;
                if let Some((_, existing_key)) = versions.iter().find(|(version, _)| req.matches(version)) {
                    record_dep(
                        &mut result,
                        &mut root_resolved,
                        &requester_key,
                        &name,
                        existing_key,
                    );continue;
                }
            }

            if let Some(metadata) = metadata_cache.get(&name) {
                match best_version(&name, metadata, &range) {
                    Ok(package) => {
                        let result_key = format!("{}@{}", name, package.version);

                        if !result.contains_key(&result_key) {
                            enqueue_all_deps(&package, &mut pending, &mut known_optional, &result_key);
                            resolved_vers.entry(name.clone()).or_default().push((package.version.clone(), result_key.clone()));
                            result.insert(result_key.clone(), package);
                        }
                        record_dep(
                            &mut result,
                            &mut root_resolved,
                            &requester_key,
                            &name,
                            &result_key,
                        );
                    }
                    Err(error) if known_optional.contains(&name) => {
                        conflict_warnings.push(format!("skipping optional '{}': {}", name, error));
                    }
                    Err(error) => return Err(error),
                }
                continue;
            }

            if let Some(items) = waiting.get_mut(&name) {items.push((range, requester_key));
                continue;
            }
            waiting.insert(name.clone(), vec![(range, requester_key)]);
            let registry = registry.clone();
            let semaphore = semaphore.clone();
            let package_name = name.clone();
            tasks.spawn(async move {
                let _permit = semaphore.acquire().await.map_err(|error| anyhow::anyhow!(error))?;
                let metadata = registry.fetch_metadata(&package_name).await?;
                Ok((package_name, metadata))
            });
        }
        if tasks.is_empty() {break;}
        let task_result = tasks.join_next().await.expect("Tasks not empty");
        match task_result {
            Ok(Ok((name, metadata))) => {
                metadata_cache.insert(name.clone(), metadata);
                if let Some(items) = waiting.remove(&name) {
                    for (range, requester_key) in items {
                        pending.push_back((name.clone(), range, requester_key));
                    }
                }
            }
            Ok(Err(error)) => {
                let failed_name = find_failed_name(&waiting);
                if let Some(ref name) = failed_name {
                    if known_optional.contains(name.as_str()) {
                        conflict_warnings.push(format!("skipping optional '{}': {}", name, error));
                        waiting.remove(name.as_str());
                    } else {
                        return Err(error);
                    }
                } else {return Err(error);}
            }
            Err(join_err) => return Err(join_err.into()),
        }

        while let Some(task_res) = tasks.try_join_next() {
            match task_res {
                Ok(Ok((name, metadata))) => {
                    metadata_cache.insert(name.clone(), metadata);
                    if let Some(items) = waiting.remove(&name) {
                        for (range, requester_key) in items {
                            pending.push_back((name.clone(), range, requester_key));
                        }
                    }
                }
                Ok(Err(error)) => {
                    let failed_name = find_failed_name(&waiting);
                    if let Some(ref name) = failed_name {
                        if known_optional.contains(name.as_str()) {
                            conflict_warnings
                                .push(format!("skipping optional '{}': {}", name, error));
                            waiting.remove(name.as_str());
                        } else {return Err(error);}
                    } else {return Err(error);}
                }
                Err(join_err) => return Err(join_err.into()),
            }
        }
    }

    let mut peer_warnings = Vec::new();
    for package in result.values() {
        for (peer_name, peer_range) in &package.peer_dependencies {
            let req = NpmVersionReq::parse(peer_range)?;
            let is_optional = package.optional_peers.contains(peer_name);
            let resolved_version = package.resolved_deps.get(peer_name).and_then(|result_key| result.get(result_key)).map(|package| &package.version);
            match resolved_version {
                Some(version) if req.matches(version) => {}
                Some(version) => {
                    peer_warnings.push(PeerWarning { package: format!("{}@{}", package.name, package.version), peer_name: peer_name.clone(), required_range: peer_range.clone(), found_version: Some(version.clone()) });
                }
                None if !is_optional => {
                    peer_warnings.push(PeerWarning { package: format!("{}@{}", package.name, package.version), peer_name: peer_name.clone(), required_range: peer_range.clone(), found_version: None });
                }
                None => {}
            }
        }
    }

    Ok(ResolveResult { packages: result, peer_warnings, conflict_warnings, root_resolved })
}

fn best_version(name: &str, metadata: &PackageMetadata, range: &str) -> Result<PackageInfo> {
    let req = NpmVersionReq::parse(range)?;
    let mut matching: Vec<(semver::Version, &crate::registry::npm::VersionMetadata)> = metadata.versions.iter().filter_map(|(ver_str, ver_meta)| {
        let ver = semver::Version::parse(ver_str).ok()?;
        if req.matches(&ver) { Some((ver, ver_meta)) } else { None }
    }).collect();
    matching.sort_by(|(version_a, _), (version_b, _)| version_b.cmp(version_a));
    let (ver, ver_meta) = matching.into_iter().next().with_context(|| format!("no version of '{}' satisfies the range '{}'", name, range))?;
    let peer_dependencies = ver_meta.peer_dependencies.clone().unwrap_or_default();
    let optional_peers: HashSet<String> = ver_meta.peer_dependencies_meta.as_ref().map(|meta| meta.iter().filter(|(_, meta_entry)| meta_entry.optional).map(|(name, _)| name.clone()).collect()).unwrap_or_default();
    Ok(PackageInfo { name: name.to_string(), version: ver, tarball_url: ver_meta.dist.tarball.clone(), integrity: ver_meta.dist.integrity.clone(), dependencies: ver_meta.dependencies.clone().unwrap_or_default(), optional_dependencies: ver_meta.optional_dependencies.clone().unwrap_or_default(), peer_dependencies, optional_peers, os: ver_meta.os.clone(), cpu: ver_meta.cpu.clone(), has_install_script: ver_meta.has_install_script, resolved_deps: HashMap::new() })
}

fn find_failed_name(waiting: &HashMap<String, Vec<(String, Option<String>)>>) -> Option<String> {waiting.keys().next().cloned()}

pub fn platform_matches(package: &PackageInfo) -> bool {
    let current_os = std::env::consts::OS;
    let current_arch = std::env::consts::ARCH;
    let npm_os = match current_os {
        "macos" => "darwin",
        other => other,
    };
    let npm_cpu = match current_arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        other => other,
    };

    if let Some(ref os_list) = package.os {
        if !check_platform_list(os_list, npm_os) {
            return false;
        }}
    if let Some(ref cpu_list) = package.cpu {
        if !check_platform_list(cpu_list, npm_cpu) {
            return false;}
    }true
}

fn check_platform_list(list: &[String], current: &str) -> bool {
    let has_negations = list.iter().any(|item| item.starts_with('!'));
    if has_negations {
        !list.contains(&format!("!{}", current))
    } else {
        list.iter().any(|item| item == current)
    }
}
