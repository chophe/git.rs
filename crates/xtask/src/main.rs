//! `cargo xtask` — one-command automation for the git.rs port.
//!
//! Subcommands:
//!   test           run the full unit + property suite (`cargo test --workspace`)
//!   differential   run the crosswise suites against the system C git
//!   gen-fixtures   (re)generate the golden fixtures under `tests/fixtures`
//!   scoreboard     run the differential suites and update `scoreboard.json`,
//!                  failing on regression against the committed baseline

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level under the workspace root")
        .to_path_buf()
}

fn cargo(args: &[&str]) -> bool {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(workspace_root())
        .status()
        .expect("cargo runs");
    status.success()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let ok = match cmd {
        "test" => cargo(&["test", "--workspace"]),
        "differential" => differential(),
        "gen-fixtures" => gen_fixtures(),
        "scoreboard" => scoreboard(),
        "help" | "-h" | "--help" => {
            print_help();
            true
        }
        other => {
            eprintln!("xtask: unknown command '{other}'");
            print_help();
            false
        }
    };
    if !ok {
        std::process::exit(1);
    }
}

fn print_help() {
    println!(
        "usage: cargo xtask <command>\n\n\
         commands:\n  \
         test           run the full workspace test suite\n  \
         differential   run crosswise suites against system git\n  \
         gen-fixtures   regenerate tests/fixtures\n  \
         scoreboard     run differential suites and update scoreboard.json"
    );
}

/// The crosswise suites, each keyed by its cargo invocation.
fn suites() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        (
            "pack-crosswise",
            &["test", "-p", "git-odb", "--test", "pack_crosswise"],
        ),
        (
            "graph-midx-crosswise",
            &["test", "-p", "git-odb", "--test", "graph_midx_crosswise"],
        ),
        (
            "phase4-crosswise",
            &["test", "-p", "git-command", "--test", "phase4_crosswise"],
        ),
        (
            "phase5-crosswise",
            &["test", "-p", "git-command", "--test", "phase5_crosswise"],
        ),
        (
            "phase6-crosswise",
            &["test", "-p", "git-command", "--test", "phase6_crosswise"],
        ),
        (
            "phase7-crosswise",
            &["test", "-p", "git-command", "--test", "phase7_crosswise"],
        ),
        (
            "phase8-crosswise",
            &["test", "-p", "git-command", "--test", "phase8_crosswise"],
        ),
        (
            "phase9-crosswise",
            &["test", "-p", "git-command", "--test", "phase9_crosswise"],
        ),
        (
            "phase10-crosswise",
            &["test", "-p", "git-command", "--test", "phase10_crosswise"],
        ),
        (
            "followups-crosswise",
            &["test", "-p", "git-command", "--test", "followups_crosswise"],
        ),
        (
            "phaseA02-crosswise",
            &["test", "-p", "git-command", "--test", "phaseA02_crosswise"],
        ),
        (
            "phaseA01-crosswise",
            &["test", "-p", "git-command", "--test", "phaseA01_crosswise"],
        ),
        (
            "phaseA03-crosswise",
            &["test", "-p", "git-command", "--test", "phaseA03_crosswise"],
        ),
        (
            "phaseA04-crosswise",
            &["test", "-p", "git-command", "--test", "phaseA04_crosswise"],
        ),
    ]
}

fn differential() -> bool {
    suites()
        .iter()
        .map(|(name, args)| {
            let ok = cargo(args);
            println!("differential suite '{name}': {}", if ok { "PASS" } else { "FAIL" });
            ok
        })
        .all(|ok| ok)
}

fn scoreboard() -> bool {
    let results: Vec<(String, bool)> = suites()
        .iter()
        .map(|(name, args)| (name.to_string(), cargo(args)))
        .collect();

    // Load the previous baseline, if any.
    let baseline_path = workspace_root().join("scoreboard.json");
    let previous = serde_lite::load(&baseline_path).unwrap_or(serde_lite::Value::Unknown);

    let mut regressions = 0;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut lines = Vec::new();
    lines.push(format!("{{\"timestamp\": {now}, \"suites\": ["));
    let mut first = true;
    for (name, ok) in &results {
        let prev_ok = previous.suite(name);
        if prev_ok == Some(true) && !ok {
            regressions += 1;
        }
        if !first {
            lines.push(",".to_string());
        }
        lines.push(format!("{{\"name\": \"{name}\", \"pass\": {ok}}}"));
        first = false;
    }
    lines.push("]}".to_string());
    let json = lines.join("");

    std::fs::write(&baseline_path, &json).expect("write scoreboard.json");
    println!("wrote {}", baseline_path.display());
    if regressions > 0 {
        eprintln!("scoreboard: {regressions} regression(s) against the baseline");
        return false;
    }
    results.iter().all(|(_, ok)| *ok)
}

/// Generate golden fixtures with the pinned system git.
fn gen_fixtures() -> bool {
    let fixtures = workspace_root().join("tests/fixtures");
    let _ = std::fs::remove_dir_all(&fixtures);
    std::fs::create_dir_all(&fixtures).expect("create fixtures dir");

    std::fs::write(
        fixtures.join("README.md"),
        "# Golden fixtures\n\n\
         Generated by `cargo xtask gen-fixtures` using the system C git.\n\
         Regenerate with: `cargo xtask gen-fixtures`.\n",
    )
    .expect("write README");

    let git = git_binary();
    if git.is_none() {
        eprintln!("gen-fixtures: no system git available; wrote empty fixtures");
        return true;
    }
    let git = git.unwrap();

    // Build a small repo and repack it into a golden pack.
    let dir = workspace_root().join("tests/fixtures/repo");
    std::fs::create_dir_all(&dir).expect("create repo dir");
    for (args, _) in [
        (vec!["init", "-q"], 0),
        (vec!["config", "user.name", "Fixtures"], 0),
        (vec!["config", "user.email", "f@example.com"], 0),
    ] {
        run_ok(git, &dir, &args);
    }
    for i in 0..3 {
        std::fs::write(dir.join("f"), format!("line {i}\n")).unwrap();
        run_ok(git, &dir, &["add", "-A"]);
        run_ok(git, &dir, &["commit", "-qm", &format!("c{i}")]);
    }
    run_ok(git, &dir, &["repack", "-ad"]);
    run_ok(git, &dir, &["multi-pack-index", "write"]);
    run_ok(git, &dir, &["commit-graph", "write", "--reachable"]);

    // Checksums.
    let mut checksums = String::new();
    for e in std::fs::read_dir(&fixtures).unwrap().flatten() {
        let p = e.path();
        if p.is_file() && p.extension().and_then(|x| x.to_str()) != Some("checksums") {
            let data = std::fs::read(&p).unwrap();
            let digest = sha1_hex(&data);
            let rel = p.strip_prefix(&fixtures).unwrap().display();
            checksums.push_str(&format!("{digest}  {rel}\n"));
        }
    }
    std::fs::write(fixtures.join(".checksums"), checksums).expect("write checksums");
    println!("generated fixtures under {}", fixtures.display());
    true
}

fn git_binary() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn run_ok(git: &str, dir: &Path, args: &[&str]) -> bool {
    Command::new(git)
        .args(args)
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn sha1_hex(data: &[u8]) -> String {
    let mut h = git_sha1();
    h.update(data);
    h.finalize()
}

/// Minimal SHA-1 for fixture checksums (avoids a dependency).
struct GitSha1 {
    state: [u32; 5],
    buf: [u8; 64],
    buflen: usize,
    total: u64,
}

fn git_sha1() -> GitSha1 {
    GitSha1 {
        state: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0],
        buf: [0; 64],
        buflen: 0,
        total: 0,
    }
}

impl GitSha1 {
    fn update(&mut self, mut data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buflen > 0 {
            let take = std::cmp::min(64 - self.buflen, data.len());
            self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
            self.buflen += take;
            data = &data[take..];
            if self.buflen == 64 {
                let block = self.buf;
                self.process(&block);
                self.buflen = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.process(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buflen = data.len();
        }
    }

    fn finalize(mut self) -> String {
        let bit_len = self.total.wrapping_mul(8);
        self.buf[self.buflen] = 0x80;
        self.buflen += 1;
        for b in &mut self.buf[self.buflen..] {
            *b = 0;
        }
        if self.buflen > 56 {
            let block = self.buf;
            self.process(&block);
            let mut fresh = [0u8; 64];
            fresh[56..64].copy_from_slice(&bit_len.to_be_bytes());
            self.process(&fresh);
        } else {
            self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
            let block = self.buf;
            self.process(&block);
        }
        let mut out = String::with_capacity(40);
        for word in self.state {
            out.push_str(&format!("{:08x}", word));
        }
        out
    }

    fn process(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = self.state;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

/// Minimal JSON value reader for the committed baseline (parses just the
/// `suites` array we write).
mod serde_lite {
    pub enum Value {
        Suites(Vec<(String, bool)>),
        Unknown,
    }

    impl Value {
        pub fn suite(&self, name: &str) -> Option<bool> {
            match self {
                Value::Suites(list) => list.iter().find(|(n, _)| n == name).map(|(_, ok)| *ok),
                Value::Unknown => None,
            }
        }
    }

    pub fn load(path: &std::path::Path) -> Option<Value> {
        let text = std::fs::read_to_string(path).ok()?;
        let idx = text.find("\"suites\"")?;
        let rest = &text[idx..];
        let open = rest.find('[')?;
        let mut list = Vec::new();
        for part in rest[open + 1..].split('{').skip(1) {
            let name = field(part, "name")?;
            let pass = field(part, "pass")?;
            list.push((name, pass == "true"));
        }
        Some(Value::Suites(list))
    }

    fn field<'a>(s: &'a str, key: &str) -> Option<String> {
        let pat = format!("\"{key}\": ");
        let i = s.find(&pat)? + pat.len();
        let rest = &s[i..];
        let end = rest.find([',', '}'])?;
        Some(rest[..end].trim_matches('"').to_string())
    }
}
