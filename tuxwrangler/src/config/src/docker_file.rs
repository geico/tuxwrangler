use std::collections::{HashMap, HashSet};

use crate::{
    lock::{
        BaseConfig, DockerInstallation, Installation, InstallationConfig, LayerType,
        RpmInstallationMethod, SingleVersioned,
    },
    TuxWranglerConfigLocked,
};
use anyhow::{Context, Result};

/// The dockerfile as a set of lines for easier manipulation
type Dockerfile = Vec<String>;
/// All local dependencies for a dockerfile
type Dependencies = Vec<String>;

/// A single image layer in a dockerfile
struct Layer {
    /// The name of the image created by this layer
    name: String,
    /// The lines (dockerfile) to create this layer
    lines: Dockerfile,
    /// The local dependencies for this layer
    dependencies: Dependencies,
}

impl Layer {
    /// Create a new Docker layer without any dependencies
    pub fn new(name: String, lines: Dockerfile) -> Self {
        Self {
            name,
            lines,
            dependencies: Default::default(),
        }
    }

    /// Add additional lines and dependencies to the existing layer
    pub fn extend(self, layer: (Dockerfile, Dependencies)) -> Self {
        let mut lines = self.lines;
        lines.extend(layer.0);
        let mut dependencies = self.dependencies;
        dependencies.extend(layer.1);
        Self {
            name: self.name,
            lines,
            dependencies,
        }
    }
}

/// Create a dockerfile for all targets in a locked config file
/// TODO: This will be useful once https://github.com/fussybeaver/bollard/issues/391 enables specifying a build target
pub fn create_dockerfile(config: &TuxWranglerConfigLocked) -> Result<(Dockerfile, Dependencies)> {
    let mut layer_names = HashSet::new();

    let mut layers = Vec::new();
    let mut dependencies = HashSet::new();

    for build in &config.builds {
        let base = base_layer(config.base(&build.base).context(format!(
            "Base {}-{} is missing from configuration",
            build.base.name, build.base.version
        ))?);
        if layer_names.insert(base.name.clone()) {
            layers.extend(base.lines)
        }
        let package_manager = config
            .package_manager_for_base(&build.base)
            .context(format!(
                "Base {}-{} is missing a package manager",
                build.base.name, build.base.version
            ))?;
        let mut prev_layer = base.name;
        for feature in build.features.clone() {
            let feature_layers = installation_layers(
                &package_manager,
                config.feature(&feature).context(format!(
                    "Feature {}-{} is missing from configuration",
                    feature.name, feature.version
                ))?,
                &prev_layer,
            )?;
            prev_layer = feature_layers
                .last()
                .context("Feature did not create any layers")?
                .name
                .clone();

            for layer in feature_layers {
                if layer_names.insert(layer.name.clone()) {
                    layers.extend(layer.lines);
                    dependencies.extend(layer.dependencies);
                }
            }
        }

        if layer_names.insert(build.target.clone()) {
            layers.extend(tag_layer(&prev_layer, &build.target))
        }
    }

    Ok((layers, dependencies.into_iter().collect()))
}

/// Create a dockerfile for the given base and features using the lock file
pub fn create_dockerfile_for(
    config: &TuxWranglerConfigLocked,
    base: &SingleVersioned,
    features: &[SingleVersioned],
) -> Result<(Dockerfile, Dependencies)> {
    // Keep track of each layer
    let mut layers = Vec::new();
    // Keep track of local dependencies from each layer
    let mut dependencies = HashSet::new();

    // Create a layer for the base
    let base_layer = base_layer(config.base(base).context(format!(
        "Base {}-{} is missing from configuration",
        base.name, base.version
    ))?);
    layers.extend(base_layer.lines);
    // Determine the package manager for rmp based feature installs
    let package_manager = config.package_manager_for_base(base).context(format!(
        "Base {}-{} is missing a package manager",
        base.name, base.version
    ))?;

    // Keep track of the previous layers name so that it can be used in the next layer
    let mut prev_layer = base_layer.name;
    // Create a layer with the installation for each feature, building them in the order they were specified
    for feature in features {
        let feature_layers = installation_layers(
            &package_manager,
            config.feature(feature).context(format!(
                "Feature {}-{} is missing from configuration",
                feature.name, feature.version
            ))?,
            &prev_layer,
        )?;
        prev_layer = feature_layers
            .last()
            .context("Feature did not create any layers")?
            .name
            .clone();

        for layer in feature_layers {
            layers.extend(layer.lines);
            dependencies.extend(layer.dependencies);
        }
    }

    Ok((layers, dependencies.into_iter().collect()))
}

/// Create a dockerfile layer for a base (base image)
fn base_layer(base: &BaseConfig) -> Layer {
    let layer_name = base.tag.to_owned().unwrap_or_else(|| "temp".to_string());
    Layer::new(
        layer_name.clone(),
        vec![format!(
            "FROM {}{} as {}\n",
            base.registry, base.identifier, layer_name
        )],
    )
}

/// Compute all installation layers for a feature
fn installation_layers(
    package_manager: &str,
    installation: &InstallationConfig,
    previous_layer: &str,
) -> Result<Vec<Layer>> {
    let mut copies: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut ephemeral_prev_layer = previous_layer.to_string();
    let mut build_prev_layer = previous_layer.to_string();
    let mut layers = Vec::<Layer>::new();
    let final_layer_name = format!(
        "{}-{}-{}-final",
        previous_layer, installation.name, installation.version
    )
    .to_lowercase();
    for (i, layer) in installation.steps.iter().enumerate() {
        let layer_name = format!("{final_layer_name}-build-{i}");
        let src = if layer.layer_type == LayerType::Actual {
            let tmp = build_prev_layer;
            build_prev_layer = layer_name.clone();
            tmp
        } else {
            let tmp = ephemeral_prev_layer;
            ephemeral_prev_layer = layer_name.clone();
            tmp
        };

        layers.push(
            Layer::new(
                layer_name.clone(),
                vec![format!("FROM {src} as {layer_name}")],
            )
            .extend((
                copies
                    .iter()
                    .flat_map(|(layer, copies)| {
                        copies
                            .iter()
                            .map(move |(src, dest)| format!("COPY --from={layer} {src} {dest}"))
                    })
                    .collect(),
                Default::default(),
            ))
            .extend(installation_inner(package_manager, &layer.installation)?),
        );
        copies.insert(layer_name.clone(), layer.copy.clone());
    }

    // Make sure we create the final layer if it wasn't defined in the config.
    layers.push(
        Layer::new(
            final_layer_name.clone(),
            vec![format!("FROM {build_prev_layer} as {final_layer_name}")],
        )
        .extend((
            copies
                .iter()
                .flat_map(|(layer, copies)| {
                    copies
                        .iter()
                        .map(move |(src, dest)| format!("COPY --from={layer} {src} {dest}"))
                })
                .collect(),
            Default::default(),
        )),
    );

    Ok(layers)
}

/// Create a feature installation layer
fn installation_inner(
    package_manager: &str,
    installation: &Installation,
) -> Result<(Dockerfile, Dependencies)> {
    // Determine the installation method from the configuration
    Ok(match installation {
        Installation::Docker(docker_config) => docker_installation(docker_config),
        Installation::Rpm(rpm_config) => (
            rpm_installation(
                rpm_config
                    .installation_methods
                    .get(package_manager)
                    .context(format!(
                        "No installation instructions for {}",
                        package_manager
                    ))?,
            ),
            // rpm installation does not support local dependencies
            Default::default(),
        ),
    })
}

/// Create the dockerfile and dependencies for a docker installation
fn docker_installation(docker_config: &DockerInstallation) -> (Dockerfile, Dependencies) {
    (
        docker_config.commands.clone(),
        docker_config.dependencies.clone(),
    )
}

/// Create the Dockerfile for rmp installation
fn rpm_installation(rpm_config: &RpmInstallationMethod) -> Dockerfile {
    // Create a line for the script installation
    run_command(&rpm_config.script).into_iter().collect()
}

/// Create a RUN command line if commands is not empty otherwise do not create a line
fn run_command(commands: &[String]) -> Option<String> {
    if commands.is_empty() {
        None
    } else {
        Some(format!("RUN {}", commands.join(" && \\\n")))
    }
}

fn tag_layer(prev_layer: &str, tag: &str) -> Dockerfile {
    vec![format!("FROM {prev_layer} as {tag}")]
}
