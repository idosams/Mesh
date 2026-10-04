use super::*;
fn d(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
fn j(n: u8) -> Json {
    Json::text(d(n).to_hex())
}
fn w(n: u8) -> Json {
    Json::Array(vec![j(n), j(n + 1)])
}
fn i(n: u8) -> Json {
    Json::Array(vec![w(n), j(n + 2)])
}
fn binding() -> NativeDependencyBinding {
    NativeDependencyBinding {
        authority: d(1),
        project: d(2),
        installation: d(3),
    }
}
fn payload(
    kind: DependencyKind,
    revision: u64,
    previous: RecordDigest,
    body: Json,
) -> (DependencyRecord, Vec<u8>) {
    let text = Json::object([
        ("schema", Json::text(SCHEMA)),
        ("authority", j(1)),
        ("revision", Json::Number(revision)),
        ("previous", Json::text(previous.to_hex())),
        ("kind", Json::Number(u64::from(kind.code()))),
        ("body", body),
    ])
    .encode();
    let envelope = DependencyRecord {
        authority: d(1),
        revision,
        previous,
        payload: RecordDigest::from_bytes(*Blake3::digest_bytes(text.as_bytes()).as_bytes()),
        kind,
    };
    (envelope, text.into_bytes())
}
fn enrollment() -> (DependencyRecord, Vec<u8>) {
    payload(
        DependencyKind::Enrollment,
        1,
        ZERO,
        Json::object([("project", j(2)), ("installation", j(3))]),
    )
}
fn grant(
    revision: u64,
    previous: RecordDigest,
    request: u8,
    generation: u64,
    prior: RecordDigest,
    allowed: bool,
) -> (DependencyRecord, Vec<u8>) {
    payload(
        DependencyKind::Grant,
        revision,
        previous,
        Json::object([
            ("request", j(request)),
            ("source", i(10)),
            ("destination", w(20)),
            ("generation", Json::Number(generation)),
            ("previous", Json::text(prior.to_hex())),
            ("allowed", Json::Bool(allowed)),
        ]),
    )
}
fn consume(
    revision: u64,
    previous: RecordDigest,
    grant: RecordDigest,
    request: u8,
) -> (DependencyRecord, Vec<u8>) {
    payload(
        DependencyKind::Consumption,
        revision,
        previous,
        Json::object([
            ("request", j(request)),
            ("grant", Json::text(grant.to_hex())),
            ("start", i(20)),
            ("inputs", Json::Array(vec![i(10)])),
        ]),
    )
}
fn decide(
    revision: u64,
    previous: RecordDigest,
    request: u8,
    decision_revision: u64,
    prior: RecordDigest,
    state: &str,
) -> (DependencyRecord, Vec<u8>) {
    payload(
        DependencyKind::Eligibility,
        revision,
        previous,
        Json::object([
            ("request", j(request)),
            ("input", i(10)),
            ("revision", Json::Number(decision_revision)),
            ("previous", Json::text(prior.to_hex())),
            ("state", Json::text(state)),
            ("replacement", Json::Null),
        ]),
    )
}
fn review(
    revision: u64,
    previous: RecordDigest,
    request: u8,
    decision: RecordDigest,
) -> (DependencyRecord, Vec<u8>) {
    payload(
        DependencyKind::ReviewSnapshot,
        revision,
        previous,
        Json::object([
            ("request", j(request)),
            ("output", i(20)),
            (
                "decisions",
                Json::Array(vec![Json::Array(vec![
                    i(10),
                    Json::text(decision.to_hex()),
                ])]),
            ),
        ]),
    )
}
fn apply(h: &mut DependencyPolicyHistory, record: &(DependencyRecord, Vec<u8>)) {
    h.apply(record.0, &record.1).unwrap();
}
fn refuse(h: &mut DependencyPolicyHistory, record: &(DependencyRecord, Vec<u8>)) {
    let before = h.clone();
    assert_eq!(h.apply(record.0, &record.1), Err(InvalidDependencyHistory));
    assert_eq!(*h, before);
}

#[test]
fn enrollment_has_independent_canonical_bytes_and_external_identity_binding() {
    let (record, bytes) = enrollment();
    let expected=format!("{{\"schema\":\"mesh.dependency-policy/v1\",\"authority\":\"{}\",\"revision\":1,\"previous\":\"{}\",\"kind\":0,\"body\":{{\"project\":\"{}\",\"installation\":\"{}\"}}}}", "01".repeat(32),"00".repeat(32),"02".repeat(32),"03".repeat(32));
    assert_eq!(bytes, expected.as_bytes());
    let mut history = DependencyPolicyHistory::new(binding()).unwrap();
    for n in 0..bytes.len() {
        refuse(&mut history, &(record, bytes[..n].to_vec()));
    }
    for wrong in [
        NativeDependencyBinding {
            authority: d(9),
            ..binding()
        },
        NativeDependencyBinding {
            project: d(9),
            ..binding()
        },
        NativeDependencyBinding {
            installation: d(9),
            ..binding()
        },
    ] {
        refuse(
            &mut DependencyPolicyHistory::new(wrong).unwrap(),
            &(record, bytes.clone()),
        );
    }
    let mut wrong = record;
    wrong.revision = 2;
    refuse(&mut history, &(wrong, bytes.clone()));
    wrong = record;
    wrong.kind = DependencyKind::Grant;
    refuse(&mut history, &(wrong, bytes.clone()));
    wrong = record;
    wrong.previous = d(9);
    refuse(&mut history, &(wrong, bytes.clone()));
    wrong = record;
    wrong.payload = d(9);
    refuse(&mut history, &(wrong, bytes.clone()));
    apply(&mut history, &(record, bytes));
    assert_eq!(history.len(), 1);
}

#[test]
fn replay_preserves_history_and_separates_ledger_grant_and_decision_revisions() {
    let mut h = DependencyPolicyHistory::new(binding()).unwrap();
    let e = enrollment();
    apply(&mut h, &e);
    let g = grant(2, e.0.payload, 50, 1, ZERO, true);
    apply(&mut h, &g);
    let c = consume(3, g.0.payload, g.0.payload, 51);
    apply(&mut h, &c);
    let decision = decide(4, c.0.payload, 52, 1, ZERO, "eligible");
    apply(&mut h, &decision);
    let r = review(5, decision.0.payload, 53, decision.0.payload);
    apply(&mut h, &r);
    let rejection = decide(6, r.0.payload, 54, 2, decision.0.payload, "rejected");
    apply(&mut h, &rejection);
    refuse(
        &mut h,
        &review(7, rejection.0.payload, 55, decision.0.payload),
    );
    refuse(
        &mut h,
        &review(7, rejection.0.payload, 55, rejection.0.payload),
    );
    let revalidated = decide(
        7,
        rejection.0.payload,
        56,
        3,
        rejection.0.payload,
        "eligible",
    );
    apply(&mut h, &revalidated);
    let revoked = grant(8, revalidated.0.payload, 57, 2, g.0.payload, false);
    apply(&mut h, &revoked);
    refuse(&mut h, &consume(9, revoked.0.payload, g.0.payload, 58));
    let before = h.clone();
    for event in [
        &e,
        &g,
        &c,
        &decision,
        &r,
        &rejection,
        &revalidated,
        &revoked,
    ] {
        apply(&mut h, event);
    }
    assert_eq!(h, before);
    let roots: BTreeSet<_> = h.referenced_content().copied().collect();
    assert_eq!(
        roots,
        [
            e.0.payload,
            g.0.payload,
            c.0.payload,
            decision.0.payload,
            r.0.payload,
            rejection.0.payload,
            revalidated.0.payload,
            revoked.0.payload,
            d(12),
            d(22)
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn conflicting_requests_gaps_wrong_grant_predecessors_and_incomplete_decisions_are_atomic() {
    let mut h = DependencyPolicyHistory::new(binding()).unwrap();
    let e = enrollment();
    apply(&mut h, &e);
    let g = grant(2, e.0.payload, 50, 1, ZERO, true);
    apply(&mut h, &g);
    for record in [
        grant(3, g.0.payload, 50, 2, g.0.payload, false),
        grant(4, g.0.payload, 51, 2, g.0.payload, false),
        grant(3, g.0.payload, 51, 2, ZERO, false),
        grant(3, g.0.payload, 51, 3, g.0.payload, false),
        decide(3, g.0.payload, 51, 2, ZERO, "eligible"),
        review(3, g.0.payload, 51, d(90)),
        consume(3, g.0.payload, d(90), 51),
    ] {
        refuse(&mut h, &record);
    }
    let mut wrong = enrollment();
    wrong.0.revision = 3;
    wrong.0.previous = g.0.payload;
    refuse(&mut h, &wrong);
}

#[test]
fn malformed_canonical_payloads_are_rejected_even_with_matching_content_hashes() {
    let (envelope, bytes) = enrollment();
    let text = String::from_utf8(bytes).unwrap();
    let mut h = DependencyPolicyHistory::new(binding()).unwrap();
    let cases = [
        format!(" {text}"),
        text.replace("\"revision\":1", "\"revision\":0"),
        text.replace("\"revision\":1", "\"revision\":9223372036854775808"),
        text.replace("\"kind\":0", "\"kind\":99"),
        text.replace("\"body\":{", "\"body\":{\"extra\":1,"),
        text.replace("mesh.dependency-policy/v1", "mesh.dependency-policy/v2"),
        text.replace("\"project\":", "\"project\":\"invalid\",\"project\":"),
        " ".repeat(MAX_BYTES + 1),
        "[".repeat(1000),
    ];
    for text in cases {
        let mut e = envelope;
        e.payload = RecordDigest::from_bytes(*Blake3::digest_bytes(text.as_bytes()).as_bytes());
        refuse(&mut h, &(e, text.into_bytes()));
    }
}

#[test]
fn closure_rows_are_bounded_unique_and_canonically_ordered() {
    let mut h = DependencyPolicyHistory::new(binding()).unwrap();
    let e = enrollment();
    apply(&mut h, &e);
    let g = grant(2, e.0.payload, 50, 1, ZERO, true);
    apply(&mut h, &g);
    for values in [
        vec![],
        vec![i(10), i(10)],
        vec![i(30), i(10)],
        vec![i(10); MAX_INPUTS + 1],
        vec![i(30)],
    ] {
        let c = payload(
            DependencyKind::Consumption,
            3,
            g.0.payload,
            Json::object([
                ("request", j(51)),
                ("grant", Json::text(g.0.payload.to_hex())),
                ("start", i(20)),
                ("inputs", Json::Array(values)),
            ]),
        );
        refuse(&mut h, &c);
    }
    let c = consume(3, g.0.payload, g.0.payload, 51);
    apply(&mut h, &c);
}

#[test]
fn replacement_and_unrelated_decisions_preserve_exact_review_dependencies() {
    let mut h = DependencyPolicyHistory::new(binding()).unwrap();
    let e = enrollment();
    apply(&mut h, &e);
    let first = decide(2, e.0.payload, 50, 1, ZERO, "eligible");
    apply(&mut h, &first);
    let unrelated = payload(
        DependencyKind::Eligibility,
        3,
        first.0.payload,
        Json::object([
            ("request", j(51)),
            ("input", i(30)),
            ("revision", Json::Number(1)),
            ("previous", j(0)),
            ("state", Json::text("rejected")),
            ("replacement", Json::Null),
        ]),
    );
    apply(&mut h, &unrelated);
    let r = review(4, unrelated.0.payload, 52, first.0.payload);
    apply(&mut h, &r);
    for (state, replacement) in [
        ("replaced", Json::Null),
        ("eligible", i(30)),
        ("replaced", i(10)),
        ("unknown", Json::Null),
    ] {
        let bad = payload(
            DependencyKind::Eligibility,
            5,
            r.0.payload,
            Json::object([
                ("request", j(53)),
                ("input", i(10)),
                ("revision", Json::Number(2)),
                ("previous", Json::text(first.0.payload.to_hex())),
                ("state", Json::text(state)),
                ("replacement", replacement),
            ]),
        );
        refuse(&mut h, &bad);
    }
    let replaced = payload(
        DependencyKind::Eligibility,
        5,
        r.0.payload,
        Json::object([
            ("request", j(53)),
            ("input", i(10)),
            ("revision", Json::Number(2)),
            ("previous", Json::text(first.0.payload.to_hex())),
            ("state", Json::text("replaced")),
            ("replacement", i(30)),
        ]),
    );
    apply(&mut h, &replaced);
    refuse(
        &mut h,
        &review(6, replaced.0.payload, 54, replaced.0.payload),
    );
    assert!(h.referenced_content().any(|p| *p == d(32)));
    let old = h.clone();
    apply(&mut h, &r);
    assert_eq!(old, h);
}

#[test]
fn stable_work_identity_cannot_be_disguised_with_another_installation() {
    let mut h = DependencyPolicyHistory::new(binding()).unwrap();
    let e = enrollment();
    apply(&mut h, &e);
    let bad = payload(
        DependencyKind::Grant,
        2,
        e.0.payload,
        Json::object([
            ("request", j(50)),
            ("source", i(10)),
            ("destination", Json::Array(vec![j(10), j(99)])),
            ("generation", Json::Number(1)),
            ("previous", j(0)),
            ("allowed", Json::Bool(true)),
        ]),
    );
    refuse(&mut h, &bad);
}
fn unique(n: u64) -> Json {
    Json::text(
        RecordDigest::from_bytes(*Blake3::digest_bytes(&n.to_be_bytes()).as_bytes()).to_hex(),
    )
}

#[test]
fn record_budget_refuses_the_next_record_and_still_allows_exact_replay() {
    let mut h = DependencyPolicyHistory::new(binding()).unwrap();
    let e = enrollment();
    apply(&mut h, &e);
    let mut prior = ZERO;
    let mut head = e.0.payload;
    for n in 1..MAX_RECORDS as u64 {
        let event = payload(
            DependencyKind::Eligibility,
            n + 1,
            head,
            Json::object([
                ("request", unique(n)),
                ("input", i(10)),
                ("revision", Json::Number(n)),
                ("previous", Json::text(prior.to_hex())),
                ("state", Json::text("eligible")),
                ("replacement", Json::Null),
            ]),
        );
        apply(&mut h, &event);
        prior = event.0.payload;
        head = event.0.payload;
    }
    assert_eq!(h.len(), MAX_RECORDS);
    let next = payload(
        DependencyKind::Eligibility,
        MAX_RECORDS as u64 + 1,
        head,
        Json::object([
            ("request", unique(MAX_RECORDS as u64)),
            ("input", i(10)),
            ("revision", Json::Number(MAX_RECORDS as u64)),
            ("previous", Json::text(prior.to_hex())),
            ("state", Json::text("eligible")),
            ("replacement", Json::Null),
        ]),
    );
    refuse(&mut h, &next);
    let before = h.clone();
    apply(&mut h, &e);
    assert_eq!(h, before);
}

#[test]
fn aggregate_byte_budget_is_enforced_before_record_limit_without_losing_roots() {
    let mut h = DependencyPolicyHistory::new(binding()).unwrap();
    let e = enrollment();
    apply(&mut h, &e);
    let g = grant(2, e.0.payload, 50, 1, ZERO, true);
    apply(&mut h, &g);
    let mut many = vec![i(10)];
    for n in 0..255_u16 {
        let mut id = [100; 32];
        id[30..].copy_from_slice(&n.to_be_bytes());
        many.push(Json::Array(vec![
            Json::Array(vec![
                Json::text(RecordDigest::from_bytes(id).to_hex()),
                j(101),
            ]),
            j(102),
        ]));
    }
    let mut total = e.1.len() + g.1.len();
    let mut head = g.0.payload;
    let mut ordinal = 3;
    loop {
        let c = payload(
            DependencyKind::Consumption,
            ordinal,
            head,
            Json::object([
                ("request", unique(ordinal)),
                ("grant", Json::text(g.0.payload.to_hex())),
                ("start", i(20)),
                ("inputs", Json::Array(many.clone())),
            ]),
        );
        assert!(c.1.len() <= MAX_BYTES);
        if total + c.1.len() > MAX_HISTORY_BYTES {
            refuse(&mut h, &c);
            break;
        }
        total += c.1.len();
        apply(&mut h, &c);
        head = c.0.payload;
        ordinal += 1;
    }
    assert!(h.len() < MAX_RECORDS);
    assert_eq!(h.payload_bytes, total);
    let before = h.clone();
    apply(&mut h, &g);
    assert_eq!(h, before);
}

#[test]
fn bound_grant_schema_retains_correlation_and_refuses_missing_or_malformed_bindings() {
    fn bound(schema: &str, bindings: Option<Json>) -> (DependencyRecord, Vec<u8>) {
        let e = enrollment();
        let mut value =
            Json::parse(std::str::from_utf8(&grant(2, e.0.payload, 40, 1, ZERO, true).1).unwrap())
                .unwrap();
        let Json::Object(fields) = &mut value else {
            unreachable!()
        };
        fields
            .iter_mut()
            .find(|(name, _)| name == "schema")
            .unwrap()
            .1 = Json::text(schema);
        if let Some(bindings) = bindings {
            let Json::Object(body) = &mut fields
                .iter_mut()
                .find(|(name, _)| name == "body")
                .unwrap()
                .1
            else {
                unreachable!()
            };
            body.push(("bindings".into(), bindings));
        }
        let bytes = value.encode().into_bytes();
        let mut record = grant(2, e.0.payload, 40, 1, ZERO, true).0;
        record.payload = RecordDigest::from_bytes(*Blake3::digest_bytes(&bytes).as_bytes());
        (record, bytes)
    }
    let good = bound(
        "mesh.dependency-policy/v2",
        Some(Json::Array(vec![j(70), j(71)])),
    );
    let mut history = DependencyPolicyHistory::new(binding()).unwrap();
    apply(&mut history, &enrollment());
    apply(&mut history, &good);
    assert!(
        matches!(history.records.get(&good.0.payload), Some((_, Event::Grant { bindings: Some((a, b)), .. })) if *a == d(70) && *b == d(71))
    );
    for bad in [
        bound(
            "mesh.dependency-policy/v1",
            Some(Json::Array(vec![j(70), j(71)])),
        ),
        bound("mesh.dependency-policy/v2", None),
        bound("mesh.dependency-policy/v2", Some(Json::Array(vec![j(70)]))),
        bound(
            "mesh.dependency-policy/v2",
            Some(Json::Array(vec![j(0), j(71)])),
        ),
        bound(
            "mesh.dependency-policy/v3",
            Some(Json::Array(vec![j(70), j(71)])),
        ),
    ] {
        let mut history = DependencyPolicyHistory::new(binding()).unwrap();
        apply(&mut history, &enrollment());
        refuse(&mut history, &bad);
    }
}
