//! Placement drill (`contracts/placement.md`, SC-005).
//!
//! Parses the boundary table in `contracts/placement.md` and enforces:
//!   * every `FR-###` row names exactly one owner (no duplicate rows);
//!   * all 21 boundaries are present;
//!   * `(existing)` owners exist as workspace members (missing = VIOLATION);
//!   * `(reserved)` owners missing from the tree print as PENDING (the slot
//!     is documented but not yet scaffolded — not a failure, never silent);
//!   * present owners are members of `crates/Cargo.toml`;
//!   * the workspace still checks cleanly (`cargo check --workspace`).
//! Only std is used.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level under the workspace root")
        .to_path_buf()
}

struct Row {
    fr: String,
    owner: String,
    reserved: bool,
}

fn parse_placement(repo_root: &PathBuf) -> Vec<Row> {
    let path = repo_root.join("specs/014-rust-architecture/contracts/placement.md");
    let text = std::fs::read_to_string(&path).expect("read contracts/placement.md");
    let mut rows = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with("| FR-") {
            continue;
        }
        let cols: Vec<&str> = t.split('|').map(|c| c.trim()).collect();
        if cols.len() < 6 {
            continue;
        }
        let fr = cols[1].to_string();
        let owner_cell = cols[3];
        let owner = owner_cell
            .split('`')
            .nth(1)
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        if owner.is_empty() {
            continue;
        }
        let reserved = line.contains("(reserved)");
        rows.push(Row { fr, owner, reserved });
    }
    rows
}

fn workspace_members(root: &PathBuf) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("read workspace Cargo.toml");
    let mut members = Vec::new();
    let mut in_members = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("members") {
            in_members = true;
            continue;
        }
        if in_members {
            if t.starts_with(']') {
                break;
            }
            let name = t.trim_matches(|c| c == '"' || c == '\'' || c == ',' || c == ' ');
            if !name.is_empty() && !name.starts_with('#') && !name.starts_with('[') {
                members.push(name.to_string());
            }
        }
    }
    members
}

pub fn run() -> bool {
    let crates_root = workspace_root();
    let repo_root = crates_root.parent().expect("repo root").to_path_buf();
    let rows = parse_placement(&repo_root);
    let members = workspace_members(&crates_root);

    let mut ok = true;

    // Exactly one row per boundary: no duplicates, all 21 present.
    let mut seen = std::collections::HashSet::new();
    for r in &rows {
        if !seen.insert(r.fr.clone()) {
            println!("VIOLATION {} listed twice (exactly one owner per boundary)", r.fr);
            ok = false;
        }
    }
    if rows.len() != 21 {
        println!("VIOLATION placement table has {} rows, expected 21", rows.len());
        ok = false;
    }

    for r in &rows {
        let dir_exists = crates_root.join(&r.owner).join("Cargo.toml").exists();
        let is_member = members.iter().any(|m| m == &r.owner);
        if dir_exists && is_member {
            println!("OWNED {} -> {} (present, workspace member)", r.fr, r.owner);
        } else if r.reserved && !dir_exists {
            println!("PENDING {} -> {} (reserved slot, not yet scaffolded)", r.fr, r.owner);
        } else if !dir_exists {
            println!("VIOLATION {} -> {}: owner crate missing from crates/", r.fr, r.owner);
            ok = false;
        } else {
            println!("VIOLATION {} -> {}: crate exists but is not a workspace member", r.fr, r.owner);
            ok = false;
        }
    }

    // Present owners must still compile together.
    let status = Command::new("cargo")
        .args(["check", "--workspace", "--offline"])
        .current_dir(&crates_root)
        .status();
    match status {
        Ok(s) if s.success() => println!("placement: cargo check --workspace PASS"),
        Ok(s) => {
            println!("VIOLATION cargo check --workspace failed with {s} (neighbors do not compile)");
            ok = false;
        }
        Err(e) => {
            println!("VIOLATION could not run cargo check: {e}");
            ok = false;
        }
    }

    println!("placement: {} boundaries: {}", rows.len(), if ok { "PASS" } else { "FAIL" });
    ok
}
