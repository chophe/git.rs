//! Dependency-structure gate (`contracts/dependency-rules.md`).
//!
//! Reads every workspace member's `Cargo.toml`, extracts internal
//! (`git-*`/`xtask`) production edges from `[dependencies]`, and enforces:
//!   1. every internal dependency names a placed component (layer map below);
//!   2. edges point downward through the layers (surface → access → language
//!      → store → mid → foundation), with `git-command` as the sole
//!      composition-root exception and `xtask` exempt as automation;
//!   3. `git-cli` depends on `git-command` only;
//!   4. nothing depends on `xtask`;
//!   5. the graph is acyclic.
//!
//! Prints `EDGE a -> b` lines (the probe-command drill diffs these) and any
//! `VIOLATION` lines naming the offending change. Only std is used.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level under the workspace root")
        .to_path_buf()
}

/// Layer rank: lower number = lower layer. Edges must not go upward.
fn layer(name: &str) -> Option<u8> {
    match name {
        // foundation (0): no internal deps
        "git-hash" | "git-varint" | "git-date" => Some(0),
        // mid (1)
        "git-config" | "git-object" | "git-core" | "git-commitgraph" | "git-diff"
        | "git-merge" | "git-index" | "git-attributes" | "git-pretty" => Some(1),
        // store (2)
        "git-odb" | "git-refs" => Some(2),
        // language (3)
        "git-revision" | "git-pathspec" => Some(3),
        // access (4)
        "git-compress" | "git-worktree" | "git-transport" | "git-protocol" | "git-hooks"
        | "git-credentials" => Some(4),
        // surface (5)
        "git-command" | "git-cli" => Some(5),
        _ => None,
    }
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

/// Production (`[dependencies]`) internal edges of one member crate.
fn production_edges(root: &PathBuf, member: &str) -> Vec<String> {
    let path = root.join(member).join("Cargo.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut edges = Vec::new();
    let mut section = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            section = t.to_string();
            continue;
        }
        if section == "[dependencies]" {
            if let Some(eq) = t.find('=') {
                let key = t[..eq].trim();
                if (key.starts_with("git-") || key == "xtask")
                    && key.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    edges.push(key.to_string());
                }
            }
        }
    }
    edges.sort();
    edges.dedup();
    edges
}

pub fn run() -> bool {
    let root = workspace_root();
    let members = workspace_members(&root);
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for m in &members {
        edges.insert(m.clone(), production_edges(&root, m));
    }

    let mut ok = true;
    let mut edge_count = 0;
    let mut sorted_members = members.clone();
    sorted_members.sort();
    for m in &sorted_members {
        let deps = &edges[m.as_str()];
        for d in deps {
            println!("EDGE {m} -> {d}");
            edge_count += 1;
        }
    }

    // Rule 4: nothing depends on xtask.
    for m in &sorted_members {
        if edges[m.as_str()].iter().any(|d| d == "xtask") {
            println!("VIOLATION {m} depends on xtask (automation must not be a runtime dependency)");
            ok = false;
        }
    }

    // Rules 1-3: placement + direction.
    for m in &sorted_members {
        for d in &edges[m.as_str()] {
            if d == "xtask" {
                continue;
            }
            let (rl, rr) = match (layer(m), layer(d)) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    println!("VIOLATION {m} -> {d}: unplaced component (register it in contracts/placement.md)");
                    ok = false;
                    continue;
                }
            };
            if m == "git-command" || m == "xtask" {
                continue; // composition root / automation exemption
            }
            if m == "git-cli" && d != "git-command" {
                println!("VIOLATION git-cli -> {d}: git-cli may depend on git-command only (thin binary)");
                ok = false;
                continue;
            }
            if rl < rr {
                println!("VIOLATION {m} (layer {rl}) -> {d} (layer {rr}): upward edge, arrows must point downward");
                ok = false;
            }
        }
    }

    // Rule 5: acyclicity (DFS over members).
    let mut color: HashMap<&str, u8> = HashMap::new();
    for m in &members {
        color.insert(m.as_str(), 0);
    }
    fn visit<'a>(
        u: &'a str,
        edges: &'a HashMap<String, Vec<String>>,
        color: &mut HashMap<&'a str, u8>,
        stack: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        color.insert(u, 1);
        stack.push(u);
        if let Some(deps) = edges.get(u) {
            for d in deps {
                if d == "xtask" || !edges.contains_key(d) {
                    continue;
                }
                match color.get(d.as_str()) {
                    Some(1) => {
                        let mut cyc: Vec<String> = stack.iter().map(|s| s.to_string()).collect();
                        cyc.push(d.clone());
                        return Some(cyc);
                    }
                    Some(0) => {
                        if let Some(c) = visit(d, edges, color, stack) {
                            return Some(c);
                        }
                    }
                    _ => {}
                }
            }
        }
        stack.pop();
        color.insert(u, 2);
        None
    }
    for m in &members {
        if color[m.as_str()] == 0 {
            let mut stack = Vec::new();
            if let Some(cyc) = visit(m, &edges, &mut color, &mut stack) {
                println!("VIOLATION dependency cycle: {}", cyc.join(" -> "));
                ok = false;
            }
        }
    }

    // Warn on workspace members with no dependency section at all (likely new
    // scaffolds that have not declared anything yet — allowed, not a failure).
    let _ = edge_count;
    let member_set: HashSet<&str> = members.iter().map(|s| s.as_str()).collect();
    let _ = member_set;
    println!(
        "depcheck: {} members, {} production edges: {}",
        members.len(),
        edge_count,
        if ok { "PASS" } else { "FAIL" }
    );
    ok
}
