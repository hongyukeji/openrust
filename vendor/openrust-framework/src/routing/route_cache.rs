use std::{
    collections::{BTreeMap, HashMap},
    io::{self, Write as _},
    path::Path,
};

use http::Method;
use serde::{Deserialize, Serialize};

use super::Route;

const VERSION: u32 = 1;

#[derive(Deserialize, Serialize)]
pub(crate) struct RouteCache {
    version: u32,
    methods: BTreeMap<String, MethodIndex>,
}

#[derive(Default, Deserialize, Serialize)]
struct MethodIndex {
    literal: BTreeMap<String, Vec<usize>>,
    dynamic: Vec<usize>,
}

impl RouteCache {
    pub(super) fn compile(methods: &HashMap<Method, Vec<usize>>, routes: &[Route]) -> Self {
        let methods = methods
            .iter()
            .map(|(method, positions)| {
                let mut index = MethodIndex::default();
                for (ordinal, &position) in positions.iter().enumerate() {
                    match bucket(&routes[position]) {
                        Some(segment) => index
                            .literal
                            .entry(segment.into())
                            .or_default()
                            .push(ordinal),
                        None => index.dynamic.push(ordinal),
                    }
                }
                (method.to_string(), index)
            })
            .collect();
        Self {
            version: VERSION,
            methods,
        }
    }

    pub(super) fn load(
        path: &Path,
        methods: &HashMap<Method, Vec<usize>>,
        routes: &[Route],
    ) -> Option<Self> {
        let cache: Self = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        (cache.version == VERSION
            && cache.methods.len() == methods.len()
            && methods.iter().all(|(method, positions)| {
                cache
                    .methods
                    .get(method.as_str())
                    .is_some_and(|index| index.valid(positions, routes))
            }))
        .then_some(cache)
    }

    pub(super) fn candidates(
        &self,
        method: &Method,
        path: &str,
    ) -> impl Iterator<Item = usize> + '_ {
        let index = self.methods.get(method.as_str());
        let literal = index
            .and_then(|index| index.literal.get(segment(path)))
            .map_or(&[][..], Vec::as_slice);
        let dynamic = index.map_or(&[][..], |index| index.dynamic.as_slice());
        let mut literal = literal.iter().copied().peekable();
        let mut dynamic = dynamic.iter().copied().peekable();
        std::iter::from_fn(move || match (literal.peek(), dynamic.peek()) {
            (Some(left), Some(right)) if left <= right => literal.next(),
            (_, Some(_)) => dynamic.next(),
            _ => literal.next(),
        })
    }

    pub(crate) fn save(&self, path: &Path) -> io::Result<()> {
        let directory = path
            .parent()
            .ok_or_else(|| io::Error::other("Invalid route cache path"))?;
        std::fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(&mut file, self)?;
        file.flush()?;
        file.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    pub(crate) fn clear(path: &Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

impl MethodIndex {
    fn valid(&self, positions: &[usize], routes: &[Route]) -> bool {
        let valid = |ordinals: &[usize], segment| {
            ordinals.windows(2).all(|pair| pair[0] < pair[1])
                && ordinals.iter().all(|&ordinal| {
                    positions
                        .get(ordinal)
                        .is_some_and(|&index| bucket(&routes[index]) == segment)
                })
        };
        self.dynamic.len() + self.literal.values().map(Vec::len).sum::<usize>() == positions.len()
            && valid(&self.dynamic, None)
            && self
                .literal
                .iter()
                .all(|(segment, ordinals)| valid(ordinals, Some(segment.as_str())))
    }
}

fn segment(path: &str) -> &str {
    path.strip_prefix('/')
        .unwrap_or(path)
        .split('/')
        .next()
        .unwrap_or_default()
}

fn bucket(route: &Route) -> Option<&str> {
    let first = segment(route.registration_uri());
    (!first.contains('{')).then_some(first)
}
