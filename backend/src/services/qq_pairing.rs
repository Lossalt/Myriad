//! QQ C2C pairing entry: classify inbound text, then shared pairing I/O.

use myriad_agent_rules::channel::{
    InboundC2cText, InboundDecision, PAIRING_REQUIRED_REPLY, PairingBindResult, PairingLookup,
    ingest_c2c_text, pairing_bind_reply,
};
use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use crate::services::channel_pairing::{self, QQ};

pub use crate::services::channel_pairing::{IssuedPairingCode, PairingStatus};

#[cfg(test)]
pub fn mask_openid(openid: &str) -> String {
    channel_pairing::mask_openid(openid)
}

pub async fn lookup_openid(db: &DatabaseConnection, openid: &str) -> Result<PairingLookup, DbErr> {
    channel_pairing::lookup_openid(db, QQ, openid).await
}

pub async fn status_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<PairingStatus, DbErr> {
    channel_pairing::status_for_user(db, QQ, user_id).await
}

pub async fn mint_code(db: &DatabaseConnection, user_id: i32) -> Result<IssuedPairingCode, DbErr> {
    channel_pairing::mint_code(db, QQ, user_id).await
}

pub async fn unpair(db: &DatabaseConnection, user_id: i32) -> Result<bool, DbErr> {
    channel_pairing::unpair(db, QQ, user_id).await
}

pub async fn consume_code(
    db: &DatabaseConnection,
    openid: &str,
    raw_code: &str,
) -> Result<PairingBindResult, DbErr> {
    channel_pairing::consume_code(db, QQ, openid, raw_code).await
}

/// Gateway worker entry: classify the C2C text, bind pairing codes, or start Work.
pub async fn handle_inbound_c2c(event: InboundC2cText, auth_header: &str) {
    let Ok(db) = crate::services::tapp_registry::database() else {
        warn!("QQ pairing skipped: database is not connected");
        return;
    };
    let pairing = match lookup_openid(&db, &event.user_openid).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "QQ pairing lookup failed");
            return;
        }
    };
    match ingest_c2c_text(&event, pairing, false) {
        InboundDecision::Duplicate { .. } => {}
        InboundDecision::PairingRequired { reply, msg_id, .. } => {
            send_passive_text(&db, auth_header, &event.user_openid, &reply, &msg_id).await;
        }
        InboundDecision::ConsumePairingCode {
            user_openid,
            code,
            msg_id,
        } => {
            let result = match consume_code(&db, &user_openid, &code).await {
                Ok(value) => value,
                Err(error) => {
                    warn!(error = %error, "QQ pairing consume failed");
                    PairingBindResult::InvalidOrExpired
                }
            };
            if let PairingBindResult::Bound { user_id } = result {
                info!(user_id, "QQ C2C paired");
            }
            send_passive_text(
                &db,
                auth_header,
                &event.user_openid,
                pairing_bind_reply(result),
                &msg_id,
            )
            .await;
        }
        InboundDecision::StartWork {
            user_id,
            input,
            session_key,
            msg_id,
            ..
        } => {
            crate::services::qq_work::start_paired_work_with_images(
                &db,
                user_id,
                &event.user_openid,
                &event.user_openid,
                &input,
                &event.images,
                &session_key,
                &msg_id,
                auth_header,
            )
            .await;
        }
    }
}

/// Pairing replies go through the same sender as Work replies: per-message
/// `msg_seq` from the shared sequence table (a second reply to one message
/// with a fixed seq is rejected as a duplicate) and the fallback to an active
/// message once the passive reply window has closed.
async fn send_passive_text(
    db: &DatabaseConnection,
    auth_header: &str,
    openid: &str,
    content: &str,
    msg_id: &str,
) {
    if msg_id.is_empty() {
        return;
    }
    if let Err(kind) =
        crate::services::qq_work::send_c2c(db, auth_header, openid, content, Some(msg_id)).await
    {
        warn!(?kind, "QQ pairing reply failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_openid_without_returning_the_full_id() {
        assert_eq!(mask_openid("abcdefg"), "****defg");
        assert_eq!(mask_openid("ab"), "****");
        assert_eq!(mask_openid(""), "");
        assert!(!mask_openid("openid-secret-value").contains("openid-secret"));
    }

    #[test]
    fn unpaired_plain_text_is_not_a_work_request() {
        let event = InboundC2cText {
            msg_id: "m1".into(),
            user_openid: "oid".into(),
            content: "帮我查天气".into(),
            images: Vec::new(),
        };
        let decision = ingest_c2c_text(&event, PairingLookup::Unpaired, false);
        match decision {
            InboundDecision::PairingRequired { reply, .. } => {
                assert_eq!(reply, PAIRING_REQUIRED_REPLY);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pairing_channel_is_qq() {
        assert_eq!(QQ.provider, "qq");
    }
}
