use std::collections::{HashMap, HashSet};

use crate::{
    config::Build,
    lock::{InstallationConfig, BaseConfig, SingleVersioned},
    TuxWranglerConfigLocked,
};

/// A representation of the changes bewtween 2 configurations
pub struct Changes {
    title: String,
    diffs: Vec<Diff<String>>,
    inner: Vec<Changes>,
}

pub struct LockChanges {
    registry: Diff<String>,
    bases: Vec<Diff<SingleVersioned>>,
    features: Vec<Diff<SingleVersioned>>,
    build: Vec<BuildDiff>,
}

pub struct BuildDiff {
    tags: Vec<Diff<String>>,
}

pub enum Diff<T> {
    Same(T),
    Added(T),
    Removed(T),
    Changed(T, T),
}

impl<T: Eq> Diff<T> {
    fn diff(from: T, to: T) -> Diff<T> {
        if from == to {
            Self::Same(from)
        } else {
            Self::Changed(from, to)
        }
    }

    fn option_diff(from: Option<T>, to: Option<T>) -> Option<Diff<T>> {
        match (from, to) {
            (None, None) => None,
            (None, Some(t)) => Some(Self::Added(t)),
            (Some(t), None) => Some(Self::Removed(t)),
            (Some(f), Some(t)) => Some(Self::diff(f, t)),
        }
    }
}

impl TuxWranglerConfigLocked {
    pub fn update_changes(self, next: Self) -> LockChanges {
        //base
        let original: HashSet<SingleVersioned> = self
            .bases
            .into_iter()
            .map(|pc| SingleVersioned {
                name: pc.name,
                version: pc.version,
            })
            .collect();
        let new: HashSet<SingleVersioned> = next
            .bases
            .into_iter()
            .map(|pc| SingleVersioned {
                name: pc.name,
                version: pc.version,
            })
            .collect();

        let base_diffs = new
            .difference(&original)
            // new bases
            .map(|sv| Diff::Added(sv.clone()))
            // removed bases
            .chain(
                original
                    .difference(&new)
                    .map(|sv| Diff::Removed(sv.clone())),
            )
            // unchanged bases
            .chain(original.intersection(&new).map(|sv| Diff::Same(sv.clone())))
            .collect();

        //features
        let original: HashSet<SingleVersioned> = self
            .features
            .into_iter()
            .map(|pc| SingleVersioned {
                name: pc.name,
                version: pc.version,
            })
            .collect();
        let new: HashSet<SingleVersioned> = next
            .features
            .into_iter()
            .map(|pc| SingleVersioned {
                name: pc.name,
                version: pc.version,
            })
            .collect();

        let feature_diffs = new
            .difference(&original)
            // new features
            .map(|sv| Diff::Added(sv.clone()))
            // removed features
            .chain(
                original
                    .difference(&new)
                    .map(|sv| Diff::Removed(sv.clone())),
            )
            // unchanged features
            .chain(original.intersection(&new).map(|sv| Diff::Same(sv.clone())))
            .collect();
        //builds

        LockChanges {
            registry: Diff::diff(self.registry, next.registry),
            bases: base_diffs,
            features: feature_diffs,
            build: Vec::new(),
        }
    }
}
