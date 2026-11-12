//! Package change previews and the role check for deployments.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgExecutor};
use utoipa::ToSchema;

use crate::error::ApiError;
use crate::user::CurrentUser;

/// Name to version.
type Manifest = HashMap<String, String>;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Upgrade,
    Downgrade,
    Added,
    Removed,
    /// Devices are coming from versions on both sides of the target.
    Mixed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, ToSchema)]
pub struct PackageChange {
    pub package: String,
    /// Versions the affected devices are on; empty for a package being added.
    pub from: Vec<String>,
    /// `None` when the package is being removed.
    pub to: Option<String>,
    pub kind: ChangeKind,
    /// Devices this changes the package on. Zero when the change is only against
    /// the distribution's latest release.
    pub device_count: i64,
}

pub fn cannot_ship(user: &CurrentUser) -> Option<&'static str> {
    (!user.has_permission("devices", "write")).then_some("Your role does not allow deployments.")
}

pub fn ensure_can_ship(user: &CurrentUser) -> Result<(), ApiError> {
    match cannot_ship(user) {
        Some(reason) => Err(ApiError::Forbidden(reason.into())),
        None => Ok(()),
    }
}

pub struct Source {
    pub manifest: Manifest,
    pub device_count: i64,
}

/// Compares release versions component by component, so `1.10.0` is newer than
/// `1.9.0`. A suffix is a pre-release and sorts below the bare version
/// (`1.2.0-rc.1` < `1.2.0`), as release candidates are named.
pub fn compare_release_versions(a: &str, b: &str) -> Ordering {
    fn split(version: &str) -> (&str, &str) {
        let version = version.trim_start_matches('v');
        version.split_once(['-', '~']).unwrap_or((version, ""))
    }
    fn compare_dotted(a: &str, b: &str) -> Ordering {
        let mut a = a.split(['.', '+']);
        let mut b = b.split(['.', '+']);
        loop {
            let ordering = match (a.next(), b.next()) {
                (None, None) => return Ordering::Equal,
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (Some(a), Some(b)) => match (a.parse::<u64>(), b.parse::<u64>()) {
                    (Ok(a), Ok(b)) => a.cmp(&b),
                    _ => a.cmp(b),
                },
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
    }

    let (a_base, a_suffix) = split(a);
    let (b_base, b_suffix) = split(b);
    compare_dotted(a_base, b_base).then_with(|| match (a_suffix.is_empty(), b_suffix.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => compare_dotted(a_suffix, b_suffix),
    })
}

/// Compares package versions as dpkg does: `[epoch:]upstream[-revision]`, where
/// the revision is a package rebuild, so `1.2.3-1` is newer than `1.2.3`, and `~`
/// sorts before anything, even the end (`1.0~rc1` < `1.0`).
pub fn compare_package_versions(a: &str, b: &str) -> Ordering {
    fn split(version: &str) -> (u64, &str, &str) {
        let (epoch, rest) = match version.split_once(':') {
            Some((epoch, rest)) => (epoch.parse().unwrap_or(0), rest),
            None => (0, version),
        };
        let (upstream, revision) = rest.rsplit_once('-').unwrap_or((rest, ""));
        (epoch, upstream, revision)
    }
    fn order(c: Option<u8>) -> i32 {
        match c {
            None => 0,
            Some(b'~') => -1,
            Some(c) if c.is_ascii_digit() => 0,
            Some(c) if c.is_ascii_alphabetic() => i32::from(c),
            Some(c) => i32::from(c) + 256,
        }
    }
    // dpkg's verrevcmp: alternating non-digit and digit runs.
    fn compare_part(a: &str, b: &str) -> Ordering {
        let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
        while !a.is_empty() || !b.is_empty() {
            while a.first().is_some_and(|c| !c.is_ascii_digit())
                || b.first().is_some_and(|c| !c.is_ascii_digit())
            {
                let ordering = order(a.first().copied()).cmp(&order(b.first().copied()));
                if ordering != Ordering::Equal {
                    return ordering;
                }
                a = a.get(1..).unwrap_or_default();
                b = b.get(1..).unwrap_or_default();
            }
            let digits = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
            let (a_len, b_len) = (digits(a), digits(b));
            let a_num = a.get(..a_len).unwrap_or_default();
            let b_num = b.get(..b_len).unwrap_or_default();
            let trim = |n: &[u8]| -> usize { n.iter().take_while(|c| **c == b'0').count() };
            let a_sig = a_num.get(trim(a_num)..).unwrap_or_default();
            let b_sig = b_num.get(trim(b_num)..).unwrap_or_default();
            let ordering = a_sig.len().cmp(&b_sig.len()).then_with(|| a_sig.cmp(b_sig));
            if ordering != Ordering::Equal {
                return ordering;
            }
            a = a.get(a_len..).unwrap_or_default();
            b = b.get(b_len..).unwrap_or_default();
        }
        Ordering::Equal
    }

    let (a_epoch, a_upstream, a_revision) = split(a);
    let (b_epoch, b_upstream, b_revision) = split(b);
    a_epoch
        .cmp(&b_epoch)
        .then_with(|| compare_part(a_upstream, b_upstream))
        .then_with(|| compare_part(a_revision, b_revision))
}

/// The package changes of moving each source onto `target`.
pub fn review_changes(target: &Manifest, sources: &[Source]) -> Vec<PackageChange> {
    #[derive(Default)]
    struct Merged {
        from: Vec<String>,
        adds: bool,
        device_count: i64,
    }

    let mut merged: BTreeMap<&str, Merged> = BTreeMap::new();
    for source in sources {
        let names = source.manifest.keys().chain(target.keys());
        for name in names.collect::<HashSet<_>>() {
            let from = source.manifest.get(name);
            let to = target.get(name);
            if from == to {
                continue;
            }

            let entry = merged.entry(name).or_default();
            entry.device_count += source.device_count;
            match from {
                Some(from) if !entry.from.contains(from) => entry.from.push(from.clone()),
                Some(_) => {}
                None => entry.adds = true,
            }
        }
    }

    merged
        .into_iter()
        .map(|(name, mut change)| {
            let to = target.get(name).cloned();
            change.from.sort_by(|a, b| compare_package_versions(a, b));
            let kind = match &to {
                None => ChangeKind::Removed,
                Some(_) if change.from.is_empty() => ChangeKind::Added,
                Some(to) => {
                    let older = change
                        .from
                        .iter()
                        .any(|from| compare_package_versions(from, to) == Ordering::Less);
                    let newer = change
                        .from
                        .iter()
                        .any(|from| compare_package_versions(from, to) == Ordering::Greater);
                    match (older || change.adds, newer) {
                        (true, true) => ChangeKind::Mixed,
                        (false, true) => ChangeKind::Downgrade,
                        _ => ChangeKind::Upgrade,
                    }
                }
            };
            PackageChange {
                package: name.to_string(),
                from: change.from,
                to,
                kind,
                device_count: change.device_count,
            }
        })
        .collect()
}

async fn manifests<'e, E>(release_ids: &[i32], executor: E) -> sqlx::Result<HashMap<i32, Manifest>>
where
    E: PgExecutor<'e>,
{
    let rows = sqlx::query!(
        "SELECT rp.release_id, p.name, p.version
         FROM release_packages rp
         JOIN package p ON p.id = rp.package_id
         WHERE rp.release_id = ANY($1)",
        release_ids
    )
    .fetch_all(executor)
    .await?;

    let mut manifests: HashMap<i32, Manifest> = HashMap::new();
    for row in rows {
        manifests
            .entry(row.release_id)
            .or_default()
            .insert(row.name, row.version);
    }
    Ok(manifests)
}

/// What a move is being judged against.
pub enum MoveScope<'a> {
    /// The distribution's latest release: publishing a release.
    Publish,
    /// Specific devices, wherever they are now.
    Devices(&'a [i32]),
    /// A fleet rollout: every device following latest except those on a newer
    /// release candidate, plus the latest release itself so a release that would
    /// undo what's latest is caught even when no device happens to run it.
    Fleet,
    /// What a running deployment changes, judged against the release it started
    /// from rather than what is latest now.
    Deployment(i32),
}

/// The package changes of moving `scope` onto `release_id`. For
/// `MoveScope::Deployment` the release is the deployment's own.
pub async fn package_changes(
    release_id: i32,
    scope: MoveScope<'_>,
    conn: &mut PgConnection,
) -> sqlx::Result<Vec<PackageChange>> {
    let deployment = match scope {
        MoveScope::Deployment(deployment_id) => Some(
            sqlx::query!(
                "SELECT release_id, base_release_id FROM deployment WHERE id = $1",
                deployment_id
            )
            .fetch_one(&mut *conn)
            .await?,
        ),
        _ => None,
    };
    let release_id = deployment.as_ref().map_or(release_id, |d| d.release_id);

    let distribution = sqlx::query!(
        "SELECT d.id, d.latest_release_id
         FROM release r JOIN distribution d ON d.id = r.distribution_id
         WHERE r.id = $1",
        release_id
    )
    .fetch_one(&mut *conn)
    .await?;
    let latest_id = match &deployment {
        Some(deployment) => deployment.base_release_id,
        None => distribution.latest_release_id,
    };

    let groups: Vec<(Option<i32>, i64)> = match scope {
        MoveScope::Publish => latest_id.map(|id| (Some(id), 0)).into_iter().collect(),
        MoveScope::Devices(device_ids) => sqlx::query!(
            r#"SELECT release_id, COUNT(*) AS "count!" FROM device
               WHERE id = ANY($1) AND release_id IS NOT NULL
               GROUP BY release_id"#,
            device_ids
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|row| (row.release_id, row.count))
        .collect(),
        MoveScope::Fleet => {
            let ahead = super::newer_candidate_ids(release_id, &mut *conn).await?;
            let mut groups: Vec<(Option<i32>, i64)> = sqlx::query!(
                r#"SELECT d.release_id, COUNT(*) AS "count!" FROM device d
                   JOIN release r ON r.id = d.release_id
                   WHERE r.distribution_id = $1 AND d.follow_latest AND NOT d.archived
                     AND NOT (d.release_id = ANY($2)
                              OR COALESCE(d.target_release_id = ANY($2), false))
                   GROUP BY d.release_id"#,
                distribution.id,
                &ahead
            )
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(|row| (row.release_id, row.count))
            .collect();
            if let Some(latest_id) = latest_id
                && !groups.iter().any(|(id, _)| *id == Some(latest_id))
            {
                groups.push((Some(latest_id), 0));
            }
            groups
        }
        MoveScope::Deployment(deployment_id) => {
            let mut groups: Vec<(Option<i32>, i64)> = sqlx::query!(
                r#"SELECT from_release_id, COUNT(*) AS "count!" FROM deployment_devices
                   WHERE deployment_id = $1 AND from_release_id IS NOT NULL
                   GROUP BY from_release_id"#,
                deployment_id
            )
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(|row| (row.from_release_id, row.count))
            .collect();
            if let Some(latest_id) = latest_id
                && !groups.iter().any(|(id, _)| *id == Some(latest_id))
            {
                groups.push((Some(latest_id), 0));
            }
            groups
        }
    };

    let mut ids: Vec<i32> = groups.iter().filter_map(|(id, _)| *id).collect();
    ids.push(release_id);
    ids.extend(latest_id);
    let mut manifests = manifests(&ids, &mut *conn).await?;

    let sources: Vec<Source> = groups
        .iter()
        .filter(|(id, _)| *id != Some(release_id))
        .map(|(id, device_count)| Source {
            manifest: id
                .and_then(|id| manifests.get(&id))
                .cloned()
                .unwrap_or_default(),
            device_count: *device_count,
        })
        .collect();
    let target = manifests.remove(&release_id).unwrap_or_default();

    Ok(review_changes(&target, &sources))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(
            compare_release_versions("1.10.0", "1.9.0"),
            Ordering::Greater
        );
        assert_eq!(
            compare_release_versions("1.2.0-rc.1", "1.2.0"),
            Ordering::Less
        );
        assert_eq!(compare_release_versions("v1.2.0", "1.2.0"), Ordering::Equal);
    }

    #[test]
    fn package_versions_compare_as_dpkg_does() {
        let cases = [
            ("1.2.3-1", "1.2.3", Ordering::Greater),
            ("1.0~rc1", "1.0", Ordering::Less),
            ("1:0.1", "2.0", Ordering::Greater),
            ("1.10", "1.9", Ordering::Greater),
            ("1.0a", "1.0", Ordering::Greater),
            ("1.01", "1.1", Ordering::Equal),
            (
                "4.2-2ubuntu2.1-teton1.2.10",
                "4.2-2ubuntu2.1-teton1.2.9",
                Ordering::Greater,
            ),
        ];
        for (a, b, expected) in cases {
            assert_eq!(compare_package_versions(a, b), expected, "{a} vs {b}");
        }
    }

    #[test]
    fn changes_report_direction_and_devices() {
        let target = HashMap::from([("smith".into(), "2.0.0".into())]);
        let sources = [Source {
            manifest: HashMap::from([("smith".into(), "3.0.0".into())]),
            device_count: 3,
        }];
        let changes = review_changes(&target, &sources);
        assert_eq!(changes.first().map(|c| c.kind), Some(ChangeKind::Downgrade));
        assert_eq!(changes.first().map(|c| c.device_count), Some(3));
    }
}
