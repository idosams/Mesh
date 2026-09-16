// The ambient-storage-and-network scan, generalized out of
// `crates/mesh-types/src/no_ambient_io.rs` (task 01KZC25P0N5WMX8FY2Q7BFH84Z).
//
// **Why it exists at all.** "This crate declares no storage or network dependency"
// and "this crate reaches no storage and no network" are different claims, and
// only the first is a manifest fact. `std` ships a filesystem, a socket, a process
// table and an OS surface, so a lane can break plan §8.3's first bullet without
// touching `Cargo.toml`. The mesh-types lane found that and closed it with a
// compile-time `const fn` scan over its own sources; this is the same scan applied
// from outside, to every unit the map holds to the same rule.
//
// **What it is not — carried over verbatim from that file, because it is still
// true here.** A proof. It reads text, so `use std as s;` followed by `s::fs::read`
// walks straight past it, as does anything reached through a re-export or a macro.
// It catches the ordinary way I/O enters a crate that is not supposed to have any:
// a lane that needed a file and did not know the rule. It is a **lint over source
// text**, and the guarantee that a crate cannot DEPEND on storage or network code
// is the manifest rule, not this.
//
// Two deliberate narrowings:
//  - `src/` only. An integration test or a benchmark that opens a file does not
//    give the library the capability.
//  - `std::io` is absent, for the reason the mesh-types file gives: its traits and
//    its error type carry no capability, and banning the word would ban
//    `std::io::Error` from an error enum without preventing a byte of I/O.

/** The `std` module paths that reach outside the process. */
export const AMBIENT = ['std::fs', 'std::net', 'std::process', 'std::os'];

/**
 * Every ambient module named in `source`, with the 1-based line it appears on.
 *
 * Comments are NOT skipped. mesh-crypto's in-crate scan skips them so that its own
 * documentation may name what it bans; here the exemption is per file in the map,
 * which is the surface a reviewer reads, so a doc comment that names `std::fs` in a
 * pure crate is reported rather than quietly allowed. That is the stricter of the
 * two and the difference is one map entry when it is genuinely prose.
 *
 * @param {string} source
 * @returns {{ needle: string, line: number }[]}
 */
export function ambientHits(source) {
  const hits = [];
  source.split('\n').forEach((text, index) => {
    for (const needle of AMBIENT) {
      if (text.includes(needle)) hits.push({ needle, line: index + 1 });
    }
  });
  return hits;
}
