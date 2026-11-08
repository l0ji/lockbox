//! Tests for [`lockbox_store::local::folders`].

use crate::common::Temp;
use crate::fixtures::{bookmark, listed, open};
use lockbox_store::{
    Error,
    local::{
        bookmarks::{self, NewBookmark, Scope},
        folders::{self, Kind, NewFolder},
        tags,
    },
};

#[tokio::test]
async fn keeps_folders_in_a_tree() {
    let temp = Temp::new("folders");
    let db = open(&temp);
    let root = folders::create(&db, NewFolder::new(None, "Reading"), 10)
        .await
        .expect("create folder");
    let child = folders::create(&db, NewFolder::new(Some(root.id), "Rust"), 20)
        .await
        .expect("create child folder");
    let other = folders::create(&db, NewFolder::new(None, "Archive"), 30)
        .await
        .expect("create folder");
    assert_eq!(child.parent_id, Some(root.id));

    let names: Vec<String> = folders::list(&db)
        .await
        .expect("list folders")
        .into_iter()
        .map(|folder| folder.name)
        .collect();
    assert_eq!(names, ["Reading", "Archive", "Rust"]);

    // A name is unique under its parent, and nowhere else
    assert!(
        folders::create(&db, NewFolder::new(None, "Archive"), 40)
            .await
            .is_err()
    );
    assert!(
        folders::create(&db, NewFolder::new(Some(root.id), "Rust"), 40)
            .await
            .is_err()
    );
    folders::create(&db, NewFolder::new(Some(other.id), "Rust"), 40)
        .await
        .expect("the same name under another parent");

    // Folders move and are renumbered like bookmarks
    folders::place(&db, other.id, None, 0)
        .await
        .expect("move to the front");
    let roots: Vec<String> = folders::list(&db)
        .await
        .expect("list folders")
        .into_iter()
        .filter(|folder| folder.parent_id.is_none())
        .map(|folder| folder.name)
        .collect();
    assert_eq!(roots, ["Archive", "Reading"]);

    // A folder cannot be moved inside itself
    let error = folders::place(&db, root.id, Some(child.id), 0)
        .await
        .expect_err("cycle");
    assert!(matches!(error, Error::Invalid(_)), "{error}");
    let error = folders::place(&db, root.id, Some(root.id), 0)
        .await
        .expect_err("itself");
    assert!(matches!(error, Error::Invalid(_)), "{error}");

    let renamed = folders::rename(&db, child.id, "Async")
        .await
        .expect("rename folder");
    assert_eq!(renamed.name, "Async");

    // Deleting a folder takes its children and unfiles its bookmarks
    let nested = folders::create(&db, NewFolder::new(Some(other.id), "Nested"), 50)
        .await
        .expect("create nested folder");
    let saved = bookmark(&db, "https://example.com/x", Some(nested.id), 60).await;
    folders::delete(&db, other.id).await.expect("delete folder");
    let saved = bookmarks::get(&db, saved)
        .await
        .expect("read bookmark")
        .expect("bookmark");
    assert_eq!(saved.folder_id, None);
    assert!(
        !folders::list(&db)
            .await
            .expect("list folders")
            .iter()
            .any(|folder| folder.id == nested.id)
    );

    let error = folders::delete(&db, 9_999).await.expect_err("gone");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn matches_topics_in_a_dynamic_folder() {
    let temp = Temp::new("dynamic-folder");
    let db = open(&temp);
    let dynamic = folders::create(
        &db,
        NewFolder {
            kind: Kind::Dynamic,
            topics: vec!["Rust".to_owned()],
            ..NewFolder::new(None, "Rust")
        },
        10,
    )
    .await
    .expect("create dynamic folder");
    assert_eq!(dynamic.kind, Kind::Dynamic);
    assert_eq!(
        dynamic
            .topics
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["Rust"]
    );

    let a = bookmark(&db, "https://a.example", None, 20).await;
    let b = bookmark(&db, "https://b.example", None, 21).await;
    tags::set(&db, a, vec!["Rust".to_owned()], 30)
        .await
        .expect("tag");
    tags::set(&db, b, vec!["Rust".to_owned(), "Async".to_owned()], 31)
        .await
        .expect("tag");

    let scope = bookmarks::Filter {
        folder: Scope::Folder(dynamic.id),
        ..bookmarks::Filter::default()
    };
    assert_eq!(
        listed(&db, scope.clone()).await,
        ["https://b.example", "https://a.example"],
        "the newest match comes first"
    );

    // The folder follows the topics it carries
    folders::set_topics(&db, dynamic.id, vec!["Async".to_owned()], 40)
        .await
        .expect("change topics");
    assert_eq!(listed(&db, scope.clone()).await, ["https://b.example"]);

    // And the topics the bookmarks carry
    tags::set(&db, b, vec![], 50).await.expect("untag");
    assert!(listed(&db, scope).await.is_empty());

    let folders = folders::list(&db).await.expect("list folders");
    assert_eq!(
        folders
            .iter()
            .flat_map(|folder| &folder.topics)
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["Async"]
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn refuses_to_file_into_a_dynamic_folder() {
    let temp = Temp::new("dynamic-file");
    let db = open(&temp);
    let dynamic = folders::create(
        &db,
        NewFolder {
            kind: Kind::Dynamic,
            ..NewFolder::new(None, "Rust")
        },
        10,
    )
    .await
    .expect("create dynamic folder");

    let error = bookmarks::create(
        &db,
        NewBookmark {
            folder_id: Some(dynamic.id),
            ..NewBookmark::new("https://a.example", "A")
        },
        20,
    )
    .await
    .expect_err("a dynamic folder takes no files");
    assert!(matches!(error, Error::Invalid(_)), "{error}");

    let saved = bookmark(&db, "https://b.example", None, 21).await;
    let error = bookmarks::place(&db, saved, Some(dynamic.id), 0, 22)
        .await
        .expect_err("a dynamic folder takes no files");
    assert!(matches!(error, Error::Invalid(_)), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn manual_folders_have_no_topics() {
    let temp = Temp::new("manual-folder");
    let db = open(&temp);
    let manual = folders::create(&db, NewFolder::new(None, "Reading"), 10)
        .await
        .expect("create folder");
    assert_eq!(manual.kind, Kind::Manual);
    assert!(manual.topics.is_empty());

    let error = folders::create(
        &db,
        NewFolder {
            topics: vec!["Rust".to_owned()],
            ..NewFolder::new(None, "Tags")
        },
        11,
    )
    .await
    .expect_err("a manual folder has no topics");
    assert!(matches!(error, Error::Invalid(_)), "{error}");

    let error = folders::set_topics(&db, manual.id, vec!["Rust".to_owned()], 12)
        .await
        .expect_err("a manual folder has no topics");
    assert!(matches!(error, Error::Invalid(_)), "{error}");
    db.close().await.expect("close database");
}
