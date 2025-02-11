use serde::{Deserialize, Serialize};

use crate::lock::Layer;

#[derive(Debug, Serialize, Deserialize)]
pub struct TuxWranglerConfig {
    /// The docker registry that images should be pushed to.
    pub(crate) registry: String,

    /// All versions for the supported bases
    #[serde(rename = "base", default)]
    pub(crate) bases: Vec<BaseDefinition>,

    /// All versions for the supported features
    #[serde(rename = "feature", default)]
    pub(crate) features: Vec<FeatureDefinition>,

    /// The abstract builds that should be run for this configuration
    #[serde(rename = "build", default)]
    pub(crate) builds: Vec<Build>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Versioned {
    pub(crate) name: String,
    pub(crate) versions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BuildDefinition {
    Named(String),
    Versioned(Versioned),
}

impl BuildDefinition {
    pub(crate) fn name(&self) -> String {
        match self {
            BuildDefinition::Named(name) => name.clone(),
            BuildDefinition::Versioned(v) => v.name.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BaseDefinition {
    #[serde(flatten)]
    pub(crate) definition: VersionedDefinition,
    pub(crate) image: String,
    pub(crate) package_manager: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FeatureDefinition {
    #[serde(flatten)]
    pub(crate) definition: VersionedDefinition,
    #[serde(rename = "step")]
    pub(crate) steps: Vec<Layer>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct VersionedDefinition {
    #[serde(flatten)]
    pub(crate) versioned: Versioned,
    #[serde(default)]
    pub(crate) version_tag: Option<String>,
    pub(crate) fetch_version: Option<FetchVersion>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FetchVersion {
    Docker(DockerFetchVersion),
    Github(GithubFetchVersion),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DockerFetchVersion {
    pub(crate) image: String,
    pub(crate) command: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GithubFetchVersion {
    pub(crate) org: String,
    pub(crate) project: String,
    #[serde(default)]
    pub(crate) version_from: VersionFrom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum VersionFrom {
    #[default]
    Tag,
    Branch,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Build {
    pub(crate) bases: Vec<BuildDefinition>,
    pub(crate) features: Vec<Vec<BuildDefinition>>,
    pub(crate) image_name: String,
    pub(crate) image_tag: String,
}
