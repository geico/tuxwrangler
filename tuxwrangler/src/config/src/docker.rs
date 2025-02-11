use anyhow::Result;
use bollard::{auth::DockerCredentials, image::CreateImageOptions};
use docker_credential::DockerCredential;
use futures::TryStreamExt;
use log::trace;
use std::path::PathBuf;

pub struct Docker {
    pub(crate) docker: bollard::Docker,
    pub(crate) home: PathBuf,
}

impl Docker {
    pub fn new(home: PathBuf) -> Result<Self> {
        Ok(Self {
            docker: bollard::Docker::connect_with_defaults()?,
            home,
        })
    }

    pub fn from_bollard(docker: bollard::Docker, home: PathBuf) -> Self {
        Self { docker, home }
    }

    pub(crate) async fn pull(&self, image: &str) -> Result<()> {
        trace!("Pulling image '{}'", image);
        let creds = if let Some(registry) = Docker::registry(image).split('/').next() {
            trace!("Identified registry '{}'", registry);
            trace!("Checking for credentials");
            Some(match docker_credential::get_credential(registry) {
                Err(e) => {
                    trace!("No credentials found for registry '{}': {}", registry, e);
                    None
                }
                Ok(DockerCredential::IdentityToken(token)) => {
                    trace!("Using provided Docker credentials: {}", token);
                    Some(DockerCredentials {
                        username: Some("oauth2accesstoken".to_string()),
                        password: Some(token.clone()),
                        identitytoken: Some(token),
                        serveraddress: Some(registry.to_string()),
                        ..Default::default()
                    })
                }
                Ok(DockerCredential::UsernamePassword(username, password)) => {
                    trace!(
                        "Using provided Docker credentials: {}, {}",
                        username,
                        password
                    );
                    Some(DockerCredentials {
                        username: Some(username),
                        password: Some(password),
                        serveraddress: Some(Docker::registry(image)),
                        ..Default::default()
                    })
                }
            })
        } else {
            trace!("No registry found in image '{}'", image);
            trace!("No pull credentials will be used.");
            None
        };

        let mut stream = self.docker.create_image(
            Some(CreateImageOptions {
                from_image: image,
                ..Default::default()
            }),
            None,
            creds.flatten(),
        );
        while let Some(_next) = stream.try_next().await? {
            // Wait for the image pull to complete
        }

        Ok(())
    }

    pub(crate) fn registry(image: &str) -> String {
        image
            .split(":")
            .next()
            .expect("The image has a registry")
            .to_string()
    }

    pub(crate) fn tag(image: &str) -> Option<String> {
        image.split(":").nth(1).map(|s| s.to_string())
    }
}
