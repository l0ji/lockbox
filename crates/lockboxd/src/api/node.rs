//! Implementation of routes for metadata about the node itself.

use std::collections::HashMap;

use axum::{Json, Router, extract::State, routing::get};
use lockbox_core::settings::{PATH as SETTINGS_PATH, Settings, SettingsChange};
use lockbox_proto::api::node::{HEALTH_PATH, Health, STATUS_PATH, Status};
use lockbox_store::{Error as StoreError, local::settings};

use super::{ApiJson, Failure};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(HEALTH_PATH, get(health))
        .route(STATUS_PATH, get(status))
        .route(SETTINGS_PATH, get(read_settings).patch(change_settings))
}

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

async fn status(State(state): State<AppState>) -> Json<Status> {
    Json(state.node().status())
}

async fn read_settings(State(state): State<AppState>) -> Result<Json<Settings>, Failure> {
    let stored = state
        .db()
        .call(|connection| settings::load(connection))
        .await?;
    Ok(Json(parse(&stored)?))
}

async fn change_settings(
    State(state): State<AppState>,
    ApiJson(change): ApiJson<SettingsChange>,
) -> Result<Json<Settings>, Failure> {
    let changed = state
        .db()
        .call(move |connection| {
            let transaction = connection.transaction()?;
            let mut current = parse(&settings::load(&transaction)?)?;
            current.change(change);
            settings::store(&transaction, &current.stored())?;
            transaction.commit()?;
            Ok(current)
        })
        .await?;
    Ok(Json(changed))
}

/// Read the settings from the text that the database stores.
fn parse(stored: &HashMap<String, String>) -> Result<Settings, StoreError> {
    Settings::from_stored(stored).map_err(|err| StoreError::Invalid(err.to_string()))
}
