use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;

use ozzie_core::domain::MemorySchema;

use crate::AppState;

/// Lists all memory entries (metadata only).
pub async fn list_entries(State(state): State<AppState>) -> impl IntoResponse {
    let Some(store) = &state.memory_store else {
        return Json(serde_json::json!({"entries": []}));
    };

    match store.list_entries().await {
        Ok(entries) => Json(serde_json::json!({"entries": entries})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// Gets a single memory entry with content.
pub async fn get_entry(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(store) = &state.memory_store else {
        return Json(serde_json::json!({"error": "memory store not available"}));
    };

    match store.get_entry(&id).await {
        Ok((meta, content)) => Json(serde_json::json!({
            "id": meta.id,
            "title": meta.title,
            "type": meta.memory_type,
            "tags": meta.tags,
            "source": meta.source,
            "importance": meta.importance,
            "confidence": meta.confidence,
            "created_at": meta.created_at,
            "updated_at": meta.updated_at,
            "content": content,
        })),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

#[derive(serde::Deserialize)]
pub struct SearchQuery {
    q: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
}

fn default_search_limit() -> usize {
    20
}

/// Searches memory entries via FTS5.
pub async fn search_entries(
    Query(query): Query<SearchQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(store) = &state.memory_store else {
        return Json(serde_json::json!({"results": []}));
    };

    match store.search_text(&query.q, query.limit).await {
        Ok(results) => Json(serde_json::json!({"results": results})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// Lists all wiki pages (metadata only).
pub async fn list_pages(State(state): State<AppState>) -> impl IntoResponse {
    let Some(store) = &state.page_store else {
        return Json(serde_json::json!({"pages": []}));
    };

    match store.list().await {
        Ok(pages) => Json(serde_json::json!({"pages": pages})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// Gets a single wiki page by slug with content.
pub async fn get_page(
    Path(slug): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(store) = &state.page_store else {
        return Json(serde_json::json!({"error": "page store not available"}));
    };

    match store.get_by_slug(&slug).await {
        Ok((page, content)) => Json(serde_json::json!({
            "id": page.id,
            "title": page.title,
            "slug": page.slug,
            "tags": page.tags,
            "source_ids": page.source_ids,
            "revision": page.revision,
            "created_at": page.created_at,
            "updated_at": page.updated_at,
            "content": content,
        })),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// Searches wiki pages via FTS5.
pub async fn search_pages(
    Query(query): Query<SearchQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(store) = &state.page_store else {
        return Json(serde_json::json!({"results": []}));
    };

    match store.search_text(&query.q, query.limit).await {
        Ok(results) => Json(serde_json::json!({"results": results})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// Returns a structured index of all pages and uncategorized entries.
pub async fn get_index(State(state): State<AppState>) -> impl IntoResponse {
    let pages = match &state.page_store {
        Some(store) => store.list().await.unwrap_or_default(),
        None => Vec::new(),
    };

    let entries = match &state.memory_store {
        Some(store) => store.list_entries().await.unwrap_or_default(),
        None => Vec::new(),
    };

    let covered: std::collections::HashSet<String> = pages
        .iter()
        .flat_map(|p| p.source_ids.iter().cloned())
        .collect();

    let uncategorized_count = entries.iter().filter(|e| !covered.contains(&e.id)).count();

    let page_summaries: Vec<serde_json::Value> = pages
        .iter()
        .map(|p| {
            serde_json::json!({
                "title": p.title,
                "slug": p.slug,
                "source_count": p.source_ids.len(),
                "revision": p.revision,
            })
        })
        .collect();

    Json(serde_json::json!({
        "pages": page_summaries,
        "total_entries": entries.len(),
        "uncategorized_count": uncategorized_count,
    }))
}

/// Returns the current memory schema.
pub async fn get_schema(State(state): State<AppState>) -> impl IntoResponse {
    let schema = MemorySchema::load(&state.ozzie_path);
    Json(serde_json::json!({
        "max_page_chars": schema.max_page_chars,
        "language": schema.language,
        "instructions": schema.instructions,
    }))
}
