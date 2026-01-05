use std::time::Duration;

use axum::{
    Router,
    extract::{Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use db::models::{
    analytics::ProjectDevEx, project_repo::ProjectRepo, task::TaskStatus,
    task_status_event::TaskStatusEvent,
};
use deployment::Deployment;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use services::services::file_ranker::FileRanker;
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AnalyticsBucket {
    Day,
}

fn default_days() -> i64 {
    30
}

#[derive(Debug, Deserialize)]
pub struct AnalyticsRangeQuery {
    pub project_id: Uuid,
    /// RFC3339 timestamp (UTC recommended). If omitted, defaults to `to - days`.
    pub from: Option<String>,
    /// RFC3339 timestamp (UTC recommended). If omitted, defaults to now.
    pub to: Option<String>,
    #[serde(default = "default_days")]
    pub days: i64,
    #[serde(default)]
    pub bucket: Option<AnalyticsBucket>,
}

#[derive(Debug, Deserialize)]
pub struct BurndownQuery {
    #[serde(flatten)]
    pub range: AnalyticsRangeQuery,
    /// If true, counts `cancelled` as remaining work (default: false).
    #[serde(default)]
    pub include_cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct BurndownPoint {
    #[ts(type = "Date")]
    pub ts: DateTime<Utc>,
    pub remaining: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct BurndownResponse {
    pub points: Vec<BurndownPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CfdPoint {
    #[ts(type = "Date")]
    pub ts: DateTime<Utc>,
    pub todo: i64,
    pub inprogress: i64,
    pub inreview: i64,
    pub done: i64,
    pub cancelled: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CfdResponse {
    pub points: Vec<CfdPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CycleTimeHistogramBucket {
    /// Inclusive lower bound (hours)
    pub from_hours: i64,
    /// Exclusive upper bound (hours)
    pub to_hours: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CycleTimeResponse {
    pub sample_size: i64,
    pub mean_hours: f64,
    pub p50_hours: f64,
    pub p75_hours: f64,
    pub p90_hours: f64,
    pub p95_hours: f64,
    pub histogram: Vec<CycleTimeHistogramBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AnalyticsPoint {
    #[ts(type = "Date")]
    pub ts: DateTime<Utc>,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct FileHotspot {
    pub path: String,
    pub commit_count: i64,
    #[ts(type = "Date")]
    pub last_modified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RepoHotspots {
    pub repo_id: Uuid,
    pub repo_display_name: String,
    pub files: Vec<FileHotspot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct DevExResponse {
    pub agent_turns: Vec<AnalyticsPoint>,
    pub agent_runs: Vec<AnalyticsPoint>,
    pub tasks_touched: i64,
    pub hotspots: Vec<RepoHotspots>,
}

#[derive(Debug, Clone)]
struct NormalizedRange {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
}

fn floor_to_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
        .single()
        .expect("valid date")
}

fn parse_rfc3339_utc(s: &str) -> Result<DateTime<Utc>, ApiError> {
    let dt = DateTime::parse_from_rfc3339(s)
        .map_err(|_| ApiError::BadRequest(format!("Invalid RFC3339 timestamp: {s}")))?;
    Ok(dt.with_timezone(&Utc))
}

fn normalize_range(query: &AnalyticsRangeQuery) -> Result<NormalizedRange, ApiError> {
    let to_raw = match &query.to {
        Some(s) => parse_rfc3339_utc(s)?,
        None => Utc::now(),
    };

    let from_raw = match &query.from {
        Some(s) => parse_rfc3339_utc(s)?,
        None => to_raw - chrono::Duration::days(query.days.max(1)),
    };

    if from_raw > to_raw {
        return Err(ApiError::BadRequest("`from` must be <= `to`".to_string()));
    }

    let from = floor_to_day(from_raw);
    // Use an exclusive upper bound aligned to the next day so "today" is included.
    let to = floor_to_day(to_raw) + chrono::Duration::days(1);

    Ok(NormalizedRange { from, to })
}

fn day_buckets(range: &NormalizedRange) -> Vec<DateTime<Utc>> {
    let mut out = Vec::new();
    let mut ts = range.from;
    while ts <= range.to {
        out.push(ts);
        ts = ts + chrono::Duration::days(1);
    }
    out
}

fn status_idx(status: &TaskStatus) -> usize {
    match status {
        TaskStatus::Todo => 0,
        TaskStatus::InProgress => 1,
        TaskStatus::InReview => 2,
        TaskStatus::Done => 3,
        TaskStatus::Cancelled => 4,
    }
}

fn snapshot_cfd_counts(buckets: &[DateTime<Utc>], events: &[TaskStatusEvent]) -> Vec<[i64; 5]> {
    use std::collections::HashMap;

    let mut counts = [0i64; 5];
    let mut task_status: HashMap<Uuid, TaskStatus> = HashMap::new();

    let mut idx = 0usize;
    let mut snapshots = Vec::with_capacity(buckets.len());

    for &bucket_ts in buckets {
        while idx < events.len() && events[idx].created_at <= bucket_ts {
            let ev = &events[idx];
            idx += 1;

            if let Some(prev) = task_status.insert(ev.task_id, ev.status.clone()) {
                counts[status_idx(&prev)] -= 1;
            }
            counts[status_idx(&ev.status)] += 1;
        }

        snapshots.push(counts);
    }

    snapshots
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    project_id: Uuid,
    from_ts: i64,
    to_ts: i64,
    include_cancelled: bool,
}

static BURNDOWN_CACHE: Lazy<moka::future::Cache<CacheKey, BurndownResponse>> = Lazy::new(|| {
    moka::future::Cache::builder()
        .time_to_live(Duration::from_secs(30))
        .max_capacity(500)
        .build()
});

static CFD_CACHE: Lazy<moka::future::Cache<CacheKey, CfdResponse>> = Lazy::new(|| {
    moka::future::Cache::builder()
        .time_to_live(Duration::from_secs(30))
        .max_capacity(500)
        .build()
});

static CYCLE_TIME_CACHE: Lazy<moka::future::Cache<CacheKey, CycleTimeResponse>> = Lazy::new(|| {
    moka::future::Cache::builder()
        .time_to_live(Duration::from_secs(30))
        .max_capacity(500)
        .build()
});

static DEVEX_CACHE: Lazy<moka::future::Cache<CacheKey, DevExResponse>> = Lazy::new(|| {
    moka::future::Cache::builder()
        .time_to_live(Duration::from_secs(30))
        .max_capacity(200)
        .build()
});

pub async fn get_burndown(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<BurndownQuery>,
) -> Result<ResponseJson<ApiResponse<BurndownResponse>>, ApiError> {
    let range = normalize_range(&query.range)?;
    let cache_key = CacheKey {
        project_id: query.range.project_id,
        from_ts: range.from.timestamp(),
        to_ts: range.to.timestamp(),
        include_cancelled: query.include_cancelled,
    };

    if let Some(cached) = BURNDOWN_CACHE.get(&cache_key).await {
        return Ok(ResponseJson(ApiResponse::success(cached)));
    }

    let buckets = day_buckets(&range);
    let events = TaskStatusEvent::list_for_project_up_to(
        &deployment.db().pool,
        query.range.project_id,
        range.to,
    )
    .await?;

    let snapshots = snapshot_cfd_counts(&buckets, &events);

    let points = buckets
        .into_iter()
        .zip(snapshots.into_iter())
        .map(|(ts, c)| {
            let remaining = if query.include_cancelled {
                c[0] + c[1] + c[2] + c[4]
            } else {
                c[0] + c[1] + c[2]
            };
            let total = c.iter().sum();
            BurndownPoint {
                ts,
                remaining,
                total,
            }
        })
        .collect::<Vec<_>>();

    let resp = BurndownResponse { points };
    BURNDOWN_CACHE.insert(cache_key, resp.clone()).await;
    Ok(ResponseJson(ApiResponse::success(resp)))
}

pub async fn get_cfd(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<AnalyticsRangeQuery>,
) -> Result<ResponseJson<ApiResponse<CfdResponse>>, ApiError> {
    let range = normalize_range(&query)?;
    let cache_key = CacheKey {
        project_id: query.project_id,
        from_ts: range.from.timestamp(),
        to_ts: range.to.timestamp(),
        include_cancelled: false,
    };

    if let Some(cached) = CFD_CACHE.get(&cache_key).await {
        return Ok(ResponseJson(ApiResponse::success(cached)));
    }

    let buckets = day_buckets(&range);
    let events =
        TaskStatusEvent::list_for_project_up_to(&deployment.db().pool, query.project_id, range.to)
            .await?;
    let snapshots = snapshot_cfd_counts(&buckets, &events);

    let points = buckets
        .into_iter()
        .zip(snapshots.into_iter())
        .map(|(ts, c)| CfdPoint {
            ts,
            todo: c[0],
            inprogress: c[1],
            inreview: c[2],
            done: c[3],
            cancelled: c[4],
        })
        .collect();

    let resp = CfdResponse { points };
    CFD_CACHE.insert(cache_key, resp.clone()).await;
    Ok(ResponseJson(ApiResponse::success(resp)))
}

pub async fn get_cycle_time(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<AnalyticsRangeQuery>,
) -> Result<ResponseJson<ApiResponse<CycleTimeResponse>>, ApiError> {
    let range = normalize_range(&query)?;
    let cache_key = CacheKey {
        project_id: query.project_id,
        from_ts: range.from.timestamp(),
        to_ts: range.to.timestamp(),
        include_cancelled: false,
    };

    if let Some(cached) = CYCLE_TIME_CACHE.get(&cache_key).await {
        return Ok(ResponseJson(ApiResponse::success(cached)));
    }

    let events =
        TaskStatusEvent::list_for_project_up_to(&deployment.db().pool, query.project_id, range.to)
            .await?;

    use std::collections::HashMap;

    #[derive(Default)]
    struct TaskCycle {
        start: Option<DateTime<Utc>>,
        done: Option<DateTime<Utc>>,
    }

    let mut by_task: HashMap<Uuid, TaskCycle> = HashMap::new();

    for ev in events {
        let entry = by_task.entry(ev.task_id).or_default();
        match ev.status {
            TaskStatus::InProgress | TaskStatus::InReview => {
                if entry.start.is_none() {
                    entry.start = Some(ev.created_at);
                }
            }
            TaskStatus::Done => {
                if entry.start.is_some() && entry.done.is_none() {
                    entry.done = Some(ev.created_at);
                }
            }
            _ => {}
        }
    }

    let mut durations_hours = by_task
        .into_values()
        .filter_map(|c| match (c.start, c.done) {
            (Some(start), Some(done)) if done >= range.from && done <= range.to => {
                Some((done - start).num_seconds() as f64 / 3600.0)
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    durations_hours.sort_by(|a, b| a.total_cmp(b));

    fn percentile(sorted: &[f64], p: f64) -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let rank = (p * (sorted.len() - 1) as f64).round() as usize;
        sorted[rank.min(sorted.len() - 1)]
    }

    let sample_size = durations_hours.len() as i64;
    let mean_hours = if durations_hours.is_empty() {
        0.0
    } else {
        durations_hours.iter().sum::<f64>() / durations_hours.len() as f64
    };

    let p50_hours = percentile(&durations_hours, 0.50);
    let p75_hours = percentile(&durations_hours, 0.75);
    let p90_hours = percentile(&durations_hours, 0.90);
    let p95_hours = percentile(&durations_hours, 0.95);

    // Simple fixed buckets for a readable histogram in the UI.
    let bucket_edges = [0i64, 2, 6, 12, 24, 48, 72, 96, 168, 336, 672, 10_000];

    let mut histogram = Vec::new();
    for w in bucket_edges.windows(2) {
        histogram.push(CycleTimeHistogramBucket {
            from_hours: w[0],
            to_hours: w[1],
            count: 0,
        });
    }

    for &h in &durations_hours {
        let h_i = h.floor() as i64;
        if let Some(bucket) = histogram
            .iter_mut()
            .find(|b| h_i >= b.from_hours && h_i < b.to_hours)
        {
            bucket.count += 1;
        }
    }

    let resp = CycleTimeResponse {
        sample_size,
        mean_hours,
        p50_hours,
        p75_hours,
        p90_hours,
        p95_hours,
        histogram,
    };

    CYCLE_TIME_CACHE.insert(cache_key, resp.clone()).await;
    Ok(ResponseJson(ApiResponse::success(resp)))
}

pub async fn get_devex(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<AnalyticsRangeQuery>,
) -> Result<ResponseJson<ApiResponse<DevExResponse>>, ApiError> {
    let range = normalize_range(&query)?;
    let cache_key = CacheKey {
        project_id: query.project_id,
        from_ts: range.from.timestamp(),
        to_ts: range.to.timestamp(),
        include_cancelled: false,
    };

    if let Some(cached) = DEVEX_CACHE.get(&cache_key).await {
        return Ok(ResponseJson(ApiResponse::success(cached)));
    }

    let pool = &deployment.db().pool;

    let agent_turn_times =
        ProjectDevEx::list_agent_turn_timestamps(pool, query.project_id, range.from, range.to)
            .await?;
    let agent_run_times =
        ProjectDevEx::list_agent_run_timestamps(pool, query.project_id, range.from, range.to)
            .await?;
    let tasks_touched =
        ProjectDevEx::count_tasks_touched(pool, query.project_id, range.from, range.to).await?;

    let buckets = day_buckets(&range);

    fn bucketize(
        range_from: DateTime<Utc>,
        buckets: &[DateTime<Utc>],
        times: &[DateTime<Utc>],
    ) -> Vec<AnalyticsPoint> {
        let mut counts = vec![0i64; buckets.len()];
        for t in times {
            let days = (*t - range_from).num_days();
            if days < 0 {
                continue;
            }
            let idx = days as usize;
            if idx < counts.len() {
                counts[idx] += 1;
            }
        }
        buckets
            .iter()
            .copied()
            .zip(counts.into_iter())
            .map(|(ts, value)| AnalyticsPoint { ts, value })
            .collect()
    }

    let agent_turns_series = bucketize(range.from, &buckets, &agent_turn_times);
    let agent_runs_series = bucketize(range.from, &buckets, &agent_run_times);

    let repos = ProjectRepo::find_repos_for_project(pool, query.project_id).await?;
    let file_ranker = FileRanker::new();

    let mut hotspots = Vec::new();
    for repo in repos {
        let stats = match file_ranker.get_stats(&repo.path).await {
            Ok(stats) => stats,
            Err(_) => std::sync::Arc::new(Default::default()),
        };
        let mut files = stats
            .iter()
            .map(|(path, stat)| FileHotspot {
                path: path.clone(),
                commit_count: stat.commit_count as i64,
                last_modified_at: stat.last_time,
            })
            .collect::<Vec<_>>();
        files.sort_by(|a, b| {
            b.commit_count
                .cmp(&a.commit_count)
                .then_with(|| b.last_modified_at.cmp(&a.last_modified_at))
        });
        files.truncate(20);

        hotspots.push(RepoHotspots {
            repo_id: repo.id,
            repo_display_name: repo.display_name,
            files,
        });
    }

    let resp = DevExResponse {
        agent_turns: agent_turns_series,
        agent_runs: agent_runs_series,
        tasks_touched,
        hotspots,
    };

    DEVEX_CACHE.insert(cache_key, resp.clone()).await;
    Ok(ResponseJson(ApiResponse::success(resp)))
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new().nest(
        "/analytics",
        Router::new()
            .route("/burndown", get(get_burndown))
            .route("/cfd", get(get_cfd))
            .route("/cycle-time", get(get_cycle_time))
            .route("/devex", get(get_devex)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_track_latest_status_per_task() {
        let project_id = Uuid::new_v4();
        let t1 = Uuid::new_v4();
        let t2 = Uuid::new_v4();

        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let buckets = vec![base, base + chrono::Duration::days(1)];

        let events = vec![
            TaskStatusEvent {
                id: Uuid::new_v4(),
                task_id: t1,
                project_id,
                status: TaskStatus::Todo,
                created_at: base,
            },
            TaskStatusEvent {
                id: Uuid::new_v4(),
                task_id: t2,
                project_id,
                status: TaskStatus::Todo,
                created_at: base + chrono::Duration::hours(1),
            },
            TaskStatusEvent {
                id: Uuid::new_v4(),
                task_id: t1,
                project_id,
                status: TaskStatus::InProgress,
                created_at: base + chrono::Duration::hours(2),
            },
        ];

        let snaps = snapshot_cfd_counts(&buckets, &events);
        assert_eq!(snaps.len(), 2);

        // At t=base, t1 is todo (1). t2 hasn't been created yet.
        assert_eq!(snaps[0], [1, 0, 0, 0, 0]);

        // At t=base+1d, both tasks exist; t1 is inprogress, t2 is todo.
        assert_eq!(snaps[1], [1, 1, 0, 0, 0]);
    }
}
