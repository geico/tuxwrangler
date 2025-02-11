use crate::docker::Docker;
use crate::Result;
use anyhow::Context;
use log::info;

impl Docker {
    pub(crate) async fn version(&self, image: &str, commands: &[String]) -> Result<String> {
        info!("Fetching version for '{}' from Docker", image);
        self.run_command(image, commands)
            .await?
            .into_iter()
            .last()
            .context(format!("No response from version command for '{image}'"))
    }

    pub(crate) async fn digest(&self, image: &str) -> Result<String> {
        info!("Fetching digest for '{}' from Docker", image);

        self.docker
            .inspect_registry_image(image, None)
            .await?
            .descriptor
            .digest
            .context(format!("'{image}' has no digest."))
    }
}
