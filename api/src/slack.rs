use serde_json::Value;
use tracing::error;

pub async fn send_slack_notification(slack_hook_url: &str, message: Value) {
    let client = reqwest::Client::new();
    if let Err(e) = client
        .post(slack_hook_url)
        .header("Content-Type", "application/json")
        .json(&message)
        .send()
        .await
    {
        error!("Failed to send Slack notification: {e}");
    }
}
