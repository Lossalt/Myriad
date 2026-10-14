//! The group's ledger as the site keeps it: how it types and how long she
//! takes to answer, as numbers only (see `myriad_merope::group::Ledger`).

use super::*;

/// Runtime-registry namespace of each group's talk ledger.
pub(super) const LEDGER_NAMESPACE: &str = "group_talk_ledger";
pub(super) const LEDGER_DAYS: i64 = 30;
/// Who she is in the ledger.
const HER: &str = myriad_merope::group::HER;

/// Each group's talk between `start` and `end` (unix seconds), as counts:
/// lines said, hers among them, and how many others spoke.
pub(crate) async fn talk_between(
    db: &DatabaseConnection,
    start: i64,
    end: i64,
) -> Vec<myriad_merope::vitals::GroupTalk> {
    let rows = crate::services::runtime_registry::list(db, LEDGER_NAMESPACE, None, None)
        .await
        .unwrap_or_default();
    let mut talk = Vec::new();
    for row in rows {
        let Ok(ledger) = serde_json::from_value::<myriad_merope::group::Ledger>(row.payload) else {
            continue;
        };
        let said: Vec<_> = ledger
            .messages
            .iter()
            .filter(|typed| typed.at >= start && typed.at < end)
            .collect();
        if said.is_empty() {
            continue;
        }
        let hers = said.iter().filter(|typed| typed.by == HER).count();
        let others = said
            .iter()
            .filter(|typed| typed.by != HER)
            .map(|typed| typed.by.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len();
        talk.push(myriad_merope::vitals::GroupTalk {
            venue: row.record_id,
            lines: said.len() as u64,
            hers: hers as u64,
            others: others as u64,
        });
    }
    talk
}
