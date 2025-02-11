use std::{collections::HashMap, env, time::Duration};

use anyhow::{anyhow, Result};
use log::{debug, info};
use octocrab::Octocrab;

use crate::{config::VersionFrom, version::find_tag};
const MAX_PAGES: u8 = 4;
const MAX_RETRIES: u32 = 5;
const BASE_BACKOFF_S: u64 = 1;

pub struct Github {
    cache: HashMap<(String, String), HashMap<u8, Vec<String>>>,
    octo: Octocrab,
}

impl Github {
    pub fn new(gh_token: Option<String>) -> Result<Self> {
        let gh_token = if gh_token.is_some() {
            gh_token
        } else if env::var("GH_TOKEN").ok().is_some() {
            env::var("GH_TOKEN").ok()
        } else if env::var("GITHUB_TOKEN").ok().is_some() {
            env::var("GITHUB_TOKEN").ok()
        } else {
            None
        };
        if gh_token.is_none() {
            debug!("No GitHub token was provided, you may see errors from rate limiting");
        }
        Ok(match gh_token {
            Some(token) => Self {
                octo: Octocrab::builder().personal_token(token).build()?,
                cache: Default::default(),
            },
            None => Self {
                octo: Octocrab::default(),
                cache: Default::default(),
            },
        })
    }

    pub(crate) async fn print_rate_limit(&self) -> Result<()> {
        println!(
            "GitHub rate limits: '{:?}'",
            self.octo.ratelimit().get().await?
        );
        Ok(())
    }

    pub(crate) async fn tags(
        &mut self,
        org: &str,
        project: &str,
        offset: u8,
        version_from: &VersionFrom,
    ) -> Result<Vec<String>> {
        let mut retry = 0;
        info!("Pulling tags from github for '{org}/{project}'");
        while retry < MAX_RETRIES {
            let res = self.tags_inner(org, project, offset, version_from).await;
            match res {
                Ok(r) => return Ok(r),
                Err(e) => debug!("Failed to get tags: '{:?}'", e),
            }
            retry += 1;
            debug!("Failed to reach github");
            tokio::time::sleep(Duration::from_secs(BASE_BACKOFF_S * 2_u64.pow(retry))).await;
        }
        Err(anyhow!(
            "Unable to pull tags for '{org}/{project}' after '{retry}' retries."
        ))
    }
    pub(crate) async fn tags_inner(
        &mut self,
        org: &str,
        project: &str,
        offset: u8,
        version_from: &VersionFrom,
    ) -> Result<Vec<String>> {
        if let Some(tag_sets) = self.cache.get(&(org.to_string(), project.to_string())) {
            if let Some(tags) = tag_sets.get(&offset) {
                debug!("Using cached github tags for '{org}/{project}'");
                return Ok(tags.clone());
            }
        }
        let tags: Vec<String> = match version_from {
            VersionFrom::Tag => self.get_tags(org, project, offset).await?,
            VersionFrom::Branch => self.get_branches(org, project, offset).await?,
        };
        if let Some(cache) = self.cache.get_mut(&(org.to_string(), project.to_string())) {
            cache.insert(offset, tags.clone());
        } else {
            self.cache.insert(
                (org.to_string(), project.to_string()),
                vec![(offset, tags.clone())].into_iter().collect(),
            );
        }
        Ok(tags)
    }

    async fn get_tags(&self, org: &str, project: &str, offset: u8) -> Result<Vec<String>> {
        let repo = self.octo.repos(org, project);
        Ok(futures::future::join_all(
            (MAX_PAGES * offset..MAX_PAGES + MAX_PAGES * offset)
                .map(|page| repo.list_tags().page(page).send()),
        )
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flat_map(|page| page.items)
        .map(|tag| tag.name)
        .collect())
    }

    async fn get_branches(&self, org: &str, project: &str, offset: u8) -> Result<Vec<String>> {
        let repo = self.octo.repos(org, project);
        let num_pages = repo
            .list_branches()
            .send()
            .await?
            .number_of_pages()
            .unwrap_or_default();
        if num_pages <= (offset * MAX_PAGES) as u32 {
            return Ok(Default::default());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(futures::future::join_all(
            // Branches are returned in order from oldest to newest
            (num_pages - ((offset + 1) * MAX_PAGES) as u32
                ..num_pages - (offset * MAX_PAGES) as u32)
                .map(|page| repo.list_branches().page(page + 1).send()),
        )
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flat_map(|page| page.items)
        .map(|branch| branch.name)
        .rev()
        .collect())
    }

    pub(crate) async fn version(
        &mut self,
        target_version: &str,
        org: &str,
        project: &str,
        version_from: &VersionFrom,
    ) -> Result<String> {
        let mut retry = 0;
        info!("Pulling tags from github for '{org}/{project}'");
        while retry < MAX_RETRIES {
            let res = find_tag(
                target_version,
                &self.tags(org, project, retry as u8, version_from).await?,
            );
            match res {
                Ok(r) => return Ok(r),
                Err(_) => debug!("Unable to find tag for '{target_version}' in {org}/{project} on tag set {retry}")
            }
            retry += 1;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Err(anyhow!(
            "Unable to find tag for '{target_version}' in {org}/{project}"
        ))
    }
}
