#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const manifestSource = await readFile(resolve(root, "crates/mesh-types/src/manifest.rs"), "utf8");
const adr = await readFile(resolve(root, "docs/adr/0044-select-a-scalable-large-file-manifest-representation.md"), "utf8");
const pagingSource = await readFile(resolve(root, "crates/mesh-daemon/src/manifest_paging.rs"), "utf8");

export function verifyIdentity(source, paging, decision) {
  assert.match(source, /pub struct FileManifest\s*\{[\s\S]*byte_length: u64,[\s\S]*content_hash: Digest32,[\s\S]*chunks: Vec<ChunkRef>/);
  assert.match(source, /DomainTag::new\("mesh\.v0\.file-manifest"\)/);
  assert.match(source, /writer\.u64\(self\.byte_length\);[\s\S]*writer\.digest\(&self\.content_hash\);[\s\S]*writer\.sequence\(&self\.chunks/);
  assert.match(source, /FieldSchema::new\("byte_length", CanonicalType::Unsigned\),[\s\S]*FieldSchema::new\("content_hash", CanonicalType::Bytes\(Some\(32\)\)\),[\s\S]*"chunks"/);
  assert.match(decision, /## Decision\s+Retain the v0 logical manifest and candidate B\./);
  assert.match(decision, /Candidate C is not authorized/);
  assert.match(paging, /pub const MANIFEST_PAGE_REFERENCES: usize = 256/);
  assert.match(paging, /DomainTag::new\("mesh\.physical\.file-manifest-page\.v1"\)/);
  assert.match(paging, /DomainTag::new\("mesh\.physical\.file-manifest-index\.v1"\)/);
  assert.match(paging, /PageAtOrAbove\(NonZeroUsize\)/);
  assert.doesNotMatch(paging, /impl Default for ManifestPagingPolicy/);
  assert.match(paging, /canonical_digest::<Blake3, _>\(&logical\)/);
  assert.match(paging, /if found != expected_manifest_id/);
  assert.match(paging, /DuplicatePage/);
}

verifyIdentity(manifestSource, pagingSource, adr);
if (process.argv.includes("--mutations")) {
  for (const [name, source, paging, decision] of [
    ["v0 domain", manifestSource.replace("mesh.v0.file-manifest", "mesh.v1.file-manifest"), pagingSource, adr],
    ["v0 field order", manifestSource.replace("writer.u64(self.byte_length);", "writer.sequence(&self.chunks, |_writer, _chunk| {});"), pagingSource, adr],
    ["page width", manifestSource, pagingSource.replace("MANIFEST_PAGE_REFERENCES: usize = 256", "MANIFEST_PAGE_REFERENCES: usize = 255"), adr],
    ["page domain", manifestSource, pagingSource.replace("mesh.physical.file-manifest-page.v1", "mesh.v0.file-manifest"), adr],
    ["index domain", manifestSource, pagingSource.replace("mesh.physical.file-manifest-index.v1", "mesh.v0.file-manifest"), adr],
    ["C implied", manifestSource, pagingSource, adr.replace("Candidate C is not authorized", "Candidate C is authorized")]
  ]) assert.throws(() => verifyIdentity(source, paging, decision), undefined, `${name} mutation was admitted`);
  console.log("manifest-scaling compatibility mutations: PASS (6 identity/physical-boundary drifts rejected)");
}
console.log("manifest-scaling compatibility: PASS (v0 authoritative; explicit candidate-B paging; C refused)");
