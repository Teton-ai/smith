use serde::Serialize;
use smith::utils::schema::NetworkInfo;
use utoipa::ToSchema;

use super::route::{DeviceExtendedTestResult, MinuteStats};

#[derive(Debug, Serialize, ToSchema)]
pub struct Evaluation {
    pub aggregate: AggregateEvaluation,
    pub per_device: Vec<PerDeviceEvaluation>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AggregateEvaluation {
    /// "Stable", "Moderate Degradation", or "Bandwidth Degrades Under Load"
    pub bandwidth_health: String,
    /// Percentage change from first to last minute average (negative = degradation)
    pub bandwidth_health_trend_percent: f64,
    /// "Fast", "Moderate", or "Slow"
    pub speed_tier: String,
    pub average_download_mbps: f64,
    /// Coefficient of variation as a percentage (stdDev/mean * 100)
    pub coefficient_of_variation: f64,
    /// "Consistent", "Variable", or "Poor Coverage"
    pub coverage_quality: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PerDeviceEvaluation {
    pub device_id: i32,
    pub serial_number: String,
    /// "Degrading", "Variable", "Fast", "Moderate", or "Slow"
    pub label: String,
    pub diagnoses: Vec<String>,
}

pub fn evaluate(results: &[DeviceExtendedTestResult]) -> Evaluation {
    let aggregate = compute_aggregate(results);
    let per_device = results.iter().map(compute_per_device).collect();
    Evaluation {
        aggregate,
        per_device,
    }
}

fn compute_aggregate(results: &[DeviceExtendedTestResult]) -> AggregateEvaluation {
    let completed: Vec<&DeviceExtendedTestResult> = results
        .iter()
        .filter(|r| {
            r.status == "completed"
                && r.minute_stats
                    .as_ref()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
        })
        .collect();

    if completed.is_empty() {
        return AggregateEvaluation {
            bandwidth_health: "Stable".to_string(),
            bandwidth_health_trend_percent: 0.0,
            speed_tier: "Slow".to_string(),
            average_download_mbps: 0.0,
            coefficient_of_variation: 0.0,
            coverage_quality: "Consistent".to_string(),
        };
    }

    // Per-device averages (used for CV calculation — matches InsightsCards.tsx)
    let device_avg_speeds: Vec<f64> = completed
        .iter()
        .filter_map(|r| {
            let stats = r.minute_stats.as_ref()?;
            if stats.is_empty() {
                return None;
            }
            let sum: f64 = stats.iter().map(|s| s.download.average_mbps).sum();
            Some(sum / stats.len() as f64)
        })
        .collect();

    let overall_avg = mean(&device_avg_speeds);

    // CV across device averages
    let overall_variance = variance(&device_avg_speeds);
    let cv = if overall_avg > 0.0 {
        (overall_variance.sqrt() / overall_avg) * 100.0
    } else {
        0.0
    };

    // Bandwidth trend: first vs last minute average across devices
    let first_minute_speeds: Vec<f64> = completed
        .iter()
        .filter_map(|r| {
            let stats = r.minute_stats.as_ref()?;
            stats
                .iter()
                .min_by_key(|s| s.minute)
                .map(|s| s.download.average_mbps)
        })
        .collect();

    let last_minute_speeds: Vec<f64> = completed
        .iter()
        .filter_map(|r| {
            let stats = r.minute_stats.as_ref()?;
            stats
                .iter()
                .max_by_key(|s| s.minute)
                .map(|s| s.download.average_mbps)
        })
        .collect();

    let avg_first = mean(&first_minute_speeds);
    let avg_last = mean(&last_minute_speeds);
    let trend_percent = if avg_first > 0.0 {
        ((avg_last - avg_first) / avg_first) * 100.0
    } else {
        0.0
    };

    AggregateEvaluation {
        bandwidth_health: bandwidth_health_label(trend_percent).to_string(),
        bandwidth_health_trend_percent: trend_percent,
        speed_tier: speed_tier_label(overall_avg).to_string(),
        average_download_mbps: overall_avg,
        coefficient_of_variation: cv,
        coverage_quality: coverage_quality_label(cv).to_string(),
    }
}

fn compute_per_device(result: &DeviceExtendedTestResult) -> PerDeviceEvaluation {
    let (label, diagnoses) = if result.status != "completed" {
        (result.status.clone(), vec![])
    } else {
        match &result.minute_stats {
            None => (
                "No data".to_string(),
                vec!["No data available for analysis".to_string()],
            ),
            Some(stats) if stats.is_empty() => (
                "No data".to_string(),
                vec!["No data available for analysis".to_string()],
            ),
            Some(stats) => {
                let label = compute_device_label(stats);
                let diagnoses = compute_device_diagnoses(stats, result.network_info.as_ref());
                (label, diagnoses)
            }
        }
    };

    PerDeviceEvaluation {
        device_id: result.device_id,
        serial_number: result.serial_number.clone(),
        label,
        diagnoses,
    }
}

fn compute_device_label(stats: &[MinuteStats]) -> String {
    let download_speeds: Vec<f64> = stats.iter().map(|s| s.download.average_mbps).collect();
    let avg = mean(&download_speeds);

    let std_dev = variance(&download_speeds).sqrt();

    let first = stats
        .iter()
        .min_by_key(|s| s.minute)
        .map(|s| s.download.average_mbps)
        .unwrap_or(0.0);
    let last = stats
        .iter()
        .max_by_key(|s| s.minute)
        .map(|s| s.download.average_mbps)
        .unwrap_or(0.0);
    let trend_percent = if first > 0.0 {
        ((last - first) / first) * 100.0
    } else {
        0.0
    };

    // Degradation takes priority
    if trend_percent < -20.0 {
        return "Degrading".to_string();
    }

    // High variance
    let cv_percent = if avg > 0.0 {
        (std_dev / avg) * 100.0
    } else {
        0.0
    };
    if cv_percent > 30.0 {
        return "Variable".to_string();
    }

    speed_tier_label(avg).to_string()
}

fn compute_device_diagnoses(
    stats: &[MinuteStats],
    network_info: Option<&NetworkInfo>,
) -> Vec<String> {
    let mut diagnoses = Vec::new();

    let download_speeds: Vec<f64> = stats.iter().map(|s| s.download.average_mbps).collect();
    let avg_download = mean(&download_speeds);

    let first = stats
        .iter()
        .min_by_key(|s| s.minute)
        .map(|s| s.download.average_mbps)
        .unwrap_or(0.0);
    let last = stats
        .iter()
        .max_by_key(|s| s.minute)
        .map(|s| s.download.average_mbps)
        .unwrap_or(0.0);
    let trend_percent = if first > 0.0 {
        ((last - first) / first) * 100.0
    } else {
        0.0
    };

    // Speed drop analysis
    if trend_percent < -30.0 {
        diagnoses.push(format!(
            "Speed dropped {:.0}% over test duration - possible thermal throttling or network congestion",
            trend_percent.abs()
        ));
    } else if trend_percent < -20.0 {
        diagnoses.push(format!(
            "Speed decreased {:.0}% during test - may indicate bandwidth contention",
            trend_percent.abs()
        ));
    }

    // Variance analysis
    let std_dev = variance(&download_speeds).sqrt();
    let cv = if avg_download > 0.0 {
        (std_dev / avg_download) * 100.0
    } else {
        0.0
    };

    if cv > 40.0 {
        diagnoses.push(
            "High variance suggests intermittent connection or wireless interference".to_string(),
        );
    } else if cv > 25.0 {
        diagnoses.push("Moderate speed fluctuations detected".to_string());
    }

    // Upload vs download analysis
    let upload_speeds: Vec<f64> = stats
        .iter()
        .filter_map(|s| s.upload.as_ref().map(|u| u.average_mbps))
        .collect();
    if !upload_speeds.is_empty() {
        let avg_upload = mean(&upload_speeds);
        if avg_download > avg_upload * 10.0 {
            diagnoses.push(
                "Upload significantly lower than download - typical for asymmetric connections"
                    .to_string(),
            );
        }
    }

    // WiFi signal analysis
    if let Some(info) = network_info
        && let Some(signal_dbm) = get_wifi_signal_dbm(info)
    {
        if signal_dbm < -75 {
            diagnoses.push(format!(
                "Weak WiFi signal ({signal_dbm} dBm) - consider moving device closer to access point"
            ));
        } else if signal_dbm < -65 {
            diagnoses.push(format!(
                "Fair WiFi signal ({signal_dbm} dBm) - signal could be improved"
            ));
        }
    }

    // Speed tier
    if avg_download < 25.0 {
        diagnoses.push("Slow connection speed may impact device operations".to_string());
    }

    if diagnoses.is_empty() {
        diagnoses.push("Connection appears healthy with consistent performance".to_string());
    }

    diagnoses
}

// -- helpers --

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn variance(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let avg = mean(values);
    values.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / values.len() as f64
}

fn bandwidth_health_label(trend_percent: f64) -> &'static str {
    if trend_percent >= -10.0 {
        "Stable"
    } else if trend_percent >= -25.0 {
        "Moderate Degradation"
    } else {
        "Bandwidth Degrades Under Load"
    }
}

fn speed_tier_label(avg_mbps: f64) -> &'static str {
    if avg_mbps >= 100.0 {
        "Fast"
    } else if avg_mbps >= 50.0 {
        "Moderate"
    } else {
        "Slow"
    }
}

fn coverage_quality_label(cv: f64) -> &'static str {
    if cv <= 20.0 {
        "Consistent"
    } else if cv <= 40.0 {
        "Variable"
    } else {
        "Poor Coverage"
    }
}

fn get_wifi_signal_dbm(info: &NetworkInfo) -> Option<i32> {
    use smith::utils::schema::NetworkDetails;
    match &info.details {
        NetworkDetails::Wifi { signal_dbm, .. } => *signal_dbm,
        _ => None,
    }
}
