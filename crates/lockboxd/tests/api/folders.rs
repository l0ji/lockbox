//! Tests of the folder routes.

use lockbox_proto::api::{
    ApiError, Code,
    reader::{FOLDERS_PATH, folder_path},
};
use lockbox_types::folder::{Folder, Kind};

use crate::common::{Daemon, json, request};
use crate::fixtures::folder;

#[tokio::test]
async fn creates_and_lists_a_folder_tree() {
    let daemon = Daemon::start().await;
    let root = folder(&daemon, r#"{"name":"Reading"}"#).await;
    assert_eq!(root.kind, Kind::Manual);
    let child = folder(
        &daemon,
        &serde_json::json!({ "name": "Rust", "parent_id": root.id }).to_string(),
    )
    .await;
    assert_eq!(child.parent_id, Some(root.id));

    let (code, body) = request(daemon.socket(), "GET", FOLDERS_PATH, None).await;
    assert_eq!(code, 200, "{body}");
    let folders: Vec<Folder> = json(&body);
    assert_eq!(
        folders
            .iter()
            .map(|folder| folder.name.as_str())
            .collect::<Vec<_>>(),
        ["Reading", "Rust"]
    );

    let (code, body) = request(daemon.socket(), "GET", &folder_path(root.id), None).await;
    assert_eq!(code, 200, "{body}");
    let read: Folder = json(&body);
    assert_eq!(read, root);
}

#[tokio::test]
async fn creates_a_dynamic_folder_with_its_topics() {
    let daemon = Daemon::start().await;
    let folder = folder(
        &daemon,
        r#"{"name":"Rust","kind":"dynamic","topics":["Rust","Async"]}"#,
    )
    .await;
    assert_eq!(folder.kind, Kind::Dynamic);
    assert_eq!(
        folder
            .topics
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["Async", "Rust"]
    );
}

#[tokio::test]
async fn changes_the_name_place_and_topics_of_a_folder() {
    let daemon = Daemon::start().await;
    let root = folder(&daemon, r#"{"name":"Reading"}"#).await;
    let dynamic = folder(
        &daemon,
        r#"{"name":"Dynamic","kind":"dynamic","topics":["One"]}"#,
    )
    .await;

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &folder_path(dynamic.id),
        Some(
            &serde_json::json!({
                "name": "Async",
                "parent_id": root.id,
                "position": 0,
                "topics": ["Rust", "Async"],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let changed: Folder = json(&body);
    assert_eq!(changed.name, "Async");
    assert_eq!(changed.parent_id, Some(root.id));
    assert_eq!(changed.position, 0);
    assert_eq!(
        changed
            .topics
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["Async", "Rust"]
    );
}

#[tokio::test]
async fn refuses_a_taken_folder_name() {
    let daemon = Daemon::start().await;
    folder(&daemon, r#"{"name":"One"}"#).await;
    let two = folder(&daemon, r#"{"name":"Two"}"#).await;

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &folder_path(two.id),
        Some(r#"{"name":"One"}"#),
    )
    .await;
    assert_eq!(code, 409, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Conflict);
}

#[tokio::test]
async fn refuses_a_cycle() {
    let daemon = Daemon::start().await;
    let root = folder(&daemon, r#"{"name":"Reading"}"#).await;
    let child = folder(
        &daemon,
        &serde_json::json!({ "name": "Rust", "parent_id": root.id }).to_string(),
    )
    .await;

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &folder_path(root.id),
        Some(&serde_json::json!({ "parent_id": child.id }).to_string()),
    )
    .await;
    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
}

#[tokio::test]
async fn refuses_topics_on_a_manual_folder() {
    let daemon = Daemon::start().await;
    let manual = folder(&daemon, r#"{"name":"Reading"}"#).await;

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &folder_path(manual.id),
        Some(r#"{"topics":["Rust"]}"#),
    )
    .await;
    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
}

#[tokio::test]
async fn deletes_a_folder_and_unfiles_its_bookmarks() {
    let daemon = Daemon::start().await;
    let folder = folder(&daemon, r#"{"name":"Reading"}"#).await;
    let (code, body) = request(
        daemon.socket(),
        "POST",
        lockbox_proto::api::reader::BOOKMARKS_PATH,
        Some(
            &serde_json::json!({
                "url": "https://a.example",
                "title": "A",
                "folder_id": folder.id,
                "visibility": "public",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(code, 201, "{body}");
    let bookmark: lockbox_types::bookmark::Bookmark = json(&body);

    let (code, body) = request(daemon.socket(), "DELETE", &folder_path(folder.id), None).await;
    assert_eq!(code, 204, "{body}");

    let (code, body) = request(
        daemon.socket(),
        "GET",
        &lockbox_proto::api::reader::bookmark_path(bookmark.id),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let read: lockbox_types::bookmark::Bookmark = json(&body);
    assert_eq!(read.folder_id, None);
}

#[tokio::test]
async fn reports_a_folder_which_does_not_exist() {
    let daemon = Daemon::start().await;
    for (method, body) in [("GET", None), ("PATCH", Some("{}")), ("DELETE", None)] {
        let (code, body) = request(daemon.socket(), method, &folder_path(7), body).await;
        assert_eq!(code, 404, "{method}: {body}");
        let error: ApiError = json(&body);
        assert_eq!(error.code, Code::NotFound);
    }
}
