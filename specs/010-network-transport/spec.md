# Feature Specification: Network Transport

**Feature Branch**: `010-network-transport`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create the specification for Git network transport in git.rs. Cover: local repositories, file transport, SSH transport, HTTP/HTTPS transport, Git smart protocol, protocol versions, upload-pack, receive-pack, fetch negotiation, push negotiation, ref advertisement, packfile transfer, shallow repositories, partial clone where applicable, authentication boundaries, remote helpers, redirects, proxy configuration, TLS, connection failures, retries, progress reporting. Define compatibility with standard Git servers and clients. Separate transport abstraction from protocol implementation and repository/object handling. Specify which protocols/features belong to each implementation phase."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Clone and fetch from local paths (Priority: P1)

A user clones a repository from a local path, fetches new commits, and pushes back, with refs, objects, and diagnostics identical to standard Git.

**Why this priority**: Local transport needs no network, no credentials, and no protocol framing. It delivers clone/fetch/push value first and proves the fetch/push negotiation and pack-transfer core that every later transport reuses.

**Independent Test**: Can be fully tested by running clone/fetch/push between fixture repositories via filesystem paths under both implementations and comparing refs, objects, work trees, stdout/stderr, and exit codes.

**Acceptance Scenarios**:

1. **Given** a source repository with branches and tags, **When** the user clones it via a local path, **Then** refs, HEAD, checked-out files, and remote configuration match a standard-Git clone of the same source.
2. **Given** new upstream commits, **When** the user fetches, **Then** only the missing objects transfer and the destination refs update exactly like standard Git (fast-forward, forced, new-branch, and rejected cases).
3. **Given** local commits, **When** the user pushes to a local-path remote, **Then** the destination accepts or rejects (non-fast-forward, hook-equivalent outcomes where hooks are out of scope) with identical diagnostics.

---

### User Story 2 - Exchange history with standard Git servers and clients (Priority: P2)

A user fetches from and pushes to a standard Git server over SSH or HTTPS, and a standard Git client fetches from and pushes to a git.rs-served repository, with identical protocol behavior in both directions.

**Why this priority**: Interoperation with the installed base is the point of the transport work. Client parity (git.rs speaks to C git servers) and server parity (C git clients speak to git.rs upload-pack/receive-pack equivalents) are independently valuable and independently testable.

**Independent Test**: Can be fully tested with a matrix of client/server pairs (git.rs client vs standard-Git daemon/server fixtures; standard-Git client vs git.rs served endpoints) over SSH and HTTPS, comparing transferred objects, ref updates, and failure diagnostics.

**Acceptance Scenarios**:

1. **Given** a standard Git server over SSH/HTTPS, **When** the user clones, fetches, and pushes with git.rs, **Then** results and diagnostics match standard-Git client behavior for the same operations.
2. **Given** a repository served by git.rs, **When** a standard Git client clones, fetches, and pushes, **Then** the client succeeds with identical ref/object outcomes as against a standard-Git server.
3. **Given** authentication, redirect, or proxy configuration, **When** the user connects, **Then** credential handling boundaries, redirect following, and proxy use match standard Git policy without leaking secrets into logs or error output.

---

### User Story 3 - Work with shallow and partial histories (Priority: P3)

A user clones with limited depth or partial object filters to save time and disk, then fetches more history or missing objects on demand, with boundary and promisor semantics matching standard Git.

**Why this priority**: Shallow/partial support saves bandwidth but depends on correct negotiation and on-demand fetching; wrong boundaries corrupt history walks, so it gates behind correct full-clone transport.

**Independent Test**: Can be fully tested with depth-1/deepen/shorten and filter-based fixtures, comparing shallow-boundary files, fetched object sets, and on-demand fault-in behavior against standard Git.

**Acceptance Scenarios**:

1. **Given** a depth-1 clone request, **When** the user clones, **Then** history truncates at the same boundary commit(s) with identical shallow-marker state as standard Git.
2. **Given** a shallow repository, **When** the user deepens or unshallows, **Then** the boundary moves exactly like standard Git and subsequent walks see the newly fetched commits.
3. **Given** a partial-clone filter request, **When** the user clones and then checks out a path whose objects were excluded, **Then** missing objects fault in on demand with the same observable outcome as standard Git.

---

### User Story 4 - Survive failing networks predictably (Priority: P2)

A user on a flaky connection sees the same progress output, retry behavior, and failure diagnostics as standard Git, and interrupted transfers never corrupt the local repository.

**Why this priority**: Network failure is normal operation. Predictable retries, honest progress, and atomic application of received data are safety boundaries, not polish.

**Independent Test**: Can be fully tested with fault-injecting servers/proxies (dropped connections, truncated packs, slow streams, auth failures, redirect loops) comparing progress output, retry counts, diagnostics, exit codes, and post-failure repository integrity.

**Acceptance Scenarios**:

1. **Given** a dropped connection mid-transfer, **When** the transfer fails, **Then** the diagnostic names the failure class, the exit code matches standard Git, and the local repository contains no half-applied refs or partial packs visible to readers.
2. **Given** a retryable failure within policy, **When** the operation runs, **Then** it retries the same number of times with the same backoff-observable behavior and progress resumption as standard Git.
3. **Given** an authentication or TLS failure, **When** the connection is refused, **Then** the diagnostic matches the standard failure class and no credentials are exposed in output.

---

### Edge Cases

- Local path that is not a repository, is a bare vs non-bare repository, or disappears mid-transfer: standard "not a git repository" / transport diagnostics, not panics.
- `file://` URLs vs plain paths, relative remote URLs, and `insteadOf`/`pushInsteadOf` rewriting applied identically before transport selection.
- SSH command override (`GIT_SSH`/`GIT_SSH_COMMAND`/`core.sshCommand`), missing ssh binary, host-key failure, multiplexed-control-path staleness: identical failure classes and diagnostics.
- HTTP proxy env (`http_proxy`/`https_proxy`/`no_proxy`) and per-URL proxy config; authenticated proxies never log credentials.
- Redirects: same-origin followed within policy; cross-origin credential stripping; redirect loops reported as failures, not followed forever.
- Protocol version fallback: server without v2 support falls back to v0/v1 with identical capability outcome; unknown server capabilities ignored per protocol rules, malformed framing is a transport error.
- Empty fetch (everything up to date), empty push (nothing to push), delete-ref push, and non-fast-forward rejection each produce the standard summary lines and exit codes.
- Shallow boundaries on merge commits, deepen past root, unshallow with no server support: boundary files and diagnostics match standard Git.
- Partial-clone fault-in during checkout/diff/log: missing-object fetch triggered identically; unfetchable objects reported as missing, not as corruption of existing objects.
- Interrupts (Ctrl-C) and timeouts: partial pack data quarantined and discarded; locks released; repository left in pre-transfer state.
- Progress output to non-terminals: suppressed or machine-stable like standard Git; `--progress`/`--quiet` force the documented behavior.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST transfer history over local paths (plain directory and `file://`) with ref and object outcomes identical to standard Git, including alternates-aware reads where the object-store specification defines them.
- **FR-002**: The system MUST implement the Git smart protocol framing (pkt-line, flush, delim, response-end, capability advertisement parsing) byte-compatible with standard Git for covered protocol versions.
- **FR-003**: The system MUST negotiate protocol version 2 (command-based `ls-refs` + `fetch`) where the server offers it and MUST fall back to version 0/1 correctly where it does not, with identical effective capability sets either way.
- **FR-004**: The system MUST implement the fetch client: ref advertisement consumption, want/have negotiation rounds terminating with the same round count class and outcome as standard Git, and packfile reception with trailer-checksum verification before any ref update.
- **FR-005**: The system MUST implement the push client: ref-update command construction, forced vs fast-forward-only semantics, delete-ref handling, and report-status parsing with per-ref success/rejection lines identical to standard Git.
- **FR-006**: The system MUST serve fetch requests (upload-pack equivalent): read the requested wants/haves, negotiate, and emit a packfile plus ref advertisement that standard Git clients accept, with identical outcomes to a standard-Git server on the same repository.
- **FR-007**: The system MUST serve push requests (receive-pack equivalent): validate ref updates (fast-forward, force policy, delete policy), apply accepted updates atomically, and emit per-ref status lines standard Git clients parse.
- **FR-008**: The system MUST transfer packfiles with integrity: trailer checksum verified on receipt, corrupt/truncated packs rejected before refs move, and received objects quarantined until the owning operation accepts them (per the object-store quarantine rule).
- **FR-009**: The system MUST implement SSH transport honoring `GIT_SSH`/`GIT_SSH_COMMAND`/`core.sshCommand` precedence, batch-mode failure semantics, and standard host-key/auth-failure diagnostics; it MUST NOT implement its own SSH wire protocol.
- **FR-010**: The system MUST implement HTTP/HTTPS smart transport (info/refs + stateless RPC framing for covered protocol versions) with identical request sequences and fallback behavior to standard Git against the same servers.
- **FR-011**: The system MUST follow redirects within the documented policy (same-danger-class rules, credential stripping on cross-origin, loop bound) and MUST report redirect failures with standard diagnostics.
- **FR-012**: The system MUST apply proxy configuration (per-URL config plus standard proxy environment variables with `no_proxy` bypass) identically to standard Git, and MUST NOT emit proxy credentials into output, logs, or diagnostics.
- **FR-013**: The system MUST validate TLS server identity per standard policy (hostname verification, system trust store, standard failure diagnostics) and MUST NOT offer an insecure-skip option wider than standard Git's documented escape hatch.
- **FR-014**: Authentication MUST stop at defined boundaries: credential helpers and configured stores are invoked through the same lookup protocol as standard Git; passwords/tokens MUST NOT be persisted beyond the helper contract and MUST NOT appear in output or error text.
- **FR-015**: The system MUST support remote helpers for non-native transports through the documented helper protocol where declared in scope; helper absence MUST produce the standard "unable to find remote helper" class diagnostic.
- **FR-016**: The system MUST implement shallow semantics: depth, deepen, shorten, unshallow, and boundary-file maintenance with identical boundary commits and graft-equivalent visibility to standard Git for covered options.
- **FR-017**: The system MUST implement partial-clone filters where declared (blob:none and tree-filter-equivalent subsets first), with promisor marking and on-demand fault-in during checkout/diff/log; unfetchable objects MUST surface as missing-object diagnostics, not silent content substitution.
- **FR-018**: Connection failures MUST be classified (DNS, refused, reset, timeout, truncated, protocol framing error, auth, TLS) with standard-Git-equivalent diagnostics and exit codes; retries apply only to the documented retryable classes with bounded attempts and must be observable in tests.
- **FR-019**: Progress reporting (`Receiving objects`, `Resolving deltas`, `Writing objects`, ref-update summaries) MUST match standard Git text and `--progress`/`--quiet`/non-terminal suppression rules for covered commands.
- **FR-020**: Received state MUST apply atomically: refs move only after pack verification (fetch) or update validation (push-serve); failures leave prior refs, index, and work tree intact; temp/lock files are removed on success and failure.
- **FR-021**: Layering MUST be separated: transport moves bytes (connect/read/write/close per URL scheme); protocol implements negotiation and framing over a byte stream; repository/object handling owns refs, packs, shallow state, and quarantine. Layers interact only through declared interfaces so each is testable without the others.
- **FR-022**: Every transport-affecting configuration key and environment variable in scope MUST be documented per command with identical precedence and effect to standard Git; undeclared keys MUST NOT change transfer behavior.
- **FR-023**: Compatibility MUST hold in both directions: git.rs clients against standard-Git servers, and standard-Git clients against git.rs-served endpoints, verified by the interop matrix for every phase-gated feature.
- **FR-024**: Phase boundaries MUST be explicit: each protocol, transport, and feature (shallow, partial clone, helpers, proxy/TLS variants) names the phase it belongs to; anything outside the current phase MUST fail with a clear unsupported diagnostic, never a silently wrong transfer.

### Key Entities *(include if feature involves data)*

- **Transport**: A byte-moving channel for a URL scheme (local-path, file, SSH subprocess, HTTP/HTTPS); owns connect/read/write/close, proxies, TLS, and retries — never protocol semantics.
- **Smart Protocol**: The framed Git dialogue (pkt-line framing, ref advertisement, capability negotiation, want/have rounds, pack request/response, report-status); versioned (v0/v1/v2).
- **Protocol Version**: v0 (original upload-pack dialogue), v1 (version-line + extended capabilities), v2 (stateless command RPC: `ls-refs`, `fetch`); negotiated per connection with fallback.
- **Ref Advertisement**: The server's ref list (names, IDs, symref targets, capabilities/ablities) opening a fetch or push session.
- **Fetch Negotiation**: Client haves vs server wants rounds converging on a minimal pack; ends with depth/filter/shallow directives where applicable.
- **Push Negotiation**: Client update commands vs server validation producing per-ref accept/reject report-status.
- **Packfile Transfer**: The object payload of a fetch (or push-with-thin-pack where covered); checksummed and quarantined until accepted.
- **Shallow Boundary**: The truncation frontier of a shallow clone (boundary commits recorded locally); deepening moves it, unshallowing removes it.
- **Promisor / Partial State**: The record that some objects were deliberately excluded by filter and may be faulted in later from the promisor remote.
- **Remote Helper**: An external program speaking the helper protocol for non-native transports; boundary for transports git.rs does not implement natively.
- **Credential Boundary**: The exact surface where secrets enter (helpers, configured stores, interactive prompts) and the rule that they never cross into logs, diagnostics, or persisted state beyond the helper contract.
- **Progress Reporter**: The stderr transfer-progress renderer honoring `--progress`/`--quiet`/terminal rules.
- **Quarantine Area**: The incoming-object staging directory invisible to normal reads until the transfer is accepted.

## Detailed Behavioral Specification

Each item states expected behavior, input/output format, compatibility requirements, error behavior, and required tests.

### 1. Local repositories and file transport

- **Expected**: Plain paths and `file://` URLs open the source repository directly; fetches copy/link missing objects with have-based pruning; pushes update destination refs under the same fast-forward/force rules as network pushes.
- **Format**: No protocol framing; ref lists and packfiles handled as local files; `insteadOf` rewriting applied before path resolution.
- **Compatibility**: Transferred object sets, ref outcomes, and summary output MUST equal standard Git for identical source/destination pairs.
- **Error**: Missing/non-repository paths, permission failures, and disappearing sources are transport diagnostics with standard exit codes.
- **Tests**: Clone/fetch/push matrix (bare/non-bare, branches/tags/deletes/force, up-to-date no-ops) byte- and behavior-compared in both directions.

### 2. SSH transport

- **Expected**: Command selection precedence `GIT_SSH_COMMAND` > `core.sshCommand` > `GIT_SSH` > default ssh; batch failure (auth/host-key) surfaces the ssh diagnostic class; multiplexing flags passed through where standard Git passes them.
- **Format**: `ssh [flags] host "git-upload-pack 'path'"` command construction identical to standard Git for covered options.
- **Compatibility**: Connection setup, failure classes, and exit propagation MUST match; no independent SSH wire implementation.
- **Error**: Missing binary, auth refusal, host-key mismatch, and broken pipes reported distinctly with standard wording classes.
- **Tests**: Fake-ssh harness asserting command-line construction plus auth/host-key/timeout failure fixtures vs standard Git.

### 3. HTTP/HTTPS transport

- **Expected**: Smart-info/refs discovery then stateless RPC for covered versions; `http.followRedirects`, `http.extraHeader`, user-agent, and auth-challenge flows honored per documented keys; dumb-HTTP only where explicitly declared.
- **Format**: Request paths, query parameters, content types, and pkt-line bodies identical to standard Git for the same server capabilities.
- **Compatibility**: Request sequences and fallback decisions MUST match against capability-varying fixtures.
- **Error**: HTTP status classes mapped to standard diagnostics (401/403 auth, 404 not-found, 5xx server, redirect loops, framing errors on 200-with-garbage).
- **Tests**: Local HTTP fixture server matrix (capability variants, redirects, auth challenges, error statuses) comparing request logs and outcomes.

### 4. Smart protocol versions and advertisement

- **Expected**: v2 negotiated when offered (`version 2` request honored); `ls-refs` with symref/peel arguments; v0/v1 advertisement parsed (IDs, names, symrefs, capabilities, shallow lines); unknown capabilities ignored, malformed lines are protocol errors.
- **Format**: Exact pkt-line framing (`XXXX` length prefix, flush/delim/response-end markers), NUL-separated capability lists, `shallow`/`unshallow` directive lines.
- **Compatibility**: Byte-level framing MUST match; effective capability outcomes identical after fallback.
- **Error**: Truncated framing, bad lengths, or capability contradictions abort with protocol-error diagnostics before any ref moves.
- **Tests**: Framing corpus (golden sessions, fuzzed truncations/mutations) plus version-fallback matrix against capability-varying servers.

### 5. upload-pack (fetch serving) and fetch negotiation

- **Expected**: Multi-round have/want with `done`/`ready` semantics per version; server emits minimal sufficient pack; `deepen`/`shallow`/`unshallow` lines honored where shallow is covered; filter lines honored where partial clone is covered.
- **Compatibility**: Termination round, emitted pack membership, and shallow/filter responses MUST equal standard Git for identical have/want sets.
- **Error**: Unknown wants, invalid haves, and unsupported depth/filter requests rejected with standard diagnostics; no partial packs emitted as success.
- **Tests**: Negotiation fixtures (up-to-date, one-round, multi-round, merge-heavy, shallow/filter variants) comparing round transcripts and pack contents.

### 6. receive-pack (push serving) and push negotiation

- **Expected**: Update commands validated in order (old-ID matches, fast-forward or force allowed, delete policy); all-or-nothing atomicity where standard Git is atomic; per-ref `ok`/`ng` report-status lines.
- **Format**: Command list framing plus unpack + report-status response identical to standard Git.
- **Compatibility**: Accept/reject decisions and report lines MUST match for identical before/after ref states.
- **Error**: Stale old-IDs, non-fast-forward without force, and validation failures reported per-ref; transport continues to report all refs, exit code reflects overall failure.
- **Tests**: Push matrix (create/update/delete/force/stale/reject) against both server implementations with report-status comparison.

### 7. Packfile transfer integrity

- **Expected**: Trailer checksum verified before acceptance; objects land in quarantine; promotion is per-object atomic; accepted packs verify under standard inspection.
- **Compatibility**: Corrupt packs rejected identically; accepted packs readable by both implementations.
- **Error**: Checksum mismatch, truncation, or delta-base damage is a transfer failure with no ref movement.
- **Tests**: Corruption corpus (bit-flip, truncate, checksum damage) plus crosswise pack-acceptance checks.

### 8. Shallow repositories

- **Expected**: Clone/fetch `--depth N`, `--deepen N`, `--shorten N`, `--unshallow` maintain boundary files; walks stop at boundaries; boundary commits excluded from have sets per protocol rules.
- **Compatibility**: Boundary commit sets and walk visibility MUST equal standard Git for identical histories and depths.
- **Error**: Deepen-past-root and unshallow-against-unsupporting-server handled with standard diagnostics, never fabricating ancestors.
- **Tests**: Depth matrix on linear/merge/criss-cross histories comparing boundary files and log-walk membership.

### 9. Partial clone

- **Expected**: Covered filters (blob:none first, declared tree filters next) exclude matching objects, mark promisor state, and fault in on demand during checkout/diff/log; unfetchable paths report missing objects.
- **Compatibility**: Excluded sets, fault-in triggers, and fallback diagnostics MUST match standard Git for covered filters.
- **Error**: Unsupported filter strings rejected with standard diagnostics rather than treated as no-filter.
- **Tests**: Filter fixtures comparing excluded-object sets, fault-in transcripts, and end-state file bytes.

### 10. Authentication, helpers, redirects, proxy, TLS

- **Expected**: Credential lookup order (helpers, then configured stores, then interactive where declared) with per-origin scoping; helpers invoked via the standard helper protocol; redirects followed within policy with credential stripping; proxy env/config honored with bypass lists; TLS verified against system trust.
- **Compatibility**: Lookup order, redaction behavior, redirect/proxy decisions, and failure classes MUST match standard Git.
- **Error**: Auth/TLS/proxy failures are distinct diagnostics; secrets never appear in output; helper absence is the standard missing-helper error.
- **Tests**: Fake-helper/challenge-proxy/redirect-loop fixtures asserting lookup order, redaction (secret strings absent from all captured output), and diagnostic equivalence.

### 11. Failures, retries, progress

- **Expected**: Failure classes distinguished (see FR-018); only documented retryable classes retried with bounded attempts; progress lines follow standard text and suppression rules; interrupts discard quarantine and release locks.
- **Compatibility**: Retry counts, progress bytes on terminals vs pipes, and post-failure repository state MUST match standard Git observably.
- **Error**: Retry exhaustion reports the last failure class; partial state never promoted.
- **Tests**: Fault-injection matrix (drop/reset/slow/truncate/auth/TLS/redirect-loop) comparing transcripts, exit codes, and post-failure integrity.

### 12. Interoperability tests

- **Direction A (git.rs client, standard-Git server)**: Clone/fetch/push/shallow/partial suites against standard-Git–served fixtures (local-path, ssh, https) asserting identical refs, objects, outputs, and diagnostics.
- **Direction B (standard-Git client, git.rs server)**: Same suites with roles reversed, asserting the standard client behaves identically as against a standard server.
- **Harness**: Shared fixture repositories, request/round transcripts, byte comparison of packs and ref states, stdout/stderr/exit-code comparison; corruption/fault fixtures in both directions.
- **Gates**: No family passes unless both directions agree; any silent object-set, ref-state, or diagnostic divergence is a failure.

## Implementation Phases

- **Phase T1 — Local transport + fetch/push core (first)**: FR-001, FR-004–FR-008 (local bindings), FR-020–FR-021 scaffolding. No network, no shallow/partial. Gate: local clone/fetch/push matrix green both directions.
- **Phase T2 — Server endpoints**: FR-006–FR-007 over local/ssh-command channels so standard-Git clients interoperate; quarantine + atomicity gates. Gate: Direction-B suites green for local transport.
- **Phase T3 — SSH client + protocol versions**: FR-002–FR-003, FR-009 with fake-ssh harness; v2 with v0/v1 fallback. Gate: version-fallback matrix + SSH failure fixtures green.
- **Phase T4 — HTTPS client**: FR-010–FR-013 (redirect/proxy/TLS/auth boundaries per FR-011–FR-014, FR-022). Gate: HTTP fixture-server matrix green incl. redaction tests.
- **Phase T5 — Shallow + partial clone**: FR-016–FR-017 with boundary/promisor and fault-in suites. Gate: depth/filter matrices green both directions.
- **Phase T6 — Hardening and parity**: FR-015 (remote helpers), FR-018–FR-019 (retry/progress refinement), remaining option surface to full parity. Anything beyond the gated set stays rejected with explicit unsupported diagnostics per FR-024.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Local-path clone/fetch/push results (refs, objects, work trees, outputs, exit codes) are identical between implementations across the full local matrix.
- **SC-002**: Against standard-Git servers over SSH and HTTPS, git.rs transfers identical object sets and ref states with equivalent diagnostics across the client interop matrix.
- **SC-003**: Against git.rs-served endpoints, standard-Git clients behave identically as against standard-Git servers across the server interop matrix.
- **SC-004**: Shallow and partial-clone fixtures show identical boundaries, excluded-object sets, fault-in behavior, and end-state files in both directions.
- **SC-005**: Fault-injection suites show zero half-applied transfers observed by readers, equivalent diagnostics/exit codes, and retry behavior matching policy in every case.
- **SC-006**: No secret material appears in any captured output, log, or diagnostic across the authentication test corpus (redaction property holds).
- **SC-007**: A user cloning, fetching, pushing, and recovering from failures observes no behavioral difference between implementations at any step (task-completion parity verified by the interop suites).

## Assumptions

- Standard Git behavior (protocol framing plus the `t/` transport suites) is the oracle; where documentation and the suite disagree, the suite wins.
- The object-store, refs, index, and porcelain-phases specifications are companion contracts; this spec defers byte layouts, locking, quarantine, and command-gating to them and defines only transport-observable behavior here.
- Pinned-version parity: framing and capability defaults match the pinned standard Git version; version-dependent differences are recorded, not treated as failures.
- No independent SSH wire or TLS stack implementation: system ssh and system trust roots are used; custom backends are out of scope.
- Credential helpers already configured by the user are invoked, never reimplemented; interactive prompting follows standard behavior only where declared and never stores secrets outside the helper contract.
- Performance targets (large-pack throughput, negotiation round counts on huge histories beyond correctness equivalence) are out of scope for this spec; correctness and byte-compatibility gate first.
