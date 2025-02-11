use crate::Result;
use anyhow::Context;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display, fs::File, io::Write, path::PathBuf};
use toml_edit::DocumentMut;

#[derive(Debug, Serialize, Deserialize)]
pub struct TuxWranglerConfigLocked {
    /// The docker registry that images should be pushed to.
    pub registry: String,

    /// All versions for the supported bases
    #[serde(rename = "base", default)]
    pub bases: Vec<BaseConfig>,

    /// All versions for the supported features
    #[serde(rename = "feature", default)]
    pub features: Vec<InstallationConfig>,

    /// The abstract builds that should be run for this configuration
    #[serde(rename = "build", default)]
    pub builds: Vec<SingleBuild>,
}

impl TuxWranglerConfigLocked {
    pub fn base(&self, target_base: &SingleVersioned) -> Option<&BaseConfig> {
        self.bases
            .iter()
            .find(|base| base.name == target_base.name && base.version == target_base.version)
    }

    pub fn package_manager_for_base(&self, base: &SingleVersioned) -> Option<String> {
        self.base(base).map(|p| p.package_manager.to_string())
    }

    pub fn feature(&self, target_feature: &SingleVersioned) -> Option<&InstallationConfig> {
        self.features.iter().find(|feature| {
            feature.name == target_feature.name && feature.version == target_feature.version
        })
    }

    pub fn write(&self, path: PathBuf) -> Result<()> {
        let mut doc = toml::to_string_pretty(self)?.parse::<DocumentMut>()?;
        doc["feature"]
            .as_array_of_tables_mut()
            .context("Could not create array from features.")?
            .iter_mut()
            .for_each(|feature| feature.sort_values());
        let mut f = File::create(path.clone())?;
        f.write_all(doc.to_string().as_bytes())?;

        let mut f = File::create(path.with_extension("txt"))?;
        f.write_all(
            self.builds
                .iter()
                .map(|build| &build.target)
                .join("\n")
                .as_bytes(),
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct SingleVersioned {
    pub name: String,
    pub version: String,
}

impl Display for SingleVersioned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BaseConfig {
    pub name: String,
    pub version: String,
    pub registry: String,
    pub identifier: ImageIdentifier,
    pub package_manager: String,
    pub tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ImageIdentifier {
    Tag { tag: String },
    Digest { digest: String },
}

impl Display for ImageIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageIdentifier::Tag { tag } => write!(f, ":{tag}"),
            ImageIdentifier::Digest { digest } => write!(f, "@{digest}"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstallationConfig {
    pub name: String,
    pub version: String,
    #[serde(rename = "step")]
    pub steps: Vec<Layer>,
    pub tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum LayerType {
    Build,
    #[default]
    Actual,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Layer {
    #[serde(default, rename = "type")]
    pub layer_type: LayerType,
    #[serde(flatten)]
    pub installation: Installation,
    #[serde(default)]
    pub copy: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "method", rename_all = "kebab-case")]
pub enum Installation {
    Docker(DockerInstallation),
    Rpm(RpmInstallation),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct DockerInstallation {
    pub commands: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct RpmInstallation {
    #[serde(flatten)]
    pub installation_methods: HashMap<String, RpmInstallationMethod>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct RpmInstallationMethod {
    pub script: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SingleBuild {
    pub base: SingleVersioned,
    #[serde(default)]
    pub features: Vec<SingleVersioned>,
    pub target: String,
    pub image_name: String,
    pub image_tag: String,
}

impl Display for SingleBuild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}",
            self.base,
            self.features
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<String>>()
                .join(" ")
        )
    }
}
