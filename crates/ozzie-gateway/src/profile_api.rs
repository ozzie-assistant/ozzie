use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;

use ozzie_core::profile::{FsProfileRepository, ProfileRepository};

use crate::AppState;

/// Returns the full user profile.
pub async fn get_profile(State(state): State<AppState>) -> impl IntoResponse {
    let repo = FsProfileRepository::new(&state.ozzie_path);
    match repo.load().await {
        Ok(Some(profile)) => Json(serde_json::json!(profile)),
        Ok(None) => Json(serde_json::json!({"error": "no profile found"})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

#[derive(serde::Deserialize)]
pub struct UpdateProfileRequest {
    pub name: Option<String>,
    pub tone: Option<String>,
    pub language: Option<String>,
}

/// Updates the user profile (partial update).
pub async fn update_profile(
    State(state): State<AppState>,
    Json(req): Json<UpdateProfileRequest>,
) -> impl IntoResponse {
    let repo = FsProfileRepository::new(&state.ozzie_path);
    let mut profile = match repo.load().await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return Json(serde_json::json!({"error": "no profile found — run onboarding first"}));
        }
        Err(e) => return Json(serde_json::json!({"error": e.to_string()})),
    };

    if let Some(name) = req.name {
        profile.name = name;
    }
    if let Some(tone) = req.tone {
        profile.tone = if tone.is_empty() { None } else { Some(tone) };
    }
    if let Some(lang) = req.language {
        profile.language = if lang.is_empty() { None } else { Some(lang) };
    }
    profile.updated_at = chrono::Utc::now().date_naive();

    match repo.save(&profile).await {
        Ok(()) => Json(serde_json::json!({"ok": true})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// Returns only the whoami entries.
pub async fn get_whoami(State(state): State<AppState>) -> impl IntoResponse {
    let repo = FsProfileRepository::new(&state.ozzie_path);
    match repo.load().await {
        Ok(Some(profile)) => Json(serde_json::json!({"whoami": profile.whoami})),
        Ok(None) => Json(serde_json::json!({"whoami": []})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}
