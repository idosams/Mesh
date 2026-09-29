# Key custody: what each backend actually guarantees

**Maturity: security guarantee reference.** This describes the backend boundary, while the
end-to-end product still uses narrower local proof paths. See
[Project status](../../docs/project-status.md) and [Security policy](../../.github/SECURITY.md).

The output task `01KZC2CDEKQ0BDNVZ96ZMM100P` calls "the platform key backends and their guarantee
documentation". `platform/mesh-keychain/src/support.rs` carries the same rows as data and
`tests/key_isolation.rs` asserts this file and that table agree, so a row edited here and not there
fails `npm test`.

Read the top section first. It is the part a release cannot ship without.

## 1. Can an agent advance canonical state?

**No—not from an agent-held actor key.** Mesh has two deliberately separate authority paths:

The legacy Ed25519 capability route ends in a value the software actor-key backend cannot build:

1. Canonical state advances on a signature made under a `Capability<HumanHeld>`. `HumanHeld` is the
   only tier whose action vocabulary contains `AdvanceCanonicalHead`; `DelegatedAction` has no such
   variant, so an agent's capability cannot *name* it.
2. The only constructor of a `Capability<HumanHeld>` is `Capability::root`, which takes a
   `HumanKeyAttestation`.
3. `HumanKeyAttestation` has no public constructor. The one inside `mesh-crypto` takes an
   `IsolationProof` by value.
4. The only constructor of an `IsolationProof` is `CustodyBackend::isolation_proof`, and it returns
   `None` for every backend below `IsolationClass::OsMediated`.

`SoftwareCustody` reports
`CustodyBackend::SoftwareInProcess`, which is `IsolationClass::InProcess`. It therefore cannot
obtain a proof, cannot build an attestation, cannot mint a human capability, and cannot reach the
action. It does not implement `HumanKeyCustody`, and a test asserts no source in this crate does.

The macOS alpha route accepts only a typed ES256 receipt from the separate P-256 Secure Enclave
  credential. Its private key has no export operation, the public API has no generic sign-bytes
  method, Security.framework requires fresh user presence for private-key use, and the daemon
  reconstructs and verifies the exact shown statement before advancing the shared version.

**An agent that fully compromises the ordinary actor-key path gets a key it can author ChangeSets
with and can ask macOS to show an approval prompt. It cannot silently produce the user-presence
receipt that advances the shared version.**

## 2. The isolation order

| Class | The secret is | Survives an agent reading this process's memory |
|---|---|---|
| `in-process` | a value in this address space | no |
| `os-gated` | held by the OS, **released to the process** to use | no |
| `os-mediated` | held by the OS, which **signs** without releasing it | yes |
| `hardware-non-exportable` | inside a coprocessor with no export operation | yes, and survives a compromised kernel |

`CustodyRequirement::HUMAN_APPROVAL` is `os-mediated`. That is the line where "the key is used, not
held" becomes true. It is deliberately not `hardware-non-exportable`: requiring hardware would
answer a compromised kernel, which `docs/threat-model.md` §11 puts out of scope, and would be
unsatisfiable on every platform in §4 below.

A caller states a **minimum** and is refused, never downgraded —
`CustodyRequirement::check` returns `CustodyError::InsufficientIsolation { required, offered }`.

## 3. The software fallback and its weaker guarantee

`SoftwareCustody` is a real Ed25519 signer. Its signatures verify under the same audited
`mesh_crypto::Ed25519` every peer verifies with — there is one definition of a Mesh signature.

**Its guarantee is `in-process`, and that is weaker than it sounds.** What holds:

- No accessor, no `to_bytes`, no `Clone`, no `Debug` of the secret, no serialization. No code path
  in the workspace exports the scalar, and a compile-time scan (`src/no_export.rs`) fails the build
  if one is added.
- There is **no constructor that takes secret bytes**. `SoftwareCustody::generate` is the only one,
  it reads from the operating system's random source, and it scrubs the seed buffer before
  returning. "Generate here, store there" is not a shape a caller can write.
- `ed25519_dalek::SigningKey` is `ZeroizeOnDrop` under the pinned `zeroize` feature, so the scalar
  is overwritten when custody drops.

What does **not** hold, plainly:

- A debugger, `ptrace`, `task_for_pid`, a core dump, a crash reporter or another thread in the same
  process reads the scalar out of memory. **An agent process running as the same user with debug
  privileges can extract this key.** The task's third acceptance criterion is met for in-workspace
  code paths and is **not met** against that adversary, and the software fallback is the reason it
  cannot be — which is exactly why the fallback may never hold a human approval key.
- The buffer scrub is a plain store loop plus a `SeqCst` compiler fence, not `write_volatile`,
  because this crate has no `unsafe`. That is a strong hint to the optimiser, not a guarantee.

Anything that needs the criterion to hold against a same-user process must require
`os-mediated` and take the refusal.

## 4. What each platform can do with an **Ed25519** key

The rows below are a documented reading of published platform APIs, not a measurement of them.
Nothing in this repository has been run against a Secure Enclave, a TPM, a CNG provider, a kernel
keyring or a hardware token. Each row names the API fact it rests on so it can be checked.

| Platform | Mechanism | Holds Ed25519 | Reachable class for a Mesh actor key | Implemented |
|---|---|---|---|---|
| macos | `apple-secure-enclave` | **no** | `in-process` | no |
| macos | `apple-keychain` | yes | `os-gated` | yes, source implemented; signed-app acceptance required |
| windows | `windows-cng` | **no** | `in-process` | no |
| linux | `linux-kernel-keyring` | yes | `os-gated` | no |
| macos | `hardware-token` | yes | `hardware-non-exportable` | no |
| windows | `hardware-token` | yes | `hardware-non-exportable` | no |
| linux | `hardware-token` | yes | `hardware-non-exportable` | no |

**The Apple Secure Enclave cannot hold an Ed25519 key.** It generates and holds NIST P-256 only:
`SecKeyCreateRandomKey` with `kSecAttrTokenIDSecureEnclave` accepts `kSecAttrKeyTypeECSECPrimeRandom`
at 256 bits, and CryptoKit exposes `SecureEnclave.P256` with no `Curve25519` counterpart. Plan §8.1
pins Ed25519 for actor keys, so "Mesh Ed25519 actor key, in the Secure Enclave" is not a thing that
can be built. It is unavailable, not unbuilt. `docs/threat-model.md` §11 currently says key isolation
"is delegated to the OS keychain and Secure Enclave", and a reader is entitled to take that as a
description of where the key will end up; on Apple platforms it will not.

**Windows CNG has no Ed25519 provider.** The mechanism is `os-mediated` — a CNG key storage
provider signs without releasing the key — but neither the Microsoft Software Key Storage Provider
nor the Platform Crypto Provider offers Ed25519, so an Ed25519 actor key needs a third-party KSP
that Mesh does not ship.

**The macOS Keychain and the Linux kernel keyring store bytes; they do not sign Ed25519.** The seed
is read back into this address space to be used. Strong access control in front of the read makes
them `os-gated` at rest and changes nothing in use.

**The only route to a hardware-isolated Ed25519 approval key today is an external token.** Ed25519
in non-exportable hardware is real on YubiKey 5 — the OpenPGP applet from firmware 5.2.3, PIV from
5.7 — reached over PKCS#11. It needs a device the user bought.

## 5. Separate human-approval custody on macOS

Mesh human approval does not reuse the Ed25519 actor key. The macOS desktop has a separate P-256
credential generated by `SecKeyCreateRandomKey` with `kSecAttrTokenIDSecureEnclave`,
`kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly`, and a private-key access control requiring both
private-key use and user presence. Only the public SEC1 point leaves the platform adapter. The
private key signs a typed `HumanApprovalReceiptDraft`; no generic sign-bytes method is exported.

The signed receipt binds the exact workspace, canonical and reviewed heads, complete review bundle,
validation result, policy epoch, enrolled public credential, fresh challenge, application scope,
user-verification marker, and approve/reject decision. The daemon independently reconstructs that
context and verifies the ES256 signature before advancing the shared version. Legacy Ed25519
approval records remain readable but cannot satisfy this production authority path.

The automated boundary test is read-only so it cannot surprise a developer with an enrollment
prompt. An interactive macOS app run must prove enrollment, cancellation, Touch ID or password
presence, approval, and persistence across restart before the alpha calls this journey stable.

## 6. What is not implemented

Native Keychain actor persistence is source-implemented, with signed-app acceptance pending.
There is no CNG, kernel keyring or PKCS#11 implementation. The implemented Secure Enclave path is only the separate P-256 human
approval credential described above; it does not change the actor-key availability table.


## 7. Persistent macOS execution identity

`AppleActorCustody` holds a native installation account and expected public key, with no persistent
in-memory seed. Explicit creation uses OS randomness and create-only Keychain insertion. Opening
requires an independently admitted public key; every signature reloads the value and refuses a
missing, locked, malformed or changed identity. No public secret import/export, replacement, deletion,
rotation or fallback is exposed. Failed provisioning retains any created item for reconciliation.

The bridge first requires the existing Apple-signed `dev.mesh.desktop` application identity and its
exact app-private keychain group. It uses a separate generic-password service for worker Ed25519,
Data Protection storage, no synchronization and device-only access after first unlock. Reads disallow
interaction. This never reads or signs with the P-256 human approval credential. An unsigned helper
is unavailable, and other operating systems refuse this backend.

The reported backend is `AppleKeychain`, whose isolation is `OsGated`, below human approval's
required level. Ed25519 signing still reads the seed into process memory. Rust seed buffers are
scrubbed and the signing object zeroizes on drop; Security/Foundation internal copies and hostile
process-memory access are not covered by an erasure guarantee. Source support does not establish
that a machine is provisioned or eligible. Deterministic storage-double and native item-shape tests
are not evidence of actual signed-app creation, reopening or signing; that acceptance remains open.
