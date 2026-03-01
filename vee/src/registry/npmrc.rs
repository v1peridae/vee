use anyhow::Result;
use base64::Engine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RegistryAuth {
    pub token: Option<String>,
    pub legacy_auth: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub always_auth: bool,
}

impl RegistryAuth {
    fn empty() -> Self { Self { token: None, legacy_auth: None, username: None, password: None, always_auth: false }}
    pub fn header_value(&self) -> Option<String> {
        if let Some(ref token) = self.token {
            return Some(format!("Bearer {}", token));
        }
        if let Some(ref basic) = self.legacy_auth {
            return Some(format!("Basic {}", basic));
        }
        if let (Some(ref user), Some(ref encoded_pass)) = (&self.username, &self.password) {
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded_pass) {
                if let Ok(pass) = String::from_utf8(decoded) {
                    let combined = base64::engine::general_purpose::STANDARD
                        .encode(format!("{}:{}", user, pass));
                    return Some(format!("Basic {}", combined));
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct NpmrcConfig {
    pub default_registry: String,
    pub scoped_registries: HashMap<String, String>,
    pub auth_registries: HashMap<String, RegistryAuth>,
    pub strict_ssl: bool,
}

impl Default for NpmrcConfig {
    fn default() -> Self {
        Self {
            default_registry: "https://registry.npmjs.org".to_string(),
            scoped_registries: HashMap::new(),
            auth_registries: HashMap::new(),
            strict_ssl: true,
        }
    }
}

fn expand_env_vars(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'$' {
            if position + 1 < bytes.len() && bytes[position + 1] == b'{' {
                if let Some(end) = value[position + 2..].find('}') {
                    let var_name = &value[position + 2..position + 2 + end];
                    result.push_str(&std::env::var(var_name).unwrap_or_default());
                    position += 3 + end;
                    continue;
                }
            } else if position + 1 < bytes.len() && (bytes[position + 1].is_ascii_alphabetic() || bytes[position + 1] == b'_') {
                let start = position + 1;
                let mut end = start;
                while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') 
                {end += 1;}
                let var_name = &value[start..end];
                result.push_str(&std::env::var(var_name).unwrap_or_default());
                position = end;
                continue;
            }
        }
        result.push(bytes[position] as char);
        position += 1;
    }
    result
}

impl NpmrcConfig {
    pub fn parse(contents: &str) -> Result<Self> {
        let mut config = Self::default();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {continue;}
            let Some((key, value)) = line.split_once('=') else {continue;};
            let key = key.trim();
            let value = expand_env_vars(value.trim());
            if key == "registry" {
                config.default_registry = value.trim_end_matches('/').to_string();
            } else if key == "strict-ssl" {
                config.strict_ssl = value != "false";
            } else if key.starts_with('@') && key.ends_with(":registry") {
                let scope = key.trim_end_matches(":registry").to_string();
                config.scoped_registries.insert(scope, value.trim_end_matches('/').to_string());
            } else if key.starts_with("//") {
                if let Some((host_path, setting)) = key.trim_start_matches("//").rsplit_once(':') {
                    let host_key = host_path.trim_end_matches('/').to_string();
                    let entry = config.auth_registries.entry(host_key)
                        .or_insert_with(RegistryAuth::empty);
                    match setting {
                        "_authToken" => entry.token = Some(value),
                        "_auth" => entry.legacy_auth = Some(value),
                        "username" => entry.username = Some(value),
                        "_password" => entry.password = Some(value),
                        "always-auth" => entry.always_auth = value == "true",
                        _ => {}
                    }
                }
            }
        }
        Ok(config)
    }

    pub fn parse_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::parse(&contents)
    }



    fn merge(&mut self, other: Self) {
        if other.default_registry != "https://registry.npmjs.org" {
            self.default_registry = other.default_registry;
        }
        if !other.strict_ssl {self.strict_ssl = false;}
        self.scoped_registries.extend(other.scoped_registries);
        for (host, auth) in other.auth_registries {
            self.auth_registries.insert(host, auth);
        }
    }

    pub fn load(project_dir: &Path) -> Result<Self> {
        let mut config = Self::default();
        if let Ok(home) = std::env::var("HOME") {
            let user_rc = PathBuf::from(&home).join(".npmrc");
            if user_rc.exists() {
                if let Ok(user_config) = Self::parse_file(&user_rc) {
                    config.merge(user_config);
                }
            }
        }

        let project_rc = project_dir.join(".npmrc");
        if project_rc.exists() {
            if let Ok(project_config) = Self::parse_file(&project_rc) {
                config.merge(project_config);
            }
        }

        Ok(config)
    }

    pub fn auth_header_for_url(&self, url: &str) -> Option<(String, String)> {
        let stripped = url.trim_start_matches("https://").trim_start_matches("http://");
        let mut candidate = stripped.trim_end_matches('/');
        loop {
            for (key, auth) in &self.auth_registries {
                if key == candidate || candidate.starts_with(&format!("{}/", key)) {
                    if let Some(value) = auth.header_value() {
                        return Some(("Authorization".to_string(), value));
                    }
                }
            }
            match candidate.rfind('/') { Some(pos) => candidate = &candidate[..pos], None => break }
        }
        None
    }
}
