//! Tests for [`lockbox_store::local::tags`].

use crate::common::Temp;
use crate::fixtures::{bookmark, listed, open};
use lockbox_store::{
    Error,
    local::{bookmarks, tags, topics},
};
use lockbox_types::bookmark::Filter;

#[tokio::test]
async fn tags_a_bookmark() {
    let temp = Temp::new("tags");
    let db = open(&temp);
    let saved = bookmark(&db, "https://example.com/rust", None, 10).await;

    let tagged = tags::set(
        &db,
        saved,
        vec![
            "Rust".to_owned(),
            "rust".to_owned(),
            " Databases ".to_owned(),
            "  ".to_owned(),
        ],
        20,
    )
    .await
    .expect("tag the bookmark");
    let names: Vec<&str> = tagged.iter().map(|topic| topic.name.as_str()).collect();
    assert_eq!(
        names,
        ["Databases", "Rust"],
        "trimmed, deduplicated, by name"
    );

    let tagged = tags::set(&db, saved, vec!["rust".to_owned(), "Async".to_owned()], 30)
        .await
        .expect("retag the bookmark");
    let names: Vec<&str> = tagged.iter().map(|topic| topic.name.as_str()).collect();
    assert_eq!(names, ["Async", "Rust"], "the first spelling is kept");

    // A topic which lost its last bookmark stays in the namespace
    let names: Vec<String> = topics::list(&db)
        .await
        .expect("list topics")
        .into_iter()
        .map(|topic| topic.name)
        .collect();
    assert_eq!(names, ["Async", "Databases", "Rust"]);

    bookmark(&db, "https://example.com/stew", None, 40).await;
    let other = bookmarks::list(&db, &Filter::default(), None, usize::MAX)
        .await
        .expect("list bookmarks")
        .items
        .into_iter()
        .find(|bookmark| bookmark.url == "https://example.com/stew")
        .expect("the stew bookmark")
        .id;
    tags::set(&db, other, vec!["Cooking".to_owned()], 50)
        .await
        .expect("tag the other bookmark");

    assert_eq!(
        listed(
            &db,
            Filter {
                topic: Some("RUST".to_owned()),
                ..Filter::default()
            }
        )
        .await,
        ["https://example.com/rust"]
    );
    assert_eq!(
        listed(
            &db,
            Filter {
                query: Some("stew".to_owned()),
                ..Filter::default()
            }
        )
        .await,
        ["https://example.com/stew"]
    );

    let error = tags::set(&db, 9_999, vec![], 60)
        .await
        .expect_err("no such bookmark");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}
