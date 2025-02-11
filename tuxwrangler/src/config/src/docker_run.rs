use crate::docker::Docker;
use crate::Result;
use anyhow::{anyhow, Context};
use bollard::container::Config;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures::StreamExt;
use log::{debug, trace};

impl Docker {
    /// Run the given command on a docker container built from the provided image.
    /// This function creates the container, starts the container and execs the command in the container.
    /// After completing execution, the docker container is cleaned up.
    pub(crate) async fn run_command(
        &self,
        image: &str,
        commands: &[String],
    ) -> Result<Vec<String>> {
        debug!("Running command '{:?}' in image '{}'", commands, image);
        self.pull(image)
            .await
            .context(anyhow!("Unable to pull image '{}'", image))?;
        trace!("Creating container for image '{image}'");
        let id = self
            .docker
            .create_container::<String, _>(
                None,
                Config {
                    image: Some(image.to_string()),
                    tty: Some(true),
                    ..Default::default()
                },
            )
            .await?
            .id;
        trace!("Container id '{id}'");
        trace!("Starting container '{id}'");
        self.docker.start_container::<String>(&id, None).await?;
        trace!("Creating Docker exec command '{:?}'", commands);
        let exec_id = self
            .docker
            .create_exec(
                &id,
                CreateExecOptions {
                    cmd: Some(commands.to_vec()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await?
            .id;
        trace!("Starting Docker exec");
        let output = if let StartExecResults::Attached { mut output, .. } =
            self.docker.start_exec(&exec_id, None).await?
        {
            trace!("Docker exec started. Waiting for logs...");
            let mut message = Vec::new();
            while let Some(Ok(next)) = output.next().await {
                trace!("Docker message: {next}");
                message.push(next.to_string());
            }
            trace!("Docker exec finished");
            message
        } else {
            unreachable!()
        };
        trace!("Stopping Docker container '{id}'");
        self.docker.stop_container(&id, None).await?;
        trace!("Removing Docker container '{id}'");
        self.docker.remove_container(&id, None).await?;
        trace!("Docker container '{id}' removed");
        debug!("Exec output: '{:?}'", output);
        Ok(output)
    }
}
