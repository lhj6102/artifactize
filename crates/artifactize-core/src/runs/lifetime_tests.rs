use super::*;
use serde_json::json;

#[tokio::test]
async fn reservation_retry_cancel_and_failed_launch_release_their_writer_before_return() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir(&repo).unwrap();
    crate::test_declaration::write(
        repo.join("index.artf"),
        json!({"name":"app", "fingerprint":false, "evals":[{
            "id":"check", "title":"Check", "profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]},
            "payload":{"instruction":"Check."}
        }]}).to_string(),
    ).unwrap();
    let options = VerifyOptions::default();
    let key = "retry".parse().unwrap();
    let (id, accepted) =
        crate::project::verify::reserve(&repo, Some(&state), &Selection::All, &options, Some(&key))
            .await
            .unwrap();
    assert!(accepted);
    let database = state.join(store::DATABASE);
    assert!(crate::test_os::database_released(&database).unwrap());
    let context = Context::new(&repo, Some(&state), &repo.join("missing-worker")).unwrap();
    let retried = start(&context, &Selection::All, &options, Some(key))
        .await
        .unwrap();
    assert_eq!(retried.run, id);
    assert!(crate::test_os::database_released(&database).unwrap());
    assert!(cancel(&context, &id).await.unwrap().cancellation_requested);
    assert!(crate::test_os::database_released(&database).unwrap());
    let error = start(&context, &Selection::All, &options, None)
        .await
        .unwrap_err();
    assert!(error.contains("handoff failed"), "{error}");
    assert!(crate::test_os::database_released(&database).unwrap());
    root.close().unwrap();
}

#[test]
fn supervisor_identity_is_optional_in_saved_control_and_preserves_executor_ownership() {
    let old = json!({"worker":{"state":"admitted","owner":{"pid":1,"start_time":2}},"activity":"executing","cancellationRequested":false});
    let control: Control = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(control.supervisor, None);
    assert_eq!(serde_json::to_value(&control).unwrap(), old);
    let mut supervised = old;
    supervised["supervisor"] = json!({"pid":3,"start_time":4});
    let control: Control = serde_json::from_value(supervised.clone()).unwrap();
    let Worker::Admitted { owner } = control.worker else {
        panic!("executor stays admitted");
    };
    assert_eq!(owner.pid.get(), 1);
    assert_eq!(control.supervisor.unwrap().pid.get(), 3);
    assert_eq!(serde_json::to_value(control).unwrap(), supervised);
}
