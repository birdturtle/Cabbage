use serde::{Deserialize, Serialize};
const REPOSITORY: &str = "birdturtle/Cabbage";

#[derive(Clone, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    #[serde(default)] pub draft: bool,
    #[serde(default)] pub prerelease: bool,
    pub name: Option<String>,
    pub body: Option<String>,
    pub published_at: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub notes: String,
    pub published_at: String,
    pub installed_version: String,
    pub available: bool,
    pub message: String,
}
pub fn version(tag: &str) -> Result<semver::Version, String> {
    let text = tag.strip_prefix('v').unwrap_or(tag);
    if !text
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
    {
        return Err("Invalid release version.".into());
    }
    semver::Version::parse(text).map_err(|_| "Invalid release version.".into())
}
fn select_release(releases: Vec<GitHubRelease>, beta: bool) -> Result<GitHubRelease, String> {
    releases
        .into_iter()
        .filter(|r| !r.draft && (beta || !r.prerelease))
        .filter_map(|r| version(&r.tag_name).ok().map(|v| (v, r)))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, r)| r)
        .ok_or_else(|| "No published releases are available for this update channel.".into())
}
pub async fn latest(http: &reqwest::Client, beta: bool) -> Result<GitHubRelease, String> {
    if beta {
        let mut releases = Vec::new();
        for page in 1..=10 {
            let batch: Vec<GitHubRelease> = http
                .get(format!(
                    "https://api.github.com/repos/{REPOSITORY}/releases?per_page=100&page={page}"
                ))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "Cabbage")
                .send()
                .await
                .map_err(|_| {
                    "Cannot reach GitHub. Try again later.".to_string()
                })?
                .error_for_status()
                .map_err(|_| "GitHub could not return releases.".to_string())?
                .json()
                .await
                .map_err(|_| "GitHub returned invalid release information.".to_string())?;
            let done = batch.len() < 100;
            releases.extend(batch);
            if done {
                return select_release(releases, true);
            }
        }
        return Err("Too many releases to check safely.".into());
    }
    http.get(format!(
        "https://api.github.com/repos/{REPOSITORY}/releases/latest"
    ))
    .header("Accept", "application/vnd.github+json")
    .header("User-Agent", "Cabbage")
    .send()
    .await
    .map_err(|_| "Cannot reach GitHub. Try again later.".to_string())?
    .error_for_status()
    .map_err(|_| "No published Cabbage release is available, or GitHub could not return it.".to_string())?
    .json()
    .await
    .map_err(|_| "GitHub returned invalid release information.".into())
}

pub fn describe(release: GitHubRelease, installed: &str) -> Result<Release, String> {
    let available = version(&release.tag_name)? > version(installed)?;
    Ok(Release {
        tag: release.tag_name,
        name: release.name.unwrap_or_default(),
        notes: release.body.unwrap_or_default(),
        published_at: release.published_at.unwrap_or_default(),
        installed_version: installed.into(),
        available,
        message: if available { "A newer Cabbage release is available." } else { "You have the latest version." }.into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn release(tag: &str, prerelease: bool, draft: bool) -> GitHubRelease {
        GitHubRelease { tag_name: tag.into(), prerelease, draft, name: None, body: None, published_at: None }
    }
    #[test]
    fn stable_supersedes_matching_rc_and_drafts_are_excluded() {
        let releases = vec![release("v1.0.0-rc.1", true, false), release("v1.0.0", false, false), release("v2.0.0", false, true)];
        assert_eq!(select_release(releases.clone(), true).unwrap().tag_name, "v1.0.0");
        assert_eq!(select_release(releases, false).unwrap().tag_name, "v1.0.0");
    }
    #[test]
    fn beta_channel_can_select_a_newer_beta() {
        assert_eq!(select_release(vec![release("v1.0.0", false, false), release("v1.1.0-beta.1", true, false)], true).unwrap().tag_name, "v1.1.0-beta.1");
    }
    #[test]
    fn malformed_and_empty_feeds_fail_closed() {
        assert!(select_release(vec![release("../escape", false, false)], true).is_err());
        assert!(select_release(vec![], false).is_err());
    }
}
