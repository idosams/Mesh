use super::*;

#[test]
fn signed_execution_status_routes_without_allocating_or_starting_work() {
    let s = Setup::new();
    let installation = installation(&s);
    let mut hub =
        NativeWorkerConnections::new(&installation, &s.destination, policy(&s), 1).unwrap();
    let mut runtime = s.f.runtime(true);
    let challenge = RemoteWorkerStatusChallenge::issue_with_execution_observation(
        &mut runtime,
        "lane",
        "run",
        public(&s.f.coordinator),
        public(&s.f.worker),
    )
    .unwrap();
    let query = challenge
        .signed_query(&mut runtime, |p| sign(&s.f.coordinator, p))
        .unwrap();
    let mut input = Vec::new();
    RemoteFrameWriter::new(&mut input)
        .write_frame(&query.frame().unwrap())
        .unwrap();
    let mut output = Vec::new();
    assert!(matches!(
        hub.serve(input.as_slice(), &mut output, |p| sign(&s.f.worker, p))
            .unwrap(),
        WorkerConnectionOutcome::StatusReplied
    ));
    let frame = RemoteFrameReader::new(output.as_slice())
        .read_frame()
        .unwrap()
        .unwrap();
    let receipt = challenge
        .verify_reply(&mut runtime, &control(frame))
        .unwrap();
    assert!(receipt.reports_execution());
    assert_eq!(receipt.recorded_execution(), None);
    assert_eq!(receipt.input_inspection(), None);
    assert!(hub.transfers.is_empty());
    assert!(hub.recoveries.is_empty());
    assert_eq!(
        fs::read_dir(s.f.path.join("allocations")).unwrap().count(),
        0
    );
}
