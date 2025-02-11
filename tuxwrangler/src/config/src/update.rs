use std::collections::HashMap;

use anyhow::Context;
use futures::future::join_all;
use futures::TryFutureExt;
use itertools::iproduct;
use itertools::Itertools;
use log::warn;

use crate::config::BaseDefinition;
use crate::config::BuildDefinition;
use crate::config::DockerFetchVersion;
use crate::config::FeatureDefinition;
use crate::config::FetchVersion;
use crate::config::GithubFetchVersion;
use crate::config::VersionedDefinition;
use crate::docker::Docker;
use crate::github::Github;
use crate::lock::BaseConfig;
use crate::lock::DockerInstallation;
use crate::lock::ImageIdentifier;
use crate::lock::Installation;
use crate::lock::InstallationConfig;
use crate::lock::Layer;
use crate::lock::RpmInstallation;
use crate::lock::RpmInstallationMethod;
use crate::lock::SingleBuild;
use crate::lock::SingleVersioned;
use crate::version::populate_name_template;
use crate::version::populate_template;
use crate::Clients;
use crate::Result;
use crate::TuxWranglerConfig;
use crate::TuxWranglerConfigLocked;

type Name = String;
type TargetVersion = String;
type ActualVersion = String;
type ActualVersions = HashMap<TargetVersion, ActualVersion>;
type NamedActualVersions = HashMap<Name, ActualVersions>;
type BaseConfigs = HashMap<SingleVersioned, BaseConfig>;
type FeatureConfigs = HashMap<SingleVersioned, InstallationConfig>;

impl TuxWranglerConfig {
    pub(crate) async fn build_locked(
        self,
        clients: &mut Clients,
    ) -> Result<TuxWranglerConfigLocked> {
        let actual_versions = self.actual_versions(clients).await?;
        let base_configs = self.base_configs(clients, &actual_versions).await?;
        let feature_configs = self.feature_configs(&actual_versions)?;
        let individual_builds = self.individual_builds(&base_configs, &feature_configs)?;
        Ok(TuxWranglerConfigLocked {
            registry: self.registry,
            bases: base_configs
                .values()
                .sorted_by(|a, b| {
                    format!("{}-{}", a.name, a.version).cmp(&format!("{}-{}", b.name, b.version))
                })
                .cloned()
                .collect(),
            features: feature_configs
                .values()
                .sorted_by(|a, b| {
                    format!("{}-{}", a.name, a.version).cmp(&format!("{}-{}", b.name, b.version))
                })
                .cloned()
                .collect(),
            builds: individual_builds,
        })
    }

    async fn actual_versions(&self, clients: &mut Clients) -> Result<NamedActualVersions> {
        let mut versions = NamedActualVersions::new();
        for base in &self.bases {
            if let Some(existing) = versions.get_mut(&base.name()) {
                existing.extend(base.actual_versions(clients).await?);
            } else {
                versions.insert(base.name(), base.actual_versions(clients).await?);
            }
        }
        for feature in &self.features {
            if let Some(existing) = versions.get_mut(&feature.name()) {
                existing.extend(feature.actual_versions(clients).await?);
            } else {
                versions.insert(feature.name(), feature.actual_versions(clients).await?);
            }
        }
        Ok(versions)
    }

    fn base_versions(&self, target_base: &Name) -> Vec<String> {
        self.bases
            .iter()
            .filter(|base| &base.name() == target_base)
            .flat_map(|base| base.definition.versioned.versions.clone())
            .collect()
    }

    fn feature_versions(&self, target_feature: &Name) -> Vec<String> {
        self.features
            .iter()
            .filter(|feature| &feature.name() == target_feature)
            .flat_map(|feature| feature.definition.versioned.versions.clone())
            .collect()
    }

    /// Compute all builds
    fn individual_builds(
        &self,
        base_configs: &BaseConfigs,
        feature_configs: &FeatureConfigs,
    ) -> Result<Vec<SingleBuild>> {
        self.builds
            .iter()
            .flat_map(|build| {
                // create iterators for each BuildDefinition, 1 per version
                let feature_groups = build
                    .features
                    .iter()
                    .map(|feature_set| {
                        feature_set
                            .iter()
                            .flat_map(|bd| {
                                let versions = match bd {
                                    BuildDefinition::Named(target_feature) => {
                                        self.feature_versions(target_feature)
                                    }
                                    BuildDefinition::Versioned(v) => v.versions.clone(),
                                };
                                let name = bd.name();
                                versions
                                    .into_iter()
                                    .map(|version| SingleVersioned {
                                        name: name.clone(),
                                        version,
                                    })
                                    .collect::<Vec<_>>()
                            })
                    })
                    // apply a cartesian product to achieve all combinations from each feature set
                    .multi_cartesian_product();

                // create base-version pairs
                let bases = build
                    .bases
                    .iter()
                    .flat_map(|bd| {
                        let versions = match bd {
                            BuildDefinition::Named(target_feature) => {
                                self.base_versions(target_feature)
                            }
                            BuildDefinition::Versioned(v) => v.versions.clone(),
                        };
                        let name = bd.name();
                        versions
                            .into_iter()
                            .map(|version| SingleVersioned {
                                name: name.clone(),
                                version,
                            })
                            .collect::<Vec<_>>()
                    });

                // Perform a cartesian product between the bases and feature groups
                iproduct!(bases, feature_groups).map(|(base, features)| {
                    base_configs
                        .get(&base).map(|p| (p.single_versioned(), p.tag.as_ref()))
                        .context(format!(
                            "Unable to find base '{}' with version '{}",
                            base.name, base.version
                        ))
                        .and_then(|p| {
                            features.iter().map(|feature| feature_configs.get(feature).map(|feature| (feature.single_versioned(), feature.tag.as_ref())).context(format!(
                                    "Unable to find feature '{}' with version '{}",
                                    feature.name, feature.version
                                ))).collect::<Result<Vec<(SingleVersioned, Option<&String>)>>>().map(|features| (p.0, p.1, features.into_iter().unzip::<SingleVersioned, Option<&String>, Vec<SingleVersioned>, Vec<Option<&String>>>()))
                        })
                        .and_then(|(base, base_tag, (features, feature_tags))| single_build(&build.image_name, &build.image_tag, base, base_tag, features, feature_tags))
                })
            })
            .collect::<Result<_>>()
    }

    async fn base_configs(
        &self,
        clients: &mut Clients,
        actual_versions: &NamedActualVersions,
    ) -> Result<BaseConfigs> {
        let mut bases = BaseConfigs::new();
        for base in &self.bases {
            let name = base.name();
            for version in &base.definition.versioned.versions {
                let single_versioned = SingleVersioned {
                    name: name.clone(),
                    version: version.clone(),
                };
                let actual_version = SingleVersioned {
                    name: name.clone(),
                    version: actual_versions
                        .get(&name)
                        .context(format!("No versions found for '{name}'"))?
                        .get(version)
                        .context(format!("Version '{version}' not found for '{name}'"))?
                        .clone(),
                };
                let tag = base
                    .definition
                    .version_tag
                    .as_ref()
                    .map(|tag| actual_version.populate_template(tag))
                    .transpose()?;
                let image = actual_version.populate_template(&base.image)?;
                let image_identifier = match clients.docker.digest(&image).await {
                    Ok(digest) => ImageIdentifier::Digest { digest },
                    Err(e) => {
                        if let Some(tag) = Docker::tag(&image) {
                            warn!("No digest was found for '{image}', using tag '{tag}' instead.");
                            ImageIdentifier::Tag { tag }
                        } else {
                            return Err(e);
                        }
                    }
                };
                let base_config = BaseConfig {
                    name: name.clone(),
                    registry: Docker::registry(&image),
                    version: actual_version.version,
                    package_manager: base.package_manager.clone(),
                    tag: tag.clone(),
                    identifier: image_identifier,
                };
                bases.insert(single_versioned, base_config);
            }
        }
        Ok(bases)
    }

    fn feature_configs(&self, actual_versions: &NamedActualVersions) -> Result<FeatureConfigs> {
        let mut features = FeatureConfigs::new();
        for feature in &self.features {
            let name = feature.name();
            for version in &feature.definition.versioned.versions {
                let single_versioned = SingleVersioned {
                    name: name.clone(),
                    version: version.clone(),
                };
                let actual_version = SingleVersioned {
                    name: name.clone(),
                    version: actual_versions
                        .get(&name)
                        .context(format!("No versions found for '{name}'"))?
                        .get(version)
                        .context(format!("Version '{version}' not found for '{name}'"))?
                        .clone(),
                };
                let tag = feature
                    .definition
                    .version_tag
                    .as_ref()
                    .map(|tag| actual_version.populate_template(tag))
                    .transpose()?;
                let feature_config = InstallationConfig {
                    name: name.clone(),
                    steps: feature
                        .steps
                        .iter()
                        .map(|step| step.populate(&actual_version))
                        .collect::<Result<_>>()?,
                    version: actual_version.version,
                    tag: tag.clone(),
                };
                features.insert(single_versioned, feature_config);
            }
        }
        Ok(features)
    }
}

impl BaseDefinition {
    async fn actual_versions(&self, clients: &mut Clients) -> Result<ActualVersions> {
        self.definition.actual_versions(clients).await
    }

    fn name(&self) -> Name {
        self.definition.name()
    }
}

impl FeatureDefinition {
    async fn actual_versions(&self, clients: &mut Clients) -> Result<ActualVersions> {
        self.definition.actual_versions(clients).await
    }

    fn name(&self) -> Name {
        self.definition.name()
    }
}

impl VersionedDefinition {
    async fn actual_versions(&self, clients: &mut Clients) -> Result<ActualVersions> {
        Ok(if let Some(fetch_version) = &self.fetch_version {
            fetch_version
                .fetch_versions(&self.versioned.versions, clients)
                .await?
        } else {
            self.versioned
                .versions
                .iter()
                .map(|version| (version.clone(), version.clone()))
                .collect()
        })
    }

    fn name(&self) -> Name {
        self.versioned.name.clone()
    }
}

impl SingleVersioned {
    fn populate_template(&self, template: &str) -> Result<String> {
        populate_template(template, &[self.version.clone()])
            .map(|map| map.values().map(|s| s.to_string()).collect())
    }

    fn populate_templates(&self, templates: &[String]) -> Result<Vec<String>> {
        templates
            .iter()
            .map(|t| self.populate_template(t))
            .collect::<Result<Vec<_>>>()
    }
}

impl FetchVersion {
    async fn fetch_versions(
        &self,
        versions: &[String],
        clients: &mut Clients,
    ) -> Result<ActualVersions> {
        match self {
            FetchVersion::Docker(fetch_version) => {
                clients.docker.fetch_versions(fetch_version, versions).await
            }
            FetchVersion::Github(fetch_version) => {
                clients.gh.fetch_versions(fetch_version, versions).await
            }
        }
    }
}

impl Docker {
    async fn fetch_versions(
        &self,
        fetch_version: &DockerFetchVersion,
        versions: &[String],
    ) -> Result<ActualVersions> {
        join_all(
            populate_template(&fetch_version.image, versions)?
                .iter()
                .map(|(target_version, image)| {
                    self.version(image, &fetch_version.command)
                        .map_ok(|version| (target_version.clone(), version))
                }),
        )
        .await
        .into_iter()
        .collect::<Result<ActualVersions>>()
    }
}

impl Github {
    async fn fetch_versions(
        &mut self,
        fetch_version: &GithubFetchVersion,
        versions: &[String],
    ) -> Result<ActualVersions> {
        let mut actual_versions = ActualVersions::new();
        for (target_version, project) in populate_template(&fetch_version.project, versions)? {
            actual_versions.insert(
                target_version.clone(),
                self.version(
                    &target_version,
                    &fetch_version.org,
                    &project,
                    &fetch_version.version_from,
                )
                .await?,
            );
        }
        Ok(actual_versions)
    }
}

impl BaseConfig {
    fn single_versioned(&self) -> SingleVersioned {
        SingleVersioned {
            name: self.name.clone(),
            version: self.version.clone(),
        }
    }
}

impl InstallationConfig {
    fn single_versioned(&self) -> SingleVersioned {
        SingleVersioned {
            name: self.name.clone(),
            version: self.version.clone(),
        }
    }
}

impl Layer {
    fn populate(&self, single_versioned: &SingleVersioned) -> Result<Self> {
        Ok(Self {
            installation: self.installation.populate(single_versioned)?,
            layer_type: self.layer_type.clone(),
            copy: self.copy.clone(),
        })
    }
}

impl Installation {
    fn populate(&self, single_versioned: &SingleVersioned) -> Result<Self> {
        Ok(match self {
            Installation::Docker(d) => Installation::Docker(d.populate(single_versioned)?),
            Installation::Rpm(r) => Installation::Rpm(r.populate(single_versioned)?),
        })
    }
}

impl DockerInstallation {
    fn populate(&self, single_versioned: &SingleVersioned) -> Result<Self> {
        Ok(Self {
            commands: single_versioned.populate_templates(&self.commands)?,
            dependencies: single_versioned.populate_templates(&self.dependencies)?,
        })
    }
}

impl RpmInstallation {
    fn populate(&self, single_versioned: &SingleVersioned) -> Result<Self> {
        Ok(Self {
            installation_methods: self
                .installation_methods
                .iter()
                .map(|(key, installation_method)| {
                    single_versioned
                        .populate_templates(&installation_method.script)
                        .map(|script| (key.clone(), RpmInstallationMethod { script }))
                })
                .collect::<Result<_>>()?,
        })
    }
}

fn single_build(
    image_name_template: &str,
    image_tag_template: &str,
    base: SingleVersioned,
    base_tag: Option<&String>,
    features: Vec<SingleVersioned>,
    feature_tags: Vec<Option<&String>>,
) -> Result<SingleBuild> {
    Ok(SingleBuild {
        image_name: populate_name_template(image_name_template, &base, &features)?,
        image_tag: populate_name_template(image_tag_template, &base, &features)?,
        base,
        features,
        target: base_tag
            .into_iter()
            .chain(feature_tags.into_iter().flatten())
            .filter(|tag| !tag.is_empty())
            .join("-"),
    })
}
