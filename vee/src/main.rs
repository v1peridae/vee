mod cache;
mod linker;
mod lockfile;
mod package_json;
mod registry;
mod resolver;
mod ui;
mod scripts;
use clap::{Parser, Subcommand};
use lockfile::Lockfile;
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

#[derive(Parser)]
#[command(name = "vee")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "v1peridae")]
#[command(about = "a fast and lightweight rust-based package manager for javascript :3")]
struct Cli {
    #[arg(short, long, global = true)]
    verbose: bool,
    #[arg(short = 'S', long, global = true)]
    simulate: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(alias = "i")]
    Install {
        package: Option<String>,
        #[arg(short = 'P', long)]
        production: bool,
        #[arg(long)]
        frozen_lockfile: bool,
        #[arg(long)]
        ignore_scripts: bool,
    },
    Add {
        packages: Vec<String>,
        #[arg(short = 'D', long = "dev")]
        dev: bool,
        #[arg(long)]
        version: Option<String>,
    },

    Remove {packages: Vec<String>},
    Update {packages: Vec<String>},
    #[command(alias = "ls")]
    List {
        #[arg(long)]
        prod: bool,
        #[arg(long)]
        dev: bool,
        #[arg(short, long)]
        tree: bool,
    },
    Run {
        script: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Create {
        package: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(alias = "dlx")]
    Exec {
        package: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Init {
        #[arg(short, long)]
        yes: bool,
    },
    Outdated,
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Subcommand)]
enum CacheAction {Clean, Info}
 

fn format_duration(duration: std::time::Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1000 {format!("{}ms", ms)} else {format!("{:.1}s", duration.as_secs_f64())}
}
fn plural_package(n: usize) -> &'static str {if n == 1 {"package"} else {"packages"}}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli).await {
        let msg = format!("{:#}", error);
        if !msg.contains("broken pipe") {
            ui::error(&msg);
            std::process::exit(1);
        }
    }
}

#[derive(Default)]
struct InstallOptions {
    production: bool,
    frozen_lockfile: bool,
    ignore_scripts: bool,
    verbose: bool,
    simulate: bool,
}

async fn handle_install(opts: &InstallOptions) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let package = package_json::PackageJson::load(std::path::Path::new("."))?;
    let project_dir = package.directory().to_path_buf();
    let registry = registry::npm::NpmRegistry::from_project_dir(&project_dir)?;
    let mut deps_to_resolve: std::collections::HashMap<String, String> =
        package.dependencies.clone().into_iter().collect();
    if !opts.production {deps_to_resolve.extend(package.dev_dependencies.clone());}
    let optional_names: HashSet<String> = package.optional_dependencies.keys().cloned().collect();
    deps_to_resolve.extend(package.optional_dependencies.clone());

    let root_deps: BTreeMap<String, String> = deps_to_resolve
        .iter()
        .map(|(name, range)| (name.clone(), range.clone()))
        .collect();

    let resolved = match Lockfile::read(&project_dir)? {
        Some(lockfile) if lockfile.is_current(&root_deps) => {
            ui::info("using existing vee.lock");
            lockfile.to_resolve_result()?
        }
        existing => {
            if opts.frozen_lockfile {
                if existing.is_none() {
                    return Err(anyhow::anyhow!("--frozen-lockfile: no vee.lock found").into());
                }
                return Err(anyhow::anyhow!("--frozen-lockfile: vee.lock is out of date").into());
            }
            let resolve_spinner = ui::spinner(&format!("resolving {} dependencies...", deps_to_resolve.len()));
            let result =
                resolver::resolve(&deps_to_resolve, &optional_names, &registry).await?;
            let lockfile = Lockfile::from_resolve_result(&result, &root_deps);
            lockfile.write(&project_dir)?;
            resolve_spinner.finish_and_clear();
            for warning in &result.conflict_warnings {
                ui::warn(warning);
            }
            ui::success(&format!("resolved {} {}", result.packages.len(), plural_package(result.packages.len())));
            result
        }
    };

    for warning in &resolved.peer_warnings {
        match &warning.found_version {
            Some(version) => ui::warn(&format!("{} requires peer {} '{}', but {} is resolved", warning.package, warning.peer_name, warning.required_range, version).to_lowercase()),
            None => ui::warn(&format!("{} requires peer {} '{}' which is not installed", warning.package, warning.peer_name, warning.required_range).to_lowercase()),
        }
    }

    if opts.simulate {
        ui::info(&format!("[SIM] would install {} {}", resolved.packages.len(), plural_package(resolved.packages.len())));
        return Ok(());
    }

    let node_modules = project_dir.join("node_modules");
    let fingerprint = linker::fingerprint(&resolved);
    if linker::check_fingerprint(&node_modules, &fingerprint) {
        ui::success(&format!("already up to date ({} packages)", resolved.packages.len()));
        return Ok(());
    }

    let optional_keys: HashSet<String> = resolved.packages.iter().filter(|(_key, package)| optional_names.contains(&package.name)).map(|(key, _)| key.clone()).collect();
    let cache = std::sync::Arc::new(cache::CacheStore::new()?);
    let client = &registry.client;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(128));
    let mut failed_optional: HashSet<String> = HashSet::new();
    let mut tasks = tokio::task::JoinSet::new();
    for (key, package) in &resolved.packages {
        if optional_keys.contains(key) && !resolver::platform_matches(package) {
            ui::warn(&format!("skipping {} (platform mismatch)", key));
            failed_optional.insert(key.clone());
            continue;
        }

        let cache = cache.clone();
        let semaphore = semaphore.clone();
        let client = client.clone();
        let key = key.clone();
        let is_optional = optional_keys.contains(&key);
        let integrity = package.integrity.clone();
        let tarball_url = package.tarball_url.clone();
        let auth_header = registry.auth_header_for_url(&tarball_url);
        tasks.spawn(async move {
            let _permit = semaphore.acquire().await.map_err(|error| anyhow::anyhow!(error))?;
            match cache.ensure(&integrity, &tarball_url, &client, auth_header).await {
                Ok(path) => Ok((key, Some(path), false)),
                Err(_) if is_optional => {
                    Ok((key, None, true))
                }
                Err(error) => Err(error),
            }
        });
    }

    let total = resolved.packages.len() as u64;
    let fetch_progress = ui::progress(total, "fetching packages...");
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok((key, Some(cached_path), _))) => {
                fetch_progress.set_message(key.clone());
                fetch_progress.inc(1);
                if opts.verbose {
                    fetch_progress.println(format!("  cached {} -> {}", key, cached_path.display()));
                }
            }
            Ok(Ok((key, None, true))) => {
                fetch_progress.inc(1);
                ui::warn(&format!("skipping optional dependency {}", key));
                failed_optional.insert(key);
            }
            Ok(Ok((_, None, false))) => unreachable!(),
            Ok(Err(error)) => return Err(error.into()),
            Err(error) => return Err(error.into()),
        }
    }
    fetch_progress.finish_and_clear();
    let fetched_count = resolved.packages.len() - failed_optional.len();
    if !failed_optional.is_empty() {
        ui::success(&format!("fetched {} packages ({} optional skipped)", fetched_count, failed_optional.len()));
    } else {ui::success(&format!("fetched {} packages", fetched_count));}

    let mut resolved_for_linking = resolved;
    for key in &failed_optional {resolved_for_linking.packages.remove(key);}
    let link_spinner = ui::spinner("linking packages...");
    let linker_inst = linker::Linker::new(project_dir.clone(), cache);
    linker_inst.link(&resolved_for_linking, &deps_to_resolve)?;
    link_spinner.finish_and_clear();
    if !opts.ignore_scripts {scripts::run_lifecycle_scripts(&project_dir, &node_modules, &resolved_for_linking, opts.verbose)?;}
    linker::write_fingerprint(&node_modules, &fingerprint)?;
    let elapsed = format_duration(started.elapsed());
    ui::success(&format!("linked {} packages into node_modules/ ({})", resolved_for_linking.packages.len(), elapsed));
    Ok(())
}

async fn handle_add(
    packages: Vec<String>,
    dev: bool,
    _version: Option<String>,
    verbose: bool,
    simulate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut package = package_json::PackageJson::load(std::path::Path::new("."))?;
    let project_dir = package.directory().to_path_buf();
    let registry = registry::npm::NpmRegistry::from_project_dir(&project_dir)?;
    for raw in &packages {
        let (name, version) = if let Some(idx) = raw.rfind('@') {
            if idx == 0 {
                (raw.as_str(), "latest".to_string())
            } else {
                (&raw[..idx], raw[idx + 1..].to_string())
            }
        } else {
            (raw.as_str(), "latest".to_string())
        };
        let version_spinner = ui::spinner(&format!("resolving {}...", name));
        let resolved_version = if version == "latest" {
            let latest = registry.latest_version(name).await?;
            format!("^{}", latest)
        } else {
            version
        };
        version_spinner.finish_and_clear();

        if simulate {ui::info(&format!("[SIM] would add {}@{}", name, resolved_version));
            continue;
        }

        let dep_type = if dev { "devDependencies" } else { "dependencies" };
        ui::info(&format!("adding {}@{} to {}", name, resolved_version, dep_type));
        package.add_dep(name, &resolved_version, dev);
    }
    package.save()?;
    if simulate {
        return Ok(());
    }

    handle_install(&InstallOptions { verbose, ..Default::default() }).await?;
    Ok(())
}

fn handle_run(
    script: String,
    args: Vec<String>,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = package_json::ScriptsManifest::load(std::path::Path::new("."))?;
    let project_dir = manifest.directory().to_path_buf();
    let script_cmd = manifest.scripts.get(&script).ok_or_else(|| {
        let available: Vec<&String> = manifest.scripts.keys().collect();
        if available.is_empty() {
            anyhow::anyhow!("no scripts defined in package.json")
        } else {
            anyhow::anyhow!(
                "script '{}' not found. available: {}",
                script,
                available.iter().map(|name| name.as_str()).collect::<Vec<_>>().join(", ")
            )
        }
    })?;

    let full_cmd = if args.is_empty() {
        script_cmd.clone()
    } else {
        let escaped: Vec<String> = args.iter().map(|arg| {
            format!("'{}'", arg.replace('\'', "'\\''"))
        }).collect();
        format!("{} {}", script_cmd, escaped.join(" "))
    };

    let bin_dir = project_dir.join("node_modules").join(".bin");
    let current_path = env!("PATH");
    let new_path = format!("{}:{}", bin_dir.display(), current_path);
    let package_name = manifest.name.as_deref().unwrap_or("");
    let package_version = manifest.version.as_deref().unwrap_or("");
    let vee_execpath = std::env::current_exe().map(|path| path.to_string_lossy().to_string()).unwrap_or_default();
    let user_agent = format!("vee/{}", env!("CARGO_PKG_VERSION"));
    let make_cmd = |cmd_str: &str, lifecycle_event: &str| -> std::process::Command {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(cmd_str)
            .current_dir(&project_dir)
            .env("PATH", &new_path)
            .env("npm_lifecycle_event", lifecycle_event)
            .env("npm_package_name", package_name)
            .env("npm_package_version", package_version)
            .env("npm_execpath", &vee_execpath)
            .env("npm_config_user_agent", &user_agent)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
        cmd
    };

    let pre_key = format!("pre{}", script);
    let post_key = format!("post{}", script);
    let pre_cmd = manifest.scripts.get(&pre_key).cloned();
    let post_cmd = manifest.scripts.get(&post_key).cloned();
    if let Some(ref pre) = pre_cmd {
        if verbose {
            ui::info(&format!("running '{}': {}", pre_key, pre));
        }
        let status = make_cmd(pre, &pre_key)
            .status()
            .map_err(|error| anyhow::anyhow!("failed to execute '{}': {}", pre_key, error))?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
    }

    if verbose {ui::info(&format!("running '{}': {}", script, full_cmd));}

    if let Some(ref post) = post_cmd {
        let status = make_cmd(&full_cmd, &script)
            .status()
            .map_err(|error| anyhow::anyhow!("failed to execute '{}': {}", script, error))?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        if verbose {
            ui::info(&format!("running '{}': {}", post_key, post));
        }
        exec_or_spawn(&mut make_cmd(post, &post_key))?;
    } else {
        exec_or_spawn(&mut make_cmd(&full_cmd, &script))?;
    }

    Ok(())
}

#[cfg(unix)]
fn exec_or_spawn(cmd: &mut std::process::Command) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::process::CommandExt;
    let error = cmd.exec();
    Err(anyhow::anyhow!("failed to execute: {}", error).into())
}


async fn handle_remove(
    packages: Vec<String>,
    verbose: bool,
    simulate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut package = package_json::PackageJson::load(std::path::Path::new("."))?;
    for name in &packages {
        if !package.has_dep(name) {
            return Err(
                anyhow::anyhow!("package '{}' not found in dependencies", name).into(),
            );
        }
        if simulate {
            ui::info(&format!("[SIM] would remove {}", name));
            continue;
        }
        ui::info(&format!("removing {}", name));
        package.remove_dep(name);
    } package.save()?;

    if simulate {return Ok(());}
    ui::success(&format!("removed {} from package.json", packages.join(", ")));
    handle_install(&InstallOptions { verbose, ..Default::default() }).await?;
    Ok(())
}

async fn handle_update(
    packages: Vec<String>,
    _verbose: bool,
    simulate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let package = package_json::PackageJson::load(std::path::Path::new("."))?;
    let project_dir = package.directory().to_path_buf();
    let registry = registry::npm::NpmRegistry::from_project_dir(&project_dir)?;
    let mut deps_to_resolve: std::collections::HashMap<String, String> = package
        .dependencies
        .iter()
        .chain(package.dev_dependencies.iter())
        .map(|(name, version)| (name.clone(), version.clone()))
        .collect();
    let optional_names: HashSet<String> = package.optional_dependencies.keys().cloned().collect();
    deps_to_resolve.extend(package.optional_dependencies.clone());

    if !packages.is_empty() {
        for name in &packages {
            if !deps_to_resolve.contains_key(name) {
                return Err(anyhow::anyhow!("package {} not found in dependencies", name).into());
            }
        }
    }
    let updating_names: Vec<&str> = if packages.is_empty() {
        deps_to_resolve.keys().map(|item| item.as_str()).collect()
    } else {
        packages.iter().map(|item| item.as_str()).collect()
    };

    if simulate {
        for name in &updating_names {ui::info(&format!("[SIM] would update {}", name));}
        return Ok(());
    }

    let resolve_spinner = ui::spinner(&format!("resolving {} dependencies...", deps_to_resolve.len()));
    let resolved = resolver::resolve(&deps_to_resolve, &optional_names, &registry).await?;
    resolve_spinner.finish_and_clear();

    let old_lockfile = lockfile::Lockfile::read(&project_dir)?;
    let root_deps: BTreeMap<String, String> = deps_to_resolve
        .iter()
        .map(|(name, range)| (name.clone(), range.clone()))
        .collect();
    let new_lockfile = Lockfile::from_resolve_result(&resolved, &root_deps);

    let mut updated_count = 0u32;
    for name in &updating_names {
        let old_ver = old_lockfile.as_ref().and_then(|lock| {
            lock.root_resolved
                .get(*name)
                .and_then(|key| lock.packages.get(key))
                .map(|package| package.version.clone())
        });
        let new_ver = new_lockfile
            .root_resolved
            .get(*name)
            .and_then(|key| new_lockfile.packages.get(key))
            .map(|package| package.version.clone());
        match (old_ver, new_ver) {
            (Some(ref old), Some(ref new)) if old != new => {
                updated_count += 1;
                ui::info(&format!("updating {} from {} to {}", name, old, new));
            }
            (None, Some(ref new)) => {
                updated_count += 1;
                ui::info(&format!("{}: (new) {}", name, new));
            }
            _ => {}
        }
    }

    if updated_count == 0 {ui::info("all packages are up to date. no updates needed");} 
    else {ui::success(&format!("updated {} packages", updated_count));}
    new_lockfile.write(&project_dir)?;
    handle_install(&InstallOptions { verbose: _verbose, ..Default::default() }).await?;
    Ok(())
}

fn to_create_package_name(input: &str) -> String {
    if let Some(rest) = input.strip_prefix('@') {
        if let Some((scope, name)) = rest.split_once('/') {
            return format!("@{}/create-{}", scope, name);
        }
    }
    format!("create-{}", input)
}

async fn handle_create(
    package: String,
    args: Vec<String>,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (raw_name, version) = parse_package_spec(&package);
    let create_package_name = to_create_package_name(&raw_name);

    let project_dir = std::path::Path::new(".");
    let registry = registry::npm::NpmRegistry::from_project_dir(project_dir)?;

    let version_spinner = ui::spinner(&format!("resolving {}...", create_package_name));
    let (resolved_version, initial_metadata) = if version == "latest" {
        let (ver, meta) = registry.latest_version_with_metadata(&create_package_name).await?;
        (
            format!("^{}", ver),
            std::collections::HashMap::from([(create_package_name.clone(), meta)]),
        )
    } else {
        (version, std::collections::HashMap::new())
    };
    version_spinner.finish_and_clear();
    ui::info(&format!("using {}@{}", create_package_name, resolved_version));

    let deps: std::collections::HashMap<String, String> =
        [(create_package_name.clone(), resolved_version)].into_iter().collect();
    let optional_names: HashSet<String> = HashSet::new();

    let resolve_spinner = ui::spinner("resolving dependencies...");
    let resolved = resolver::resolve_with_metadata(&deps, &optional_names, &registry, initial_metadata).await?;
    resolve_spinner.finish_and_clear();
    ui::success(&format!("resolved {} packages", resolved.packages.len()));

    let cache = std::sync::Arc::new(cache::CacheStore::new()?);
    let client = &registry.client;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(128));

    let mut tasks = tokio::task::JoinSet::new();
    for (key, package) in &resolved.packages {
        let cache = cache.clone();
        let semaphore = semaphore.clone();
        let client = client.clone();
        let key = key.clone();
        let integrity = package.integrity.clone();
        let tarball_url = package.tarball_url.clone();
        let auth_header = registry.auth_header_for_url(&tarball_url);
        tasks.spawn(async move {
            let _permit = semaphore.acquire().await.map_err(|error| anyhow::anyhow!(error))?;
            let path = cache.ensure(&integrity, &tarball_url, &client, auth_header).await?;
            Ok::<_, anyhow::Error>((key, path))
        });
    }

    let total = resolved.packages.len() as u64;
    let fetch_progress = ui::progress(total, "fetching packages...");
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok((key, _path))) => {
                fetch_progress.set_message(key);
                fetch_progress.inc(1);
            }
            Ok(Err(error)) => return Err(error.into()),
            Err(error) => return Err(error.into()),
        }
    }
    fetch_progress.finish_and_clear();
    ui::success(&format!("fetched {} packages", resolved.packages.len()));
    let temp_dir = tempfile::tempdir()?;
    let temp_path = temp_dir.path().to_path_buf();
    let link_spinner = ui::spinner("linking packages...");
    let linker_inst = linker::Linker::new(temp_path.clone(), cache);
    linker_inst.link_flat(&resolved, &deps)?;
    link_spinner.finish_and_clear();
    let node_modules = temp_path.join("node_modules");
    let bin_dir = node_modules.join(".bin");
    let bin_name = create_package_name.split('/').next_back().unwrap_or(&create_package_name);
    let bin_path = bin_dir.join(bin_name);
    if !bin_path.exists() {
        let entries: Vec<String> = std::fs::read_dir(&bin_dir).ok().map(|read_dir| read_dir.filter_map(|entry| entry.ok()).map(|entry| entry.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();

        if entries.len() == 1 {
            let fallback = bin_dir.join(&entries[0]);
            if verbose {
                ui::info(&format!("using bin: {}", entries[0]));
            }
            return run_create_bin(&fallback, &args, &node_modules);
        }

        return Err(anyhow::anyhow!("failed to find bin '{}' in {}. available: {}", bin_name, bin_dir.display(), entries.join(", ")).into());
    }

    run_create_bin(&bin_path, &args, &node_modules)
}

fn run_create_bin(
    bin_path: &std::path::Path,
    args: &[String],
    node_modules: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let status = std::process::Command::new(bin_path).args(args).env("NODE_PATH", node_modules).stdin(std::process::Stdio::inherit()).stdout(std::process::Stdio::inherit()).stderr(std::process::Stdio::inherit()).status().map_err(|error| anyhow::anyhow!("failed to execute {}: {}", bin_path.display(), error))?;

    if !status.success() {std::process::exit(status.code().unwrap_or(1));}
    Ok(())
}

fn parse_package_spec(input: &str) -> (String, String) {
    if let Some(scoped) = input.strip_prefix('@') {
        if let Some((scope_and_name, version)) = scoped.rsplit_once('@') {
            (format!("@{}", scope_and_name), version.to_string())
        } else {
            (input.to_string(), "latest".to_string())
        }
    } else if let Some((name, version)) = input.rsplit_once('@') {
        (name.to_string(), version.to_string())
    } else {
        (input.to_string(), "latest".to_string())
    }
}

fn handle_init(yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let package_path = cwd.join("package.json");
    if package_path.exists() {
        return Err(anyhow::anyhow!("package.json already exists").into());
    }

    let dir_name = cwd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "my-package".to_string());
    let (name, version, description, main, license) = if yes {
        (dir_name, "1.0.0".to_string(), String::new(), "index.js".to_string(), "ISC".to_string())
    } else {
        let name = prompt_with_default("package name", &dir_name);
        let version = prompt_with_default("version", "1.0.0");
        let description = prompt_with_default("description", "");
        let main = prompt_with_default("entry point", "index.js");
        let license = prompt_with_default("license", "ISC");
        (name, version, description, main, license)
    };

    let mut package = serde_json::Map::new();
    package.insert("name".into(), serde_json::Value::String(name));
    package.insert("version".into(), serde_json::Value::String(version));
    if !description.is_empty() {package.insert("description".into(), serde_json::Value::String(description));}
    package.insert("main".into(), serde_json::Value::String(main));
    package.insert("scripts".into(), serde_json::json!({"test": "echo \"Error: no test specified\" && exit 1"}));
    package.insert("license".into(), serde_json::Value::String(license));
    let json = serde_json::to_string_pretty(&package)?;
    std::fs::write(&package_path, format!("{}\n", json))?;
    ui::success(&format!("created {}", package_path.display()));
    Ok(())
}

fn prompt_with_default(prompt: &str, default: &str) -> String {
    use std::io::Write;
    if default.is_empty() {
        eprint!("{}: ", prompt);
    } else {
        eprint!("{} ({}): ", prompt, default);
    }
    std::io::stderr().flush().ok();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

async fn handle_outdated(verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    let package = package_json::PackageJson::load(std::path::Path::new("."))?;
    let project_dir = package.directory().to_path_buf();
    let registry = registry::npm::NpmRegistry::from_project_dir(&project_dir)?;
    let lockfile = Lockfile::read(&project_dir)?;
    let all_deps: BTreeMap<String, String> = package.dependencies.iter()
        .chain(package.dev_dependencies.iter())
        .chain(package.optional_dependencies.iter())
        .map(|(n, v)| (n.clone(), v.clone()))
        .collect();

    if all_deps.is_empty() {ui::info("no dependencies found");
        return Ok(());
    }

    let spinner = ui::spinner("checking for updates...");
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    for (name, range) in &all_deps {
        let current = lockfile.as_ref()
            .and_then(|lf| lf.root_resolved.get(name))
            .and_then(|key| lf_version(&lockfile, key));


        match registry.latest_version(name).await {
            Ok(latest) => {
                let current_str = current.as_deref().unwrap_or("???");
                if current_str != latest || verbose {
                    rows.push((name.clone(), current_str.to_string(), range.clone(), latest));
                }
            }
            Err(_) => {
                if verbose {
                    ui::warn(&format!("failed to check {}", name));
                }
            }
        }
    } spinner.finish_and_clear();

    if rows.is_empty() {ui::success("all packages are up to date!");
        return Ok(());
    }

    let w_name = rows.iter().map(|row| row.0.len()).max().unwrap_or(7).max(7);
    let w_cur = rows.iter().map(|row| row.1.len()).max().unwrap_or(7).max(7);
    let w_wanted = rows.iter().map(|row| row.2.len()).max().unwrap_or(6).max(6);
    let w_latest = rows.iter().map(|row| row.3.len()).max().unwrap_or(6).max(6);

    println!("{:<w_name$}  {:<w_cur$}  {:<w_wanted$}  {:<w_latest$}",
        "package", "current", "wanted", "latest");
    for (name, current, wanted, latest) in &rows {
        let is_outdated = current != latest;
        if is_outdated {
            println!("{} {:<w_name$}  {:<w_cur$}  {:<w_wanted$}  {:<w_latest$}",
                console::style("▸").yellow(), name, current, wanted, latest);
        } else {
            println!("  {:<w_name$}  {:<w_cur$}  {:<w_wanted$}  {:<w_latest$}",
                name, current, wanted, latest);
        }
    }

    let outdated_count = rows.iter().filter(|(_, current, _, latest)| current != latest).count();
    if outdated_count > 0 {ui::info(&format!("{} outdated packages", outdated_count));}
    Ok(())
}

fn lf_version(lockfile: &Option<Lockfile>, key: &str) -> Option<String> {
    lockfile.as_ref()?.packages.get(key).map(|package| package.version.clone())
}

fn handle_list(
    prod_only: bool,
    dev_only: bool,
    tree: bool,
    _verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let package = package_json::PackageJson::load(std::path::Path::new("."))?;
    let project_dir = package.directory().to_path_buf();
    let lockfile = Lockfile::read(&project_dir)?;
    let show_prod = !dev_only;
    let show_dev = !prod_only;



    if tree {
        if let Some(ref lf) = lockfile {
            if show_prod && !package.dependencies.is_empty() {
                println!("{}", console::style("dependencies:".to_lowercase()).bold());
                print_dep_tree(&package.dependencies, lf, 1);
            }
            if show_dev && !package.dev_dependencies.is_empty() {
                println!("{}", console::style("devdependencies:").bold());
                print_dep_tree(&package.dev_dependencies, lf, 1);
            }
        } else {
            ui::warn("no vee.lock found. run `vee install` first.");
        }
        return Ok(());
    }

    let mut count = 0usize;
    if show_prod && !package.dependencies.is_empty() {
        println!("{}", console::style("dependencies:".to_lowercase()).bold());
        for (name, range) in &package.dependencies {
            let version = lockfile.as_ref()
                .and_then(|lf| lf.root_resolved.get(name))
                .and_then(|key| lf_version(&lockfile, key))
                .unwrap_or_else(|| "???".to_string());
            println!("  {} {} {}", console::style(name).cyan(), console::style(&version).green(), console::style(format!("({})", range)).dim());
            count += 1;
        }
    }
    if show_dev && !package.dev_dependencies.is_empty() {
        println!("{}", console::style("devdependencies:").bold());
        for (name, range) in &package.dev_dependencies {
            let version = lockfile.as_ref()
                .and_then(|lf| lf.root_resolved.get(name))
                .and_then(|key| lf_version(&lockfile, key))
                .unwrap_or_else(|| "???".to_string());
            println!("  {} {} {}", console::style(name).cyan(), console::style(&version).green(), console::style(format!("({})", range)).dim());
            count += 1;
        }
    }

    if count == 0 {ui::info("no dependencies installed");} 
    else {
        let total = lockfile.as_ref().map(|lf| lf.packages.len()).unwrap_or(0);
        ui::info(&format!("{} direct, {} total", count, total));
    }
    Ok(())
}

fn print_dep_tree(deps: &BTreeMap<String, String>, lockfile: &Lockfile, depth: usize) {
    let indent = "  ".repeat(depth);
    for (name, range) in deps {
        let resolved_key = lockfile.root_resolved.get(name)
            .or_else(|| {
                lockfile.packages.keys()
                    .find(|key| key.starts_with(&format!("{}@", name)))
            });
        if let Some(key) = resolved_key {
            if let Some(locked_package) = lockfile.packages.get(key) {
                println!("{}├─ {} {} {}", indent,
                    console::style(name).cyan(),
                    console::style(&locked_package.version).green(),
                    console::style(format!("({})", range)).dim());
                if !locked_package.dependencies.is_empty() {
                    let child_indent = "  ".repeat(depth + 1);
                    for dep_name in locked_package.dependencies.keys() {
                        let dep_key = locked_package.resolved_deps.get(dep_name);
                        let dep_ver = dep_key
                            .and_then(|key| lockfile.packages.get(key))
                            .map(|package| package.version.as_str())
                            .unwrap_or("???");
                        println!("{}├─ {} {}", child_indent,
                            console::style(dep_name).dim(),
                            console::style(dep_ver).dim());
                    }
                }
            }
        } else {println!("{}├─ {} {}", indent, console::style(name).cyan(),console::style("(not installed)".to_lowercase()).red());}
    }
}

fn handle_cache(action: CacheAction) -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let base = std::path::PathBuf::from(&home).join(".vee");

    match action {
        CacheAction::Clean => {
            let store = base.join("store");
            let metadata = base.join("metadata");
            let tmp = base.join("tmp");
            let mut freed = 0u64;
            for dir in [&store, &metadata, &tmp] {
                if dir.exists() {freed += dir_size(dir);
                    std::fs::remove_dir_all(dir)?;
                }
            }
            std::fs::create_dir_all(&store)?;
            std::fs::create_dir_all(&metadata)?;
            std::fs::create_dir_all(&tmp)?;
            ui::success(&format!("cache cleaned (freed {})", format_bytes(freed)));
        }
        CacheAction::Info => {
            let store = base.join("store");
            let metadata = base.join("metadata");
            let store_size = if store.exists() { dir_size(&store) } else { 0 };
            let meta_size = if metadata.exists() { dir_size(&metadata) } else { 0 };
            let store_count = if store.exists() {
                std::fs::read_dir(&store).map(|rd| rd.count()).unwrap_or(0)
            } else {
                0
            };
            println!("cache directory: {}", base.display());
            println!("  packages: {} ({})", store_count, format_bytes(store_size));
            println!("  metadata: {}", format_bytes(meta_size));
            println!("  total:    {}", format_bytes(store_size + meta_size));
        }
    }
    Ok(())
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    total += dir_size(&entry.path());
                } else if ft.is_file() {
                    total += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                }
            }
        }
    }
    total
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

async fn handle_exec(
    package: String,
    args: Vec<String>,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (package_name, version) = parse_package_spec(&package);
    let short_name = package_name.split('/').next_back().unwrap_or(&package_name).to_string();

    let local_bin = std::path::Path::new("node_modules/.bin").join(&short_name);
    if local_bin.exists() {
        if verbose {
            ui::info(&format!("using local binary: {}", local_bin.display()));
        }
        let local_nm = std::path::Path::new("node_modules");
        return run_exec_bin(&local_bin, &args, local_nm);}

    let project_dir = std::path::Path::new(".");
    let registry = registry::npm::NpmRegistry::from_project_dir(project_dir)?;
    let version_spinner = ui::spinner(&format!("resolving {}...", package_name));
    let (resolved_version, initial_metadata) = if version == "latest" {
        let (ver, meta) = registry.latest_version_with_metadata(&package_name).await?;
        (
            format!("^{}", ver),
            std::collections::HashMap::from([(package_name.clone(), meta)]),
        )
    } else {
        (version, std::collections::HashMap::new())
    };
    version_spinner.finish_and_clear();
    ui::info(&format!("executing {}@{}", package_name, resolved_version));

    let deps: std::collections::HashMap<String, String> = [(package_name.clone(), resolved_version)].into_iter().collect();
    let optional_names: HashSet<String> = HashSet::new();
    let resolve_spinner = ui::spinner("resolving dependencies...");
    let resolved = resolver::resolve_with_metadata(&deps, &optional_names, &registry, initial_metadata).await?;
    resolve_spinner.finish_and_clear();
    let cache = std::sync::Arc::new(cache::CacheStore::new()?);
    let client = &registry.client;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(128));
    let mut tasks = tokio::task::JoinSet::new();
    for (key, package) in &resolved.packages {
        let cache = cache.clone();
        let semaphore = semaphore.clone();
        let client = client.clone();
        let key = key.clone();
        let integrity = package.integrity.clone();
        let tarball_url = package.tarball_url.clone();
        let auth_header = registry.auth_header_for_url(&tarball_url);
        tasks.spawn(async move {
            let _permit = semaphore.acquire().await.map_err(|error| anyhow::anyhow!(error))?;
            let path = cache.ensure(&integrity, &tarball_url, &client, auth_header).await?;
            Ok::<_, anyhow::Error>((key, path))
        });
    }

    let total = resolved.packages.len() as u64;
    let fetch_progress = ui::progress(total, "fetching packages...");
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok((key, _path))) => {
                fetch_progress.set_message(key);
                fetch_progress.inc(1);
            }
            Ok(Err(error)) => return Err(error.into()),
            Err(error) => return Err(error.into()),
        }
    }
    fetch_progress.finish_and_clear();

    let temp_dir = tempfile::tempdir()?;
    let temp_path = temp_dir.path().to_path_buf();
    let link_spinner = ui::spinner("linking packages...");
    let linker_inst = linker::Linker::new(temp_path.clone(), cache);
    linker_inst.link_flat(&resolved, &deps)?;
    link_spinner.finish_and_clear();
    let node_modules = temp_path.join("node_modules");
    let bin_dir = node_modules.join(".bin");
    let bin_path = bin_dir.join(&short_name);
    if !bin_path.exists() {
        let entries: Vec<String> = std::fs::read_dir(&bin_dir)
            .ok()
            .map(|read_dir| read_dir.filter_map(|entry| entry.ok()).map(|entry| entry.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        if entries.len() == 1 {
            let fallback = bin_dir.join(&entries[0]);
            if verbose {
                ui::info(&format!("using bin: {}", entries[0]));
            }
            return run_exec_bin(&fallback, &args, &node_modules);
        }

        if entries.is_empty() {
            return Err(anyhow::anyhow!(
                "package '{}' does not expose any binaries", package_name
            ).into());
        }

        return Err(anyhow::anyhow!(
            "failed to find bin '{}'. available: {}",
            short_name,
            entries.join(", ")
        ).into());
    }


    run_exec_bin(&bin_path, &args, &node_modules)
}

fn run_exec_bin(
    bin_path: &std::path::Path,
    args: &[String],
    node_modules: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let bin_dir = node_modules.join(".bin");
    let new_path = format!("{}:{}", bin_dir.display(), current_path);
    let status = std::process::Command::new(bin_path)
        .args(args)
        .env("NODE_PATH", node_modules)
        .env("PATH", &new_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|error| anyhow::anyhow!("failed to execute {}: {}", bin_path.display(), error))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Install { package, production, frozen_lockfile, ignore_scripts } => {
            if let Some(package_spec) = package {
                handle_add(vec![package_spec], false, None, cli.verbose, cli.simulate).await?;
            } else {
                let opts = InstallOptions {
                    production,
                    frozen_lockfile,
                    ignore_scripts,
                    verbose: cli.verbose,
                    simulate: cli.simulate,
                };
                handle_install(&opts).await?;
            }
        }
        Commands::Add { packages, dev, version } => {
            handle_add(packages, dev, version, cli.verbose, cli.simulate).await?;
        }
        Commands::Remove { packages } => {
            handle_remove(packages, cli.verbose, cli.simulate).await?;
        }
        Commands::Update { packages } => {
            handle_update(packages, cli.verbose, cli.simulate).await?;
        }
        Commands::List { prod, dev, tree } => {
            handle_list(prod, dev, tree, cli.verbose)?;
        }
        Commands::Run { script, args } => {
            handle_run(script, args, cli.verbose)?;
        }
        Commands::Create { package, args } => {
            handle_create(package, args, cli.verbose).await?;
        }
        Commands::Exec { package, args } => {
            handle_exec(package, args, cli.verbose).await?;
        }
        Commands::Init { yes } => {handle_init(yes)?;}
        Commands::Outdated => {handle_outdated(cli.verbose).await?;}
        Commands::Cache { action } => {handle_cache(action)?;}
    }
    Ok(())
}
