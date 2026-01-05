use chrono::{DateTime, Utc};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use thiserror::Error;
use url::Url;

use crate::services::github::UnifiedPrComment;

#[derive(Debug, Error)]
pub enum GitLabServiceError {
    #[error("invalid GitLab base URL: {0}")]
    InvalidBaseUrl(String),
    #[error("invalid GitLab merge request URL: {0}")]
    InvalidMergeRequestUrl(String),
    #[error("GitLab token not configured")]
    TokenMissing,
    #[error("GitLab API request failed: {0}")]
    RequestFailed(String),
    #[error("GitLab returned unexpected response: {0}")]
    UnexpectedResponse(String),
    #[error("insufficient permissions")]
    InsufficientPermissions,
    #[error("not found")]
    NotFound,
}

#[derive(Debug, Clone)]
pub struct GitLabRepoInfo {
    pub base_url: String,
    pub project_path: String,
}

impl GitLabRepoInfo {
    pub fn from_remote_url(remote_url: &str) -> Result<Self, GitLabServiceError> {
        // Supported:
        // - https://gitlab.example.com/group/sub/project.git
        // - git@gitlab.example.com:group/sub/project.git
        // - ssh://git@gitlab.example.com/group/sub/project.git

        let trimmed = remote_url.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(GitLabServiceError::InvalidBaseUrl(
                "remote URL is empty".to_string(),
            ));
        }

        if let Ok(url) = Url::parse(trimmed) {
            let host = url
                .host_str()
                .ok_or_else(|| GitLabServiceError::InvalidBaseUrl(trimmed.to_string()))?;
            let base_url = format!(
                "{}://{}{}",
                url.scheme(),
                host,
                url.port()
                    .map(|p| format!(":{p}"))
                    .unwrap_or_default()
            );

            let mut segments = url
                .path_segments()
                .map(|s| s.collect::<Vec<_>>())
                .unwrap_or_default();
            if segments.is_empty() {
                return Err(GitLabServiceError::InvalidBaseUrl(trimmed.to_string()));
            }

            // Strip trailing ".git" from last segment
            if let Some(last) = segments.last_mut()
                && let Some(stripped) = last.strip_suffix(".git")
            {
                *last = stripped;
            }

            let project_path = segments.join("/");
            return Ok(Self {
                base_url,
                project_path,
            });
        }

        // SCP-like syntax: git@host:group/sub/project.git
        let (host, path) = trimmed
            .split_once(':')
            .ok_or_else(|| GitLabServiceError::InvalidBaseUrl(trimmed.to_string()))?;
        let host = host.split('@').next_back().unwrap_or(host);
        let mut project_path = path.trim_start_matches('/').to_string();
        project_path = project_path.trim_end_matches(".git").to_string();

        Ok(Self {
            base_url: format!("https://{host}"),
            project_path,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GitLabMergeRequestRef {
    pub base_url: String,
    pub project_path: String,
    pub iid: i64,
    pub web_url: String,
}

impl GitLabMergeRequestRef {
    pub fn parse(url: &str) -> Result<Self, GitLabServiceError> {
        let parsed = Url::parse(url)
            .map_err(|e| GitLabServiceError::InvalidMergeRequestUrl(e.to_string()))?;

        let base_url = format!(
            "{}://{}{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or_default(),
            parsed
                .port()
                .map(|p| format!(":{p}"))
                .unwrap_or_default()
        );

        let segments: Vec<&str> = parsed
            .path_segments()
            .map(|s| s.collect())
            .unwrap_or_default();

        // Expected: /<group>/<project>/-/merge_requests/<iid>
        let dash_idx = segments
            .iter()
            .position(|s| *s == "-")
            .ok_or_else(|| GitLabServiceError::InvalidMergeRequestUrl(url.to_string()))?;

        let project_segments = &segments[..dash_idx];
        if project_segments.is_empty() {
            return Err(GitLabServiceError::InvalidMergeRequestUrl(url.to_string()));
        }
        let project_path = project_segments.join("/");

        let mr_idx = segments
            .iter()
            .position(|s| *s == "merge_requests")
            .ok_or_else(|| GitLabServiceError::InvalidMergeRequestUrl(url.to_string()))?;

        let iid_str = segments
            .get(mr_idx + 1)
            .ok_or_else(|| GitLabServiceError::InvalidMergeRequestUrl(url.to_string()))?;
        let iid = iid_str
            .parse::<i64>()
            .map_err(|_| GitLabServiceError::InvalidMergeRequestUrl(url.to_string()))?;

        Ok(Self {
            base_url,
            project_path,
            iid,
            web_url: url.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CreateGitLabReviewCommentInput {
    pub path: String,
    pub line: i64,
    pub side: String, // LEFT/RIGHT
    pub body: String,
}

#[derive(Debug, Deserialize)]
struct GitLabAuthor {
    username: String,
}

#[derive(Debug, Deserialize)]
struct GitLabNote {
    id: i64,
    body: String,
    created_at: DateTime<Utc>,
    author: GitLabAuthor,
    system: bool,
    #[serde(default)]
    position: Option<GitLabPosition>,
}

#[derive(Debug, Deserialize)]
struct GitLabDiscussion {
    id: String,
    notes: Vec<GitLabNote>,
}

#[derive(Debug, Deserialize)]
struct GitLabPosition {
    #[serde(default)]
    old_path: Option<String>,
    #[serde(default)]
    new_path: Option<String>,
    #[serde(default)]
    old_line: Option<i64>,
    #[serde(default)]
    new_line: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GitLabMrDetails {
    diff_refs: GitLabDiffRefs,
}

#[derive(Debug, Deserialize)]
struct GitLabDiffRefs {
    base_sha: String,
    head_sha: String,
    start_sha: String,
}

#[derive(Debug, Deserialize)]
struct GitLabMergeRequestListItem {
    iid: i64,
    web_url: String,
    state: String,
    #[serde(default)]
    merged_at: Option<DateTime<Utc>>,
    #[serde(default)]
    merge_commit_sha: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GitLabMergeRequestInfo {
    pub iid: i64,
    pub url: String,
    pub state: String,
    pub merged_at: Option<DateTime<Utc>>,
    pub merge_commit_sha: Option<String>,
}

#[derive(Clone)]
pub struct GitLabService {
    client: Client,
    base_url: String,
    token: String,
}

impl GitLabService {
    pub fn new(base_url: &str, token: &str) -> Result<Self, GitLabServiceError> {
        if token.trim().is_empty() {
            return Err(GitLabServiceError::TokenMissing);
        }

        let base_url = base_url.trim_end_matches('/').to_string();
        Url::parse(&base_url)
            .map_err(|e| GitLabServiceError::InvalidBaseUrl(e.to_string()))?;

        Ok(Self {
            client: Client::new(),
            base_url,
            token: token.to_string(),
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v4{}", self.base_url, path)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, GitLabServiceError> {
        let url = self.api_url(path);
        let req = self
            .client
            .get(url)
            .header("PRIVATE-TOKEN", &self.token)
            .query(query);

        let resp = req.send().await.map_err(|e| GitLabServiceError::RequestFailed(e.to_string()))?;
        match resp.status() {
            StatusCode::OK => resp
                .json::<T>()
                .await
                .map_err(|e| GitLabServiceError::UnexpectedResponse(e.to_string())),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(GitLabServiceError::InsufficientPermissions),
            StatusCode::NOT_FOUND => Err(GitLabServiceError::NotFound),
            s => {
                let body = resp.text().await.unwrap_or_default();
                Err(GitLabServiceError::RequestFailed(format!(
                    "status {s}: {body}"
                )))
            }
        }
    }

    async fn post_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T, GitLabServiceError> {
        let url = self.api_url(path);
        let resp = self
            .client
            .post(url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| GitLabServiceError::RequestFailed(e.to_string()))?;

        match resp.status() {
            StatusCode::OK | StatusCode::CREATED => resp
                .json::<T>()
                .await
                .map_err(|e| GitLabServiceError::UnexpectedResponse(e.to_string())),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(GitLabServiceError::InsufficientPermissions),
            StatusCode::NOT_FOUND => Err(GitLabServiceError::NotFound),
            s => {
                let body = resp.text().await.unwrap_or_default();
                Err(GitLabServiceError::RequestFailed(format!(
                    "status {s}: {body}"
                )))
            }
        }
    }

    fn encode_project_path(project_path: &str) -> String {
        utf8_percent_encode(project_path, NON_ALPHANUMERIC).to_string()
    }

    pub async fn list_merge_requests_for_branch(
        &self,
        project_path: &str,
        source_branch: &str,
    ) -> Result<Vec<GitLabMergeRequestInfo>, GitLabServiceError> {
        let project = Self::encode_project_path(project_path);
        let items: Vec<GitLabMergeRequestListItem> = self
            .get_json(
                &format!("/projects/{project}/merge_requests"),
                &[
                    ("state", "all".to_string()),
                    ("source_branch", source_branch.to_string()),
                    ("per_page", "20".to_string()),
                ],
            )
            .await?;

        Ok(items
            .into_iter()
            .map(|i| GitLabMergeRequestInfo {
                iid: i.iid,
                url: i.web_url,
                state: i.state,
                merged_at: i.merged_at,
                merge_commit_sha: i.merge_commit_sha,
            })
            .collect())
    }

    pub async fn get_merge_request_comments(
        &self,
        mr: &GitLabMergeRequestRef,
    ) -> Result<Vec<UnifiedPrComment>, GitLabServiceError> {
        let project = Self::encode_project_path(&mr.project_path);

        let notes: Vec<GitLabNote> = self
            .get_json(
                &format!("/projects/{project}/merge_requests/{}/notes", mr.iid),
                &[("per_page", "100".to_string())],
            )
            .await?;

        let discussions: Vec<GitLabDiscussion> = self
            .get_json(
                &format!(
                    "/projects/{project}/merge_requests/{}/discussions",
                    mr.iid
                ),
                &[("per_page", "100".to_string())],
            )
            .await?;

        let mut unified: Vec<UnifiedPrComment> = Vec::new();

        for n in notes {
            if n.system {
                continue;
            }
            // Notes can include diff discussion notes as well; those are already handled via
            // `/discussions` below. Skipping them avoids duplicate comments in the UI.
            if n.position.is_some() {
                continue;
            }
            unified.push(UnifiedPrComment::General {
                id: n.id.to_string(),
                author: n.author.username,
                author_association: "CONTRIBUTOR".to_string(),
                body: n.body,
                created_at: n.created_at,
                url: format!("{}#note_{}", mr.web_url, n.id),
            });
        }

        for d in discussions {
            let _ = d.id; // discussion id not currently exposed
            for n in d.notes {
                if n.system {
                    continue;
                }
                let Some(pos) = n.position else { continue };

                let (path, line, side) = if let (Some(p), Some(l)) = (pos.new_path, pos.new_line) {
                    (p, Some(l), Some("RIGHT".to_string()))
                } else if let (Some(p), Some(l)) = (pos.old_path, pos.old_line) {
                    (p, Some(l), Some("LEFT".to_string()))
                } else {
                    continue;
                };

                unified.push(UnifiedPrComment::Review {
                    id: n.id,
                    author: n.author.username,
                    author_association: "CONTRIBUTOR".to_string(),
                    body: n.body,
                    created_at: n.created_at,
                    url: format!("{}#note_{}", mr.web_url, n.id),
                    path,
                    line,
                    side,
                    diff_hunk: String::new(),
                });
            }
        }

        unified.sort_by_key(|c| match c {
            UnifiedPrComment::General { created_at, .. } => *created_at,
            UnifiedPrComment::Review { created_at, .. } => *created_at,
        });

        Ok(unified)
    }

    pub async fn submit_review_comments(
        &self,
        mr: &GitLabMergeRequestRef,
        comments: Vec<CreateGitLabReviewCommentInput>,
    ) -> Result<(), GitLabServiceError> {
        if comments.is_empty() {
            return Ok(());
        }

        let project = Self::encode_project_path(&mr.project_path);
        let details: GitLabMrDetails = self
            .get_json(
                &format!("/projects/{project}/merge_requests/{}", mr.iid),
                &[],
            )
            .await?;

        for c in comments {
            let side = c.side.to_uppercase();
            let mut position = serde_json::json!({
                "position_type": "text",
                "base_sha": details.diff_refs.base_sha,
                "start_sha": details.diff_refs.start_sha,
                "head_sha": details.diff_refs.head_sha,
                "new_path": c.path,
                "old_path": c.path,
            });

            if side == "LEFT" {
                position["old_line"] = serde_json::json!(c.line);
            } else {
                position["new_line"] = serde_json::json!(c.line);
            }

            let _resp: serde_json::Value = self
                .post_json(
                    &format!(
                        "/projects/{project}/merge_requests/{}/discussions",
                        mr.iid
                    ),
                    serde_json::json!({
                        "body": c.body,
                        "position": position,
                    }),
                )
                .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mr_url_with_nested_groups() {
        let mr = GitLabMergeRequestRef::parse(
            "https://gitlab.example.com/group/sub/project/-/merge_requests/42",
        )
        .unwrap();
        assert_eq!(mr.base_url, "https://gitlab.example.com");
        assert_eq!(mr.project_path, "group/sub/project");
        assert_eq!(mr.iid, 42);
    }

    #[test]
    fn parse_repo_info_from_https_remote() {
        let repo = GitLabRepoInfo::from_remote_url(
            "https://gitlab.example.com/group/sub/project.git",
        )
        .unwrap();
        assert_eq!(repo.base_url, "https://gitlab.example.com");
        assert_eq!(repo.project_path, "group/sub/project");
    }

    #[test]
    fn parse_repo_info_from_ssh_remote() {
        let repo =
            GitLabRepoInfo::from_remote_url("git@gitlab.example.com:group/sub/project.git")
                .unwrap();
        assert_eq!(repo.base_url, "https://gitlab.example.com");
        assert_eq!(repo.project_path, "group/sub/project");
    }
}
