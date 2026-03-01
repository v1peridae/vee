use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{Context, Result};
use crate::resolver::ResolveResult;
use crate::ui;

const LIFECYCLE_EVENTS: &[&str] = &["preinstall", "install", "postinstall"];

struct PackageScripts {
    name: String,
    version: String,
    scripts: HashMap<String, String>,
    package_dir: PathBuf,
}

fn read_package_scripts(package_dir: &Path) -> Result<HashMap<String, String>> {
    let package_json_path = package_dir.join("package.json");
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&package_json_path)?)?;
    let scripts = raw
        .get("scripts").and_then(|value| value.as_object()).map(|obj| {
            obj.iter()
                .filter_map(|(name, value)| {
                    value.as_str().map(|string| (name.clone(), string.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(scripts)
}

fn collect_lifecycle_packages(
    node_modules: &Path,
    result: &ResolveResult,
) -> Vec<PackageScripts> {
    let vee_dir = node_modules.join(".vee");
    let mut packages = Vec::new();
    for package in result.packages.values() {
        if !package.has_install_script {continue;}
        let package_dir = vee_dir.join(format!("{}@{}", package.name, package.version)).join("node_modules").join(&package.name);
        if let Ok(mut scripts) = read_package_scripts(&package_dir) {
            if !scripts.contains_key("install") && package_dir.join("binding.gyp").exists() {
                scripts.insert("install".to_string(), "node-gyp rebuild".to_string());
            }
            let has_lifecycle = scripts.keys().any(|name| LIFECYCLE_EVENTS.contains(&name.as_str()));
            if has_lifecycle {
                packages.push(PackageScripts { name: package.name.clone(), version: package.version.to_string(), scripts, package_dir });
            }
        }
    }

    packages
}

pub fn run_lifecycle_scripts(
    _project_dir: &Path,
    node_modules: &Path,
    result: &ResolveResult,
    verbose: bool,
) -> Result<()> {
    let packages = collect_lifecycle_packages(node_modules, result);
    if packages.is_empty() {return Ok(());}
    let bin_dir = node_modules.join(".bin");
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), current_path);
    let total = packages.len();
    let progress = ui::progress(total as u64, "running install scripts...");

    for package in &packages {
        for event in LIFECYCLE_EVENTS {
            if let Some(script_cmd) = package.scripts.get(*event) {
                if verbose {ui::info(&format!("running '{}' for {}@{}: {}", event, package.name, package.version, script_cmd));}
                let status = Command::new("sh").arg("-c").arg(script_cmd).current_dir(&package.package_dir).env("PATH", &new_path).env("npm_lifecycle_event", *event).env("npm_package_name", &package.name).env("npm_package_version", &package.version).env("npm_node_execpath", which_node().as_deref().unwrap_or("node")).stdout(if verbose { std::process::Stdio::inherit() } else { std::process::Stdio::null() }).stderr(if verbose { std::process::Stdio::inherit() } else { std::process::Stdio::null() }).status().with_context(|| format!("failed to execute '{}' for {}@{}", event, package.name, package.version))?;
                if !status.success() {ui::warn(&format!("{} for {}@{} failed with exit code {}", event, package.name, package.version, status.code().unwrap_or(-1)).to_lowercase());}
            }
        }
        progress.inc(1);
    }

    progress.finish_and_clear();
    ui::success(&format!("ran install scripts for {} {}", total, if total == 1 { "package" } else { "packages" }));
    Ok(())
}

fn which_node() -> Option<String> {
    Command::new("which")
        .arg("node")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|output| output.trim().to_string())
}
