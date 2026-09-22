use serde::{Deserialize, Serialize};
use utoipa::IntoParams;

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Release {
    pub id: i32,
    pub distribution_id: i32,
    pub distribution_architecture: String,
    pub distribution_name: String,
    pub version: String,
    pub draft: bool,
    pub yanked: bool,
    pub release_candidate: bool,
    pub lts: bool,
    pub lts_marked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<i32>,
    pub user_email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateRelease {
    pub draft: Option<bool>,
    pub yanked: Option<bool>,
    pub lts: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionBump {
    Patch,
    Minor,
    Major,
}

/// The version after `current` for a semver bump. Only the leading
/// `MAJOR.MINOR.PATCH` is read, so a suffix like `-rc` is dropped. None when
/// `current` does not start with one.
pub fn next_version(current: &str, bump: VersionBump) -> Option<String> {
    fn number(digits: &str) -> Option<u64> {
        // `parse` alone would also accept a leading `+`.
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok()
    }

    let mut parts = current.splitn(3, '.');
    let major = number(parts.next()?)?;
    let minor = number(parts.next()?)?;
    let rest = parts.next()?;
    let patch_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let patch = number(rest.get(..patch_end)?)?;

    Some(match bump {
        VersionBump::Patch => format!("{major}.{minor}.{}", patch + 1),
        VersionBump::Minor => format!("{major}.{}.0", minor + 1),
        VersionBump::Major => format!("{}.0.0", major + 1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bumps_each_component() {
        assert_eq!(
            next_version("1.2.3", VersionBump::Patch).as_deref(),
            Some("1.2.4")
        );
        assert_eq!(
            next_version("1.2.3", VersionBump::Minor).as_deref(),
            Some("1.3.0")
        );
        assert_eq!(
            next_version("1.2.3", VersionBump::Major).as_deref(),
            Some("2.0.0")
        );
    }

    #[test]
    fn drops_suffixes() {
        assert_eq!(
            next_version("1.2.3-rc", VersionBump::Patch).as_deref(),
            Some("1.2.4")
        );
    }

    #[test]
    fn rejects_non_semver() {
        assert_eq!(next_version("v1.2.3", VersionBump::Patch), None);
        assert_eq!(next_version("1.2", VersionBump::Patch), None);
        assert_eq!(next_version("+1.2.3", VersionBump::Patch), None);
        assert_eq!(next_version("1.2.x", VersionBump::Patch), None);
    }
}

/// Query filter for release listings.
#[derive(Debug, Serialize, Deserialize, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReleaseFilter {
    /// Filter by LTS status. If None, both LTS and non-LTS releases are included.
    pub lts: Option<bool>,
}
