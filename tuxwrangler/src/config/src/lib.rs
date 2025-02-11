pub mod config;
pub mod docker;
mod docker_build;
mod docker_file;
mod docker_run;
mod docker_version;
mod github;
pub mod lock;
mod update;
mod version;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
pub use config::TuxWranglerConfig;
use docker::Docker;
use docker_file::create_dockerfile;
use github::Github;
pub use lock::TuxWranglerConfigLocked;

pub type Result<T> = anyhow::Result<T>;
pub struct Clients {
    pub docker: Docker,
    pub gh: Github,
}

impl Clients {
    pub fn new(gh_token: Option<String>) -> Result<Self> {
        Ok(Self {
            docker: Docker::new(".".into())?,
            gh: Github::new(gh_token)?,
        })
    }

    pub async fn print_gh_rate_limit(&self) -> Result<()> {
        self.gh.print_rate_limit().await?;
        Ok(())
    }
}

pub fn load_lockfile(path: PathBuf) -> Result<TuxWranglerConfigLocked> {
    toml::from_str(
        &fs::read_to_string(&path)
            .context(format!("Unable to open lock file at '{}'", path.display()))?,
    )
    .context("Unable to serialize lock file")
}

pub fn load_config(path: PathBuf) -> Result<TuxWranglerConfig> {
    toml::from_str(&fs::read_to_string(&path).context(format!(
        "Unable to open config file at '{}'",
        path.display()
    ))?)
    .context("Unable to serialize config file")
}

pub async fn update_lock(
    clients: &mut Clients,
    config_path: PathBuf,
    lock_path: PathBuf,
) -> Result<()> {
    load_config(config_path)?
        .build_locked(clients)
        .await?
        .write(lock_path)
}

pub async fn build_images(
    clients: &Clients,
    locked: TuxWranglerConfigLocked,
    skip_tags: bool,
) -> Result<()> {
    locked.build_images(&clients.docker, skip_tags).await
}

pub fn write_dockerfile(locked: TuxWranglerConfigLocked, out_dir: &Path) -> Result<()> {
    fs::write(
        out_dir.join("Dockerfile"),
        create_dockerfile(&locked)?.0.join("\n"),
    )?;

    Ok(())
}
