use std::io::Write;

use crate::{docker::Docker, lock::SingleVersioned, TuxWranglerConfigLocked};
use anyhow::Result;
use bollard::image::{BuildImageOptions, TagImageOptions};
use futures::{future::join_all, TryStreamExt};
use log::{debug, error, info, trace};

use crate::docker_file::create_dockerfile_for;

impl Docker {
    pub async fn build_image(
        &self,
        config: &TuxWranglerConfigLocked,
        base: &SingleVersioned,
        features: &[SingleVersioned],
        tag: &str,
    ) -> Result<()> {
        let (dockerlines, dependencies) = create_dockerfile_for(config, base, features)?;
        let dockerfile = dockerlines.join("\n");
        trace!("Build Dockerfile: \n{dockerfile}");
        let mut header = tar::Header::new_gnu();
        header.set_path("Dockerfile")?;
        header.set_size(dockerfile.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        let mut tar = tar::Builder::new(Vec::new());
        tar.append(&header, dockerfile.as_bytes())?;
        for dependency in &dependencies {
            tar.append_dir_all(dependency, self.home.join(dependency))?
        }

        let uncompressed = tar.into_inner()?;
        let mut c = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        c.write_all(&uncompressed)?;
        let compressed = c.finish()?;
        let build_image_options = BuildImageOptions {
            t: tag,
            dockerfile: "Dockerfile",
            pull: true,
            ..Default::default()
        };

        let mut build = self
            .docker
            .build_image(build_image_options, None, Some(compressed.into()));

        while let Some(bi) = build.try_next().await? {
            trace!("Response: {:?}", bi);
            if let Some(status) = bi.status {
                debug!("{status}",)
            };
        }

        Ok(())
    }

    async fn _tag_images(&self, image_name: &str, repo: &str, tags: &[String]) -> Result<()> {
        for tag in tags {
            self.docker
                .tag_image(
                    image_name,
                    Some(TagImageOptions {
                        repo: repo.to_string(),
                        tag: tag.to_string(),
                    }),
                )
                .await?;
        }
        Ok(())
    }
}

impl TuxWranglerConfigLocked {
    pub(crate) async fn build_images(&self, docker: &Docker, skip_tags: bool) -> Result<()> {
        info!("Building images");
        join_all(self.builds.iter().map(|build| async move {
            info!("Build started for: {build}");
            let tag = &build.target;
            docker
                .build_image(self, &build.base, &build.features, tag)
                .await
                .inspect(|_| info!("Build completed for: {build}"))
                .inspect_err(|_| {
                    error!("Build failed for : {build}");
                })
        }))
        .await
        .into_iter()
        .collect::<Result<()>>()?;
        if skip_tags {
            info!("Skipping image tagging");
            return Ok(());
        }

        Ok(())
    }
}
