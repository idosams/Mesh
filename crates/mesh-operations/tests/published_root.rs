//! The operation owner generates and accounts for its own protocol artifacts.
mod common;

use mesh_operations::{encode_canonical, CanonicalEncode, CanonicalType, ObjectId, Operation};
use std::collections::BTreeSet;

fn documents() -> Vec<(&'static str, String)> {
    let root = ObjectId::from_bytes([0x17; 16]);
    let operation = Operation::InitializeWorkspace { root_id: root };
    let schema = operation.schema();
    assert_eq!(schema.fields.len(), 1);
    let CanonicalType::Bytes(Some(width)) = schema.fields[0].ty else {
        panic!("root identity must retain a fixed byte width");
    };
    let domain = schema.domain;
    let name = schema.fields[0].name;
    let input: String = root
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let encoding: String = encode_canonical(&operation)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    vec![
        (
            "protocol/operations/v0/workspace-root-schema.json",
            format!(
                concat!(
                    "{{\n  \"record\": \"{}\",\n  \"schema\": {{\n",
                    "    \"domain_tag\": \"{}\",\n    \"fields\": [\n      {{\n",
                    "        \"name\": \"{}\",\n        \"type\": \"bytes\",\n",
                    "        \"byte_length\": {}\n      }}\n    ]\n  }}\n}}\n"
                ),
                domain, domain, name, width
            ),
        ),
        (
            "protocol/operations/v0/workspace-root-vector.json",
            format!(
                concat!(
                    "{{\n  \"record\": \"{}\",\n  \"input\": {{\n    \"root_id\": \"{}\"\n  }},\n",
                    "  \"canonical_encoding_hex\": \"{}\"\n}}\n"
                ),
                domain, input, encoding
            ),
        ),
    ]
}

#[test]
fn generated_root_documents_match_exactly_and_no_file_is_unaccounted_for() {
    let expected = documents();
    for (path, content) in &expected {
        assert_eq!(
            &common::read_repo_file(path),
            content,
            "published artifact drift: {path}"
        );
    }
    let mut found = BTreeSet::new();
    let mut pending = vec![String::from("protocol/operations")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(common::repo_path(&directory)).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            let path = format!("{directory}/{name}");
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                pending.push(path);
            } else if name != "README.md" {
                assert!(kind.is_file());
                found.insert(path);
            }
        }
    }
    assert_eq!(
        found,
        expected.iter().map(|(path, _)| path.to_string()).collect()
    );
}

#[test]
#[ignore = "explicit publication regeneration only"]
fn write_published_root() {
    for (path, content) in documents() {
        std::fs::write(common::repo_path(path), content).unwrap();
    }
}
