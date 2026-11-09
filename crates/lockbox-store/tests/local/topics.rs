//! Tests for [`lockbox_store::local::topics`].

use crate::common::Temp;
use crate::fixtures::{bookmark, open};
use lockbox_store::{
    Error,
    local::{bookmarks, tags, topics},
};

#[tokio::test]
async fn renames_and_deletes_topics() {
    let temp = Temp::new("topics");
    let db = open(&temp);
    let saved = bookmark(&db, "https://example.com/a", None, 10).await;
    let other = bookmark(&db, "https://example.com/b", None, 11).await;
    tags::set(&db, saved, vec!["Rust".to_owned()], 20)
        .await
        .expect("tag");
    tags::set(&db, other, vec!["Other".to_owned()], 21)
        .await
        .expect("tag");

    let listed = topics::list(&db).await.expect("list topics");
    let rust = listed
        .iter()
        .find(|topic| topic.name == "Rust")
        .expect("the rust topic")
        .id;

    let renamed = topics::rename(&db, rust, "Systems")
        .await
        .expect("rename topic");
    assert_eq!(renamed.name, "Systems");
    let names: Vec<String> = tags::for_bookmark(&db, saved)
        .await
        .expect("topics of the bookmark")
        .into_iter()
        .map(|topic| topic.name)
        .collect();
    assert_eq!(names, ["Systems"]);

    // A name which is taken under another case is refused
    let error = topics::rename(&db, rust, "other")
        .await
        .expect_err("duplicate topic");
    assert!(error.to_string().contains("topic.name"), "{error}");

    topics::delete(&db, rust).await.expect("delete topic");
    assert!(
        tags::for_bookmark(&db, saved)
            .await
            .expect("topics of the bookmark")
            .is_empty()
    );
    assert!(
        bookmarks::get(&db, saved)
            .await
            .expect("read bookmark")
            .is_some(),
        "the bookmark stays"
    );

    let error = topics::delete(&db, rust).await.expect_err("gone");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn follows_and_unfollows_topics() {
    let temp = Temp::new("follow");
    let db = open(&temp);

    let followed = topics::follow(&db, "Rust", 10).await.expect("follow topic");
    assert_eq!(followed.name, "Rust");
    assert_eq!(followed.followed_at, Some(10));
    let followed_topics = topics::followed(&db).await.expect("followed topics");
    assert_eq!(followed_topics.len(), 1);
    assert_eq!(followed_topics[0].id, followed.id);

    // Following again keeps the time it started
    let again = topics::follow(&db, "rust", 20).await.expect("follow again");
    assert_eq!(again.id, followed.id);
    assert_eq!(again.name, "Rust");
    assert_eq!(again.followed_at, Some(10));

    // A tag on the same topic reuses the row which is already there
    let saved = bookmark(&db, "https://example.com/a", None, 30).await;
    tags::set(&db, saved, vec!["Rust".to_owned()], 40)
        .await
        .expect("tag");
    assert_eq!(topics::list(&db).await.expect("list topics").len(), 1);

    topics::unfollow(&db, "rust").await.expect("unfollow");
    assert!(
        topics::followed(&db)
            .await
            .expect("followed topics")
            .is_empty()
    );
    let listed = topics::list(&db).await.expect("list topics");
    assert_eq!(listed[0].followed_at, None);
    assert_eq!(listed[0].name, "Rust", "the label stays");

    // Unfollowing something which is not followed does nothing
    topics::unfollow(&db, "Nothing")
        .await
        .expect("unfollow a topic which is not there");
    db.close().await.expect("close database");
}
