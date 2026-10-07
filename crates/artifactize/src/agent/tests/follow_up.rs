//! Follow-up preparation and recorded-send outcomes use fake clients and temporary sessions only.
use super::*;

fn conversation(fixture: &Fixture) -> (Conversation, Recorder) {
    let state = fixture.state();
    let path = session::path(&state, SESSION).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, format!("{}\n{}\n", json!({"kind":"review","backend":"openai","model":"exact-model","budgets":{"timeoutMs":1000}}), json!({"kind":"message","turn":1,"message":{"role":"user","content":[{"type":"text","text":"Original review"}]}}))).unwrap();
    let conversation = Conversation::load(&path).unwrap().unwrap();
    let recorder = Recorder::append(&path, &SESSION.parse().unwrap(), 1).unwrap();
    (conversation, recorder)
}
async fn run(
    fixture: &Fixture,
    factory: impl FnOnce(crate::config::Backend, &str, &Path, &Path) -> Result<Client, Failure>,
    cancellation: CancellationToken,
) -> (FollowUp, Conversation) {
    let (conversation, mut recorder) = conversation(fixture);
    let state = fixture.state();
    let result = continue_conversation(
        &fixture.config,
        &fixture.config.evals[0],
        &Continuation {
            conversation: &conversation,
            files_changed: None,
            output: &fixture.output,
            state: &state,
        },
        "Follow-up?",
        &mut recorder,
        &cancellation,
        factory,
    )
    .await;
    let outcome = complete_follow_up(&mut recorder, result);
    (
        outcome,
        Conversation::load(&conversation.path).unwrap().unwrap(),
    )
}
fn fake(http: SequencedHttpClient) -> Client {
    Client::Openai(Box::new(
        OpenAIConfig::new("fake-only-key")
            .connect(http)
            .responses("exact-model"),
    ))
}

#[tokio::test]
async fn preparation_failure_records_no_send_answer_or_attempt() {
    let fixture = Fixture::new("openai");
    let (outcome, saved) = run(
        &fixture,
        |_, _, _, _| Err(Failure::new(Code::Authentication, "fake pre-send failure")),
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        outcome,
        FollowUp::NotStarted(Failure {
            code: Code::Authentication,
            ..
        })
    ));
    assert_eq!(saved.events.len(), 2);
    assert_eq!(saved.sends(), 0);
}

#[tokio::test]
async fn recorded_send_success_retains_usage_and_event_order() {
    let fixture = Fixture::new("openai");
    let http = SequencedHttpClient::new(vec![openai_response(
        "exact-model",
        "completed",
        vec![message("Free-text answer")],
        json!({"input_tokens":10,"output_tokens":2,"total_tokens":12}),
    )]);
    let (outcome, saved) = run(
        &fixture,
        |_, _, _, _| Ok(fake(http.clone())),
        CancellationToken::new(),
    )
    .await;
    let FollowUp::Started { answer, attempts } = outcome else {
        panic!("started");
    };
    assert_eq!(answer.unwrap(), "Free-text answer");
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].usage["inputTokens"], 10);
    let kinds: Vec<_> = saved.wire_events[2..]
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["send", "message", "attempt", "message", "answer"]);
    assert_eq!(
        saved.wire_events.last().unwrap()["text"],
        "Free-text answer"
    );
}

#[tokio::test]
async fn recorded_send_provider_error_and_cancellation_always_record_answer() {
    for cancel in [false, true] {
        let fixture = Fixture::new("openai");
        let http = SequencedHttpClient::new(vec![MockHttpResponse::error(
            StatusCode::UNAUTHORIZED,
            "{}",
        )]);
        let cancellation = CancellationToken::new();
        if cancel {
            cancellation.cancel();
        }
        let (outcome, saved) =
            run(&fixture, |_, _, _, _| Ok(fake(http.clone())), cancellation).await;
        let FollowUp::Started {
            answer: Err(error),
            attempts,
        } = outcome
        else {
            panic!("started error");
        };
        assert_eq!(
            error.code,
            if cancel {
                Code::Cancelled
            } else {
                Code::Authentication
            }
        );
        assert_eq!(attempts.len(), usize::from(!cancel));
        let kinds: Vec<_> = saved.wire_events[2..]
            .iter()
            .map(|event| event["kind"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            if cancel {
                vec!["send", "message", "answer"]
            } else {
                vec!["send", "message", "attempt", "answer"]
            }
        );
        assert_eq!(
            saved.wire_events.last().unwrap()["errorCode"],
            error.code.as_str()
        );
        assert_eq!(http.requests().len(), usize::from(!cancel));
    }
}
