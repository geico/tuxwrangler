use std::{collections::HashMap, time::SystemTime};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use handlebars::Handlebars;
use log::debug;
use regex::Regex;
use serde_json::{json, Value};

use crate::lock::SingleVersioned;

pub fn split_version(version: &str) -> Vec<String> {
    let re = Regex::new(r#"[^\w^\*]*([\w\*]*)"#).expect("regex");
    re.captures_iter(version)
        .map(|c| c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default())
        .collect()
}

pub fn find_tag(target: &str, tags: &[String]) -> Result<String> {
    debug!("Searching {:?} to match '{}'", tags, target);
    if target == "latest" {
        return tags
            .first()
            .cloned()
            .context("There were no tags found even though 'latest' version was requested.");
    }
    tags.iter()
        .find(|tag| version_match(target, tag))
        .cloned()
        .context(format!("No matching tags for {target}"))
}

pub fn version_match(target: &str, source: &str) -> bool {
    let target_versions = split_version(target);
    let source_versions = split_version(source);
    if source_versions.len() < target_versions.len() {
        return false;
    }
    target_versions
        .into_iter()
        .enumerate()
        .all(|(i, v)| v == "*" || source_versions[i] == v)
}

fn handlebars_data(version: &str) -> Value {
    json!({"version": version, "versions": split_version(version)})
}

pub fn populate_template(template: &str, versions: &[String]) -> Result<HashMap<String, String>> {
    let mut hb = Handlebars::new();
    hb.set_strict_mode(true);
    versions
        .iter()
        .map(|version| {
            hb.render_template(template, &handlebars_data(version))
                .map(|rendered| (version.clone(), rendered))
                .context(format!(
                    "Unable to render template '{template}' for version '{version}'"
                ))
        })
        .collect::<Result<HashMap<String, String>>>()
}

pub fn populate_name_template(
    template: &str,
    base_version: &SingleVersioned,
    feature_versions: &[SingleVersioned],
) -> Result<String> {
    let today = SystemTime::now();
    let dt: DateTime<Utc> = today.into();
    let date = dt.format("%y-%m-%d");
    let mut hb = Handlebars::new();
    hb.set_strict_mode(true);
    hb.render_template(
        template,
        &json!(feature_versions
            .iter()
            .chain(vec![base_version].into_iter())
            .map(|version| (version.name.clone(), handlebars_data(&version.version)))
            .chain(
                vec![(
                    "base".to_string(),
                    json!({"name": base_version.name, "v": handlebars_data(&base_version.version)})
                ), ("date".to_string(), json!(date.to_string()))]
                .into_iter()
            )
            .collect::<HashMap<String, Value>>()),
    )
    .context(format!(
        "Unable to render template '{template}' for base '{}'",
        base_version.name
    ))
}
