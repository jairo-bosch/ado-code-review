#[cfg(test)]
mod tests;

use std::{borrow::Cow, fmt};

use percent_encoding::percent_decode_str;
use url::Url;
use uuid::Uuid;

#[derive(PartialEq, Eq)]
pub(super) struct Repository {
    collection: Url,
    project: Option<String>,
    name: String,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid --repo-url: expected an HTTPS Azure Repos clone URL without a password")]
pub(super) struct InvalidRepository;

struct Scope<'a> {
    collection_segments: &'a [Cow<'a, str>],
    project: Option<&'a str>,
    repository: &'a str,
}

impl Repository {
    pub(super) fn parse(input: &str) -> Result<Self, InvalidRepository> {
        Self::validate_authority(input)?;
        let mut clone_url = Url::parse(input).map_err(|_| InvalidRepository)?;
        strip_userinfo(&mut clone_url)?;
        let segments = clone_url
            .path_segments()
            .ok_or(InvalidRepository)?
            .map(decode_segment)
            .collect::<Result<Vec<_>, _>>()?;
        let scope = split_path(&clone_url, &segments)?;
        let mut collection = clone_url.clone();
        collection
            .path_segments_mut()
            .map_err(|()| InvalidRepository)?
            .clear()
            .extend(scope.collection_segments.iter().map(AsRef::as_ref));
        Ok(Self {
            collection,
            project: scope.project.map(str::to_owned),
            name: scope.repository.to_owned(),
        })
    }

    fn validate_authority(input: &str) -> Result<(), InvalidRepository> {
        let (_, address) = input.split_once("://").ok_or(InvalidRepository)?;
        let authority = address
            .split(['/', '?', '#'])
            .next()
            .ok_or(InvalidRepository)?;
        if let Some((userinfo, _)) = authority.rsplit_once('@')
            && userinfo.contains(':')
        {
            return Err(InvalidRepository);
        }
        Ok(())
    }

    pub(super) fn git_api(&self, suffix: &[&str]) -> Result<Url, InvalidRepository> {
        let segments = ["repositories", self.name.as_str()]
            .into_iter()
            .chain(suffix.iter().copied());
        self.api(segments)
    }

    pub(super) fn api<'a>(
        &self,
        segments: impl IntoIterator<Item = &'a str>,
    ) -> Result<Url, InvalidRepository> {
        let mut endpoint = self.collection.clone();
        endpoint
            .path_segments_mut()
            .map_err(|()| InvalidRepository)?
            .pop_if_empty()
            .extend(self.project.as_deref())
            .extend(["_apis", "git"])
            .extend(segments);
        endpoint.query_pairs_mut().append_pair("api-version", "7.1");
        Ok(endpoint)
    }

    pub(super) fn connection_data(&self) -> Result<Url, InvalidRepository> {
        let mut endpoint = self.collection.clone();
        endpoint
            .path_segments_mut()
            .map_err(|()| InvalidRepository)?
            .pop_if_empty()
            .extend(["_apis", "connectionData"]);
        endpoint
            .query_pairs_mut()
            .append_pair("api-version", "7.1-preview.1");
        Ok(endpoint)
    }

    pub(super) fn pull_request_web_url(
        &self,
        (project, repository): (&str, &str),
        pr: std::num::NonZeroU32,
    ) -> Result<Url, InvalidRepository> {
        let mut endpoint = self.collection.clone();
        endpoint
            .path_segments_mut()
            .map_err(|()| InvalidRepository)?
            .pop_if_empty()
            .extend([project, "_git", repository, "pullrequest", &pr.to_string()]);
        Ok(endpoint)
    }

    pub(super) fn matches_names(&self, project: &str, repo: &str) -> bool {
        self.name.eq_ignore_ascii_case(repo)
            && self
                .project
                .as_deref()
                .is_none_or(|name| name.eq_ignore_ascii_case(project))
    }

    pub(super) fn with_ids(&self, project: Uuid, repo: Uuid) -> Self {
        Self {
            collection: self.collection.clone(),
            project: Some(project.to_string()),
            name: repo.to_string(),
        }
    }

    #[cfg(test)]
    pub(super) fn for_mock(collection: Url, project: Option<&str>) -> Self {
        Self {
            collection,
            project: project.map(str::to_owned),
            name: "Repository".into(),
        }
    }
}

impl fmt::Debug for Repository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Repository").finish_non_exhaustive()
    }
}

fn strip_userinfo(url: &mut Url) -> Result<(), InvalidRepository> {
    if url.scheme() != "https" || url.host().is_none() {
        return Err(InvalidRepository);
    }
    if url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err(InvalidRepository);
    }
    url.set_username("").map_err(|()| InvalidRepository)
}

fn decode_segment(segment: &str) -> Result<Cow<'_, str>, InvalidRepository> {
    let decoded = percent_decode_str(segment)
        .decode_utf8()
        .map_err(|_| InvalidRepository)?;
    if decoded.is_empty() || decoded.contains(['/', '\\']) || decoded.chars().any(char::is_control)
    {
        return Err(InvalidRepository);
    }
    Ok(decoded)
}

fn split_path<'a>(url: &Url, segments: &'a [Cow<'_, str>]) -> Result<Scope<'a>, InvalidRepository> {
    const TFS_COLLECTION_SEGMENTS: usize = 2;
    let [path @ .., git, name] = segments else {
        return Err(InvalidRepository);
    };
    if git != "_git" {
        return Err(InvalidRepository);
    }
    let host = url.host_str().ok_or(InvalidRepository)?;
    if host.ends_with(".visualstudio.com") {
        return visual_studio_path(path, name);
    }
    let prefix = match path {
        [tfs, ..] if tfs.eq_ignore_ascii_case("tfs") && host != "dev.azure.com" => {
            path.get(..TFS_COLLECTION_SEGMENTS)
        }
        _ => path.get(..1),
    }
    .ok_or(InvalidRepository)?;
    scoped_path(path, prefix, name)
}

fn visual_studio_path<'a>(
    path: &'a [Cow<'_, str>],
    name: &'a str,
) -> Result<Scope<'a>, InvalidRepository> {
    let prefix = match path {
        [collection, ..] if collection.eq_ignore_ascii_case("DefaultCollection") => {
            path.get(..1).ok_or(InvalidRepository)?
        }
        _ => &[],
    };
    scoped_path(path, prefix, name)
}

fn scoped_path<'a>(
    path: &'a [Cow<'_, str>],
    prefix: &'a [Cow<'_, str>],
    name: &'a str,
) -> Result<Scope<'a>, InvalidRepository> {
    let project = match path.get(prefix.len()..) {
        Some([]) => None,
        Some([project]) => Some(project.as_ref()),
        _ => return Err(InvalidRepository),
    };
    Ok(Scope {
        collection_segments: prefix,
        project,
        repository: name,
    })
}
