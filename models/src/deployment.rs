use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(sqlx::Type, Serialize, Deserialize, Debug, Clone, ToSchema, PartialEq)]
#[sqlx(type_name = "deployment_status", rename_all = "snake_case")]
pub enum DeploymentStatus {
    InProgress,
    Failed,
    Canceled,
    Done,
}

impl std::fmt::Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentStatus::InProgress => write!(f, "InProgress"),
            DeploymentStatus::Failed => write!(f, "Failed"),
            DeploymentStatus::Canceled => write!(f, "Canceled"),
            DeploymentStatus::Done => write!(f, "Done"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Deployment {
    pub id: i32,
    pub release_id: i32,
    pub status: DeploymentStatus,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, ToSchema)]
pub struct DeploymentRequest {
    /// Select release-candidate devices by labels. Stable releases choose canaries automatically.
    /// e.g. `canary_device_labels=["label1=a", "label2=b"]`
    pub canary_device_labels: Option<Vec<String>>,
    /// Select exact release-candidate device IDs, instead of labels.
    /// Cannot be used together with `canary_device_labels`.
    pub canary_device_ids: Option<Vec<i32>>,
}

impl DeploymentRequest {
    pub fn validate_selection(&self, release_candidate: bool) -> Result<(), &'static str> {
        let labels = self.canary_device_labels.as_ref();
        let ids = self.canary_device_ids.as_ref();
        if !release_candidate {
            return if labels.is_some() || ids.is_some() {
                Err(
                    "Stable releases choose canaries automatically. Device selection is only available for release candidates.",
                )
            } else {
                Ok(())
            };
        }
        match (labels, ids) {
            (Some(labels), None)
                if !labels.is_empty() && labels.iter().all(|label| !label.trim().is_empty()) =>
            {
                Ok(())
            }
            (None, Some(ids)) if !ids.is_empty() => Ok(()),
            _ => Err(
                "Release candidates require a non-empty selection of device IDs or labels, not both.",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DeploymentRequest;

    #[test]
    fn deployment_selection_stable_is_automatic() -> serde_json::Result<()> {
        for body in [
            r#"{}"#,
            r#"{"canary_device_ids":null,"canary_device_labels":null}"#,
        ] {
            let request: DeploymentRequest = serde_json::from_str(body)?;
            assert!(request.validate_selection(false).is_ok());
        }
        for body in [
            r#"{"canary_device_ids":[1]}"#,
            r#"{"canary_device_labels":["site=lab"]}"#,
            r#"{"canary_device_ids":[]}"#,
        ] {
            let request: DeploymentRequest = serde_json::from_str(body)?;
            assert!(request.validate_selection(false).is_err());
        }
        Ok(())
    }

    #[test]
    fn deployment_selection_candidates_require_one_nonempty_selector() -> serde_json::Result<()> {
        for body in [
            r#"{"canary_device_ids":[1,2]}"#,
            r#"{"canary_device_labels":["site=lab"]}"#,
        ] {
            let request: DeploymentRequest = serde_json::from_str(body)?;
            assert!(request.validate_selection(true).is_ok());
        }
        for body in [
            r#"{}"#,
            r#"{"canary_device_ids":[]}"#,
            r#"{"canary_device_labels":[]}"#,
            r#"{"canary_device_labels":[" "]}"#,
            r#"{"canary_device_ids":[1],"canary_device_labels":["site=lab"]}"#,
        ] {
            let request: DeploymentRequest = serde_json::from_str(body)?;
            assert!(request.validate_selection(true).is_err());
        }
        Ok(())
    }
}
