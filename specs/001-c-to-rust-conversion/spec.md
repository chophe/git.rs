# Feature Specification: C to Rust Conversion

**Feature Branch**: `[001-c-to-rust-conversion]`

**Created**: 2026-09-19

**Status**: Draft

**Input**: User description: "all file all units must be converted from c language into the rust, one by one all units classes libs files"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Convert Core Utilities (Priority: P1)

As a developer, I want to convert the core utility C files and libraries to Rust so that they form the foundation for the rest of the project's conversion.

**Why this priority**: Core libraries and utilities are prerequisites for higher-level components.

**Independent Test**: The Rust core utilities should compile successfully and pass equivalent unit tests that the original C code passed.

**Acceptance Scenarios**:

1. **Given** a C utility file, **When** it is converted to Rust, **Then** all exported functions have identical behavior.
2. **Given** the converted Rust library, **When** tested against crosswise differential suites, **Then** it produces byte-identical output to the C oracle.

---

### User Story 2 - Convert Object and ODB Layers (Priority: P2)

As a developer, I want to convert the git object and ODB parsing logic from C to Rust so that the core functionality is native.

**Why this priority**: It builds upon the core utilities and is necessary for almost all Git commands.

**Independent Test**: Can read and parse objects from a standard Git repository and match output of `git cat-file`.

**Acceptance Scenarios**:

1. **Given** a valid packfile, **When** the Rust ODB parser processes it, **Then** it parses all objects correctly.

---

### User Story 3 - Convert High-level Commands (Priority: P3)

As a developer, I want to convert individual git commands (e.g., status, rev-list, apply) one by one to Rust so that the full CLI surface is covered.

**Why this priority**: Depends on core utilities and ODB, completes the application surface.

**Independent Test**: Each command can be tested in isolation against the Git test suite.

**Acceptance Scenarios**:

1. **Given** the `t/` test suite, **When** a ported command runs via the shim, **Then** it passes all the oracle scripts.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST provide byte-compatible behavior with C Git across all functionality.
- **FR-002**: System MUST process repository on-disk formats exactly as the C version does (both reading and writing).
- **FR-003**: System MUST provide the same exit codes as C Git, including specific usage errors (e.g., 129).
- **FR-004**: System MUST NOT use FFI or link to the C Git implementation; it must be a pure-Rust rewrite.
- **FR-005**: System MUST pass all crosswise differential integration tests vs C Git.

### Key Entities

- **C Source File**: The original C implementation serving as the specification.
- **Rust Crate/Module**: The newly written port of the C code.
- **Crosswise Test Suite**: Integration tests comparing Rust and C binaries.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of the C Git source files are successfully ported to Rust.
- **SC-002**: 100% of the crosswise tests and `t/` test suite scripts pass without regressions against the baseline.
- **SC-003**: Code coverage for the ported Rust crates is at least 90%.

## Assumptions

- No new features are being added during the conversion; behavior must strictly mirror C Git (including intentional deviations documented in FOLLOWUPS.md).
- The C codebase version is fixed (e.g., v2.55.0-540) to prevent moving targets during conversion.
- All testing relies on the C binary as the oracle.
