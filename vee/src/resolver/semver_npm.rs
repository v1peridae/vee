use anyhow::{Context, Result};

pub struct NpmVersionReq {
    reqs: Vec<semver::VersionReq>,
}

impl NpmVersionReq {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() || input == "*" || input == "latest" {
            return Ok(Self {reqs: vec![semver::VersionReq::STAR],});
        }
        let parts: Vec<&str> = input.split("||").collect();
        let mut reqs = Vec::with_capacity(parts.len());
        for part in parts {
            let normalised = normalise_range(part.trim());
            let req = semver::VersionReq::parse(&normalised)
                .with_context(|| format!("failed to parse version range: '{}'", part.trim()))?;
            reqs.push(req);
        } Ok(Self { reqs }) }
    pub fn matches(&self, version: &semver::Version) -> bool {
        self.reqs.iter().any(|req| req.matches(version))
    }
}

fn normalise_range(input: &str) -> String {
    let mut range = input.to_string();
    range = range.replace(".x", ".*");
    if let Some(index) = range.find(" - ") {
        let left = range[..index].trim();
        let right = range[index + 3..].trim();
        range = format!(">={}, <={}", left, right);
    }
    range = add_commas(&range);
    range
}

fn add_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut prev_was_version_char = false;
    for char in input.chars() {
        if char == ' ' && prev_was_version_char {
            result.push(',');
            prev_was_version_char = false;
        } else {
            result.push(char);
            prev_was_version_char = char.is_ascii_digit() || char == '.' || char == '*';
        }
    }
    result}
