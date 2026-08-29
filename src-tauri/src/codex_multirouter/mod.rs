pub mod compiler;
pub mod migration;
pub mod mutation;
pub mod projection;
pub mod provider_set;
pub mod schema;

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;

/// Resolve the Router that owns the single shared Codex live projection.
/// An active workspace wins, followed by the device-local selection and the DB fallback.
pub(crate) fn active_codex_router_id(db: &Database) -> Result<Option<String>, AppError> {
    let local = crate::settings::get_current_provider(&AppType::Codex);
    active_codex_router_id_with_local(db, local.as_deref())
}

pub(crate) fn active_codex_router_id_with_local(
    db: &Database,
    local_provider_id: Option<&str>,
) -> Result<Option<String>, AppError> {
    if let Some(profile_id) = db.get_current_profile_id("codex")? {
        let profiles = db.get_all_profiles()?;
        if let Some(profile) = profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
        {
            let payload: serde_json::Value =
                serde_json::from_str(&profile.payload).map_err(|error| {
                    AppError::Database(format!("Failed to parse profile payload: {error}"))
                })?;
            if let Some(id) = payload
                .get("providers")
                .and_then(|providers| providers.get("codex"))
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty())
            {
                if db.get_provider_by_id(id, "codex")?.is_some() {
                    return Ok(Some(id.to_string()));
                }
            }
        }
    }
    if let Some(id) = local_provider_id.filter(|id| !id.is_empty()) {
        if db.get_provider_by_id(id, "codex")?.is_some() {
            return Ok(Some(id.to_string()));
        }
    }
    db.get_current_provider("codex")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Profile;
    use crate::provider::Provider;

    #[test]
    fn stale_profile_provider_falls_back_to_device_local_provider() {
        let db = Database::memory().expect("memory db");
        db.save_provider(
            "codex",
            &Provider::with_id(
                "router-local".to_string(),
                "Local Router".to_string(),
                serde_json::json!({}),
                None,
            ),
        )
        .expect("save local router");
        db.save_profile(&Profile {
            id: "workspace".to_string(),
            name: "Workspace".to_string(),
            payload: r#"{"providers":{"codex":"deleted-router"}}"#.to_string(),
            sort_order: None,
            created_at: Some(1),
            updated_at: Some(1),
        })
        .expect("save profile");
        db.set_current_profile_id("codex", Some("workspace"))
            .expect("activate profile");

        assert_eq!(
            active_codex_router_id_with_local(&db, Some("router-local"))
                .expect("resolve active router"),
            Some("router-local".to_string())
        );
    }
}
