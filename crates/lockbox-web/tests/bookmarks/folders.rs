//! Tests of the folder page and its forms.

use axum::{
    Json, Router,
    http::{Method, StatusCode, header},
    routing::{get, patch},
};
use lockbox_proto::api::{ApiError, Code, reader::FOLDERS_PATH};
use lockbox_types::folder::{Folder, FolderChange};

use crate::common::{Record, Stub, request};
use crate::{folders, stub};

#[tokio::test]
async fn serves_the_folder_tree() {
    let stub = Stub::serve(stub(Record::default(), StatusCode::CREATED)).await;

    let (code, _, html) = request(Method::GET, "/folders", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("<title>lockbox: folders</title>"), "{html}");
    assert!(html.contains("Reading"), "{html}");
    assert!(html.contains("Rust"), "{html}");
    assert!(
        html.contains("└─ Rust") || html.contains("├─ Rust"),
        "{html}"
    );
    assert!(
        html.contains(r#"<option value="2">└─ Rust</option>"#) || html.contains("Rust</option>"),
        "{html}"
    );
    // Only dynamic folders use the topics field
    assert!(html.contains(r#"id="new-folder-kind""#), "{html}");
    assert!(html.contains(r#"id="new-folder-topics" hidden"#), "{html}");
    assert!(html.contains("syncTopics"), "{html}");
}

#[tokio::test]
async fn creates_a_manual_folder() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/folders",
        Some("name=Reading&parent_id=&kind=manual&topics=ignored"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/folders");
    let body: lockbox_types::folder::NewFolder =
        serde_json::from_str(&record.bodies()[0]).expect("folder json");
    assert_eq!(body.name, "Reading");
    assert_eq!(body.parent_id, None);
    assert_eq!(body.kind, lockbox_types::folder::Kind::Manual);
    assert!(body.topics.is_empty(), "a manual folder takes no topics");
}

#[tokio::test]
async fn creates_a_dynamic_folder() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, _, _) = request(
        Method::POST,
        "/folders",
        Some("name=Rust&parent_id=1&kind=dynamic&topics=rust%2C+async"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    let body: lockbox_types::folder::NewFolder =
        serde_json::from_str(&record.bodies()[0]).expect("folder json");
    assert_eq!(body.kind, lockbox_types::folder::Kind::Dynamic);
    assert_eq!(body.topics, ["rust", "async"]);
}

#[tokio::test]
async fn changes_and_deletes_a_folder() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/folders/1/change",
        Some("name=Better&parent_id=2&position=3&kind=dynamic&topics=rust"),
        stub.socket(),
    )
    .await;
    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/folders");
    let body: FolderChange = serde_json::from_str(&record.bodies()[0]).expect("folder json");
    assert_eq!(body.name.as_deref(), Some("Better"));
    assert_eq!(body.parent_id, Some(Some(2)));
    assert_eq!(body.position, Some(3));
    assert_eq!(body.topics, Some(vec!["rust".to_owned()]));

    let (code, headers, _) =
        request(Method::POST, "/folders/1/delete", Some(""), stub.socket()).await;
    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/folders");
}

#[tokio::test]
async fn reports_a_folder_which_cannot_be_changed() {
    let app = Router::new()
        .route(
            FOLDERS_PATH,
            get(folders).post(|| async {
                (
                    StatusCode::CONFLICT,
                    Json(ApiError {
                        code: Code::Conflict,
                        error: "UNIQUE constraint failed: folder.name".to_owned(),
                    }),
                )
            }),
        )
        .route(
            &format!("{}/{{id}}", lockbox_proto::api::reader::FOLDERS_PATH),
            patch(|| async {
                (
                    StatusCode::CONFLICT,
                    Json(ApiError {
                        code: Code::Conflict,
                        error: "UNIQUE constraint failed: folder.name".to_owned(),
                    }),
                )
            }),
        );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(
        Method::POST,
        "/folders/1/change",
        Some("name=Reading&parent_id=&position=0&kind=manual"),
        stub.socket(),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "the page is shown again");
    assert!(html.contains("UNIQUE constraint failed"), "{html}");
}

#[tokio::test]
async fn serves_an_empty_folder_page() {
    let app = Router::new().route(FOLDERS_PATH, get(|| async { Json(Vec::<Folder>::new()) }));
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(Method::GET, "/folders", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("no folders yet"), "{html}");
}
