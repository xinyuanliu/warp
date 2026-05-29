use super::{MemberUsageRow, SourceFilter};
use crate::workspaces::workspace::{
    AiCreditsUsageAndCostSubjectType, AiCreditsUsageAndCostType, AiCreditsUsageBucket,
    AiCreditsUsageSource, BillingCycleUsageEntry,
};

const VIEWER_UID: &str = "viewer-uid";
const OTHER_UID: &str = "other-uid";

fn entry(
    subject_type: AiCreditsUsageAndCostSubjectType,
    subject_uid: Option<&str>,
    usage_source: AiCreditsUsageSource,
    credits_used: i32,
    cost_cents: i32,
) -> BillingCycleUsageEntry {
    BillingCycleUsageEntry {
        subject_type,
        subject_uid: subject_uid.map(|s| s.to_string()),
        subject_display_name: None,
        cost_type: AiCreditsUsageAndCostType::BaseLimit,
        usage_bucket: AiCreditsUsageBucket::Ai,
        usage_source,
        credits_used,
        cost_cents,
    }
}

/// Viewer-attributed entry with an explicit cost type and usage bucket, for
/// exercising the base-vs-total split.
fn entry_typed(
    cost_type: AiCreditsUsageAndCostType,
    usage_bucket: AiCreditsUsageBucket,
    credits_used: i32,
    cost_cents: i32,
) -> BillingCycleUsageEntry {
    BillingCycleUsageEntry {
        subject_type: AiCreditsUsageAndCostSubjectType::User,
        subject_uid: Some(VIEWER_UID.to_string()),
        subject_display_name: None,
        cost_type,
        usage_bucket,
        usage_source: AiCreditsUsageSource::Local,
        credits_used,
        cost_cents,
    }
}

#[test]
fn build_own_usage_row_drops_team_subject_entries() {
    // Team-aggregate rows belong to "everyone else" by construction; they
    // must never contribute to the viewer's own row totals.
    let entries = vec![
        entry(
            AiCreditsUsageAndCostSubjectType::User,
            Some(VIEWER_UID),
            AiCreditsUsageSource::Local,
            10,
            5,
        ),
        entry(
            AiCreditsUsageAndCostSubjectType::Team,
            None,
            AiCreditsUsageSource::Aggregate,
            999,
            999,
        ),
    ];
    let row = MemberUsageRow::for_viewer(
        &entries,
        Some(VIEWER_UID),
        "viewer".to_string(),
        None,
        SourceFilter::All,
    );
    assert_eq!(row.total_credits, 10);
    assert_eq!(row.total_cost_cents, 5);
}

#[test]
fn build_own_usage_row_drops_other_users_entries() {
    let entries = vec![
        entry(
            AiCreditsUsageAndCostSubjectType::User,
            Some(VIEWER_UID),
            AiCreditsUsageSource::Local,
            10,
            0,
        ),
        entry(
            AiCreditsUsageAndCostSubjectType::User,
            Some(OTHER_UID),
            AiCreditsUsageSource::Local,
            999,
            999,
        ),
    ];
    let row = MemberUsageRow::for_viewer(
        &entries,
        Some(VIEWER_UID),
        "viewer".to_string(),
        None,
        SourceFilter::All,
    );
    assert_eq!(row.total_credits, 10);
    assert_eq!(row.total_cost_cents, 0);
}

#[test]
fn build_own_usage_row_local_filter_drops_cloud_entries() {
    let entries = vec![
        entry(
            AiCreditsUsageAndCostSubjectType::User,
            Some(VIEWER_UID),
            AiCreditsUsageSource::Local,
            10,
            0,
        ),
        entry(
            AiCreditsUsageAndCostSubjectType::User,
            Some(VIEWER_UID),
            AiCreditsUsageSource::Cloud,
            20,
            0,
        ),
    ];
    let row = MemberUsageRow::for_viewer(
        &entries,
        Some(VIEWER_UID),
        "viewer".to_string(),
        None,
        SourceFilter::Local,
    );
    assert_eq!(row.total_credits, 10);
}

#[test]
fn build_own_usage_row_cloud_filter_drops_local_entries() {
    let entries = vec![
        entry(
            AiCreditsUsageAndCostSubjectType::User,
            Some(VIEWER_UID),
            AiCreditsUsageSource::Local,
            10,
            0,
        ),
        entry(
            AiCreditsUsageAndCostSubjectType::User,
            Some(VIEWER_UID),
            AiCreditsUsageSource::Cloud,
            20,
            0,
        ),
    ];
    let row = MemberUsageRow::for_viewer(
        &entries,
        Some(VIEWER_UID),
        "viewer".to_string(),
        None,
        SourceFilter::Cloud,
    );
    assert_eq!(row.total_credits, 20);
}

#[test]
fn build_own_usage_row_surfaces_supplied_base_limit() {
    let row = MemberUsageRow::for_viewer(
        &[],
        Some(VIEWER_UID),
        "viewer".to_string(),
        Some(1500),
        SourceFilter::All,
    );
    assert_eq!(row.base_limit, Some(1500));
}

#[test]
fn base_credits_excludes_add_on_spend() {
    // Mirrors the upgrade-mid-cycle case: base usage plus add-on (bonus grant)
    // usage in the same cycle. The `used / limit` gauge must count base only,
    // while the bar/tooltip total still reflects everything that was spent.
    let entries = vec![
        entry_typed(
            AiCreditsUsageAndCostType::BaseLimit,
            AiCreditsUsageBucket::Ai,
            3376,
            0,
        ),
        entry_typed(
            AiCreditsUsageAndCostType::BonusGrant,
            AiCreditsUsageBucket::Ai,
            4183,
            7366,
        ),
    ];
    let row = MemberUsageRow::for_viewer(
        &entries,
        Some(VIEWER_UID),
        "viewer".to_string(),
        Some(18000),
        SourceFilter::All,
    );
    // Gauge numerator: base only.
    assert_eq!(row.base_credits, 3376);
    // Bar + tooltip total: base + add-ons.
    assert_eq!(row.total_credits, 7559);
    assert_eq!(row.total_cost_cents, 7366);
    assert_eq!(row.base_limit, Some(18000));
}

#[test]
fn base_credits_counts_ai_and_compute_but_not_platform() {
    // Base usage draws down the AI request limit for the AI and Compute buckets
    // only (see get_base_limits_usage.sql); Platform base usage is accounted
    // separately and must not inflate the gauge numerator.
    let entries = vec![
        entry_typed(
            AiCreditsUsageAndCostType::BaseLimit,
            AiCreditsUsageBucket::Ai,
            100,
            0,
        ),
        entry_typed(
            AiCreditsUsageAndCostType::BaseLimit,
            AiCreditsUsageBucket::Compute,
            50,
            0,
        ),
        entry_typed(
            AiCreditsUsageAndCostType::BaseLimit,
            AiCreditsUsageBucket::Platform,
            25,
            0,
        ),
    ];
    let row = MemberUsageRow::for_viewer(
        &entries,
        Some(VIEWER_UID),
        "viewer".to_string(),
        Some(18000),
        SourceFilter::All,
    );
    assert_eq!(row.base_credits, 150);
    assert_eq!(row.total_credits, 175);
}
