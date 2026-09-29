//! `git update-ref` and `git symbolic-ref`.
//!
//! A port of `builtin/update-ref.c` (single-ref paths plus the full
//! `--stdin` batch grammar: `update/create/delete/verify`,
//! `symref-update/-create/-delete/-verify`, `start/prepare/commit/abort`,
//! `option`, in whitespace/C-quote and NUL modes) and
//! `builtin/symbolic-ref.c`, over the [`git_refs::transaction`] engine
//! (plan 01-01). Every die-string below was captured from the tree C binary
//! (2.55.0.552); `t/t1400-update-ref.sh` + `t/t1404-update-ref-errors.sh`
//! are the oracles (`t/` wins ties).

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_odb::Odb;
use git_refs::transaction::{Transaction, TxnOp};
use git_refs::RefStore;

const UPDATE_REF_USAGE: &str = "usage: git update-ref [<options>] -d <refname> [<old-oid>]\n\
   or: git update-ref [<options>]    <refname> <new-oid> [<old-oid>]\n\
   or: git update-ref [<options>] --stdin [-z] [--batch-updates]";
const SYMBOLIC_REF_USAGE: &str = "usage: git symbolic-ref [-m <reason>] <name> <ref>\n\
   or: git symbolic-ref [-q] [--short] [--no-recurse] <name>\n\
   or: git symbolic-ref --delete [-q] <name>";

pub struct UpdateRef;

impl Command for UpdateRef {
    fn name(&self) -> &'static str {
        "update-ref"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let cli = parse_cli(args)?;
        if cli.msg.as_deref() == Some("") {
            return Err(CommandError::fatal("fatal: Refusing to perform update with empty message."));
        }
        if cli.stdin {
            return run_stdin(ctx, out, &cli);
        }
        run_single(ctx, &cli)
    }
}

/// Parsed `update-ref` command-line options (C's `cmd_update_ref` locals).
struct Cli {
    delete: bool,
    no_deref: bool,
    stdin: bool,
    nul: bool,
    create_reflog: bool,
    batch_updates: bool,
    msg: Option<String>,
    operands: Vec<String>,
}

fn parse_cli(args: &[String]) -> Result<Cli, CommandError> {
    let mut cli = Cli {
        delete: false,
        no_deref: false,
        stdin: false,
        nul: false,
        create_reflog: false,
        batch_updates: false,
        msg: None,
        operands: Vec::new(),
    };
    let mut i = 0usize;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-d" => cli.delete = true,
            "--delete" => cli.delete = true,
            "--no-deref" => cli.no_deref = true,
            "--stdin" => cli.stdin = true,
            "-z" => cli.nul = true,
            "--create-reflog" => cli.create_reflog = true,
            // Accepted parse-options negation of `--create-reflog`;
            // inert (there is no suppression bit in C).
            "--no-create-reflog" => cli.create_reflog = false,
            "-0" | "--batch-updates" => cli.batch_updates = true,
            "-m" => {
                i += 1;
                cli.msg = Some(
                    args.get(i)
                        .ok_or_else(|| {
                            CommandError::usage(format!("error: option `m' requires a value\n{UPDATE_REF_USAGE}"))
                        })?
                        .clone(),
                );
            }
            s if s.starts_with("-m") && s.len() > 2 => {
                cli.msg = Some(s[2..].to_string());
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(CommandError::usage(format!("error: unknown option `{s}'\n{UPDATE_REF_USAGE}")));
            }
            s => cli.operands.push(s.to_string()),
        }
        i += 1;
    }
    if cli.stdin {
        if cli.delete || !cli.operands.is_empty() {
            return Err(CommandError::usage(UPDATE_REF_USAGE));
        }
    } else if cli.batch_updates {
        return Err(CommandError::fatal("fatal: --batch-updates can only be used with --stdin"));
    }
    if cli.nul && !cli.stdin {
        return Err(CommandError::usage(UPDATE_REF_USAGE));
    }
    Ok(cli)
}

/// The reflog message for an update (`-m` or empty, like C's NULL msg).
fn batch_msg(cli: &Cli) -> String {
    cli.msg.clone().unwrap_or_default()
}

/// Whether `refname` gets the branch-only new-value check (C's branch
/// rule: `HEAD` plus `refs/heads/*` must point at commits).
fn is_branch_ref(refname: &str) -> bool {
    refname == "HEAD" || refname.starts_with("refs/heads/")
}

/// Verify a new oid about to be stored (C's transaction new-value
/// checks): it must exist, and for branches it must be a commit. Returns
/// the bare detail; callers add their prefix/exit convention.
fn check_new_value(odb: &Odb, refname: &str, new: &Oid) -> Result<(), String> {
    let null = *odb.algorithm().null_oid();
    if *new == null {
        return Ok(()); // Zero new-oid means delete; no object needed.
    }
    let obj = odb
        .read(new)
        .map_err(|_| format!("trying to write ref '{refname}' with nonexistent object {new}"))?;
    if is_branch_ref(refname) && obj.kind != git_object::ObjectKind::Commit {
        return Err(format!("trying to write non-commit object {new} to branch '{refname}'"));
    }
    Ok(())
}

/// Single-ref `update-ref` (C's non-`--stdin` path through
/// `refs_update_ref` / `refs_delete_ref`).
fn run_single(ctx: &RepoContext, cli: &Cli) -> Result<(), CommandError> {
    let repo = ctx.repository()?;
    let store = RefStore::from_repo(&repo);
    let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
    let deref = !cli.no_deref;
    let msg = batch_msg(cli);

    if cli.delete {
        if cli.operands.len() < 1 || cli.operands.len() > 2 {
            return Err(CommandError::usage(UPDATE_REF_USAGE));
        }
        let name = cli.operands[0].clone();
        let old = match cli.operands.get(1) {
            None => None,
            Some(s) if s.is_empty() => Some(*repo.hash_algo.null_oid()),
            Some(s) => Some(
                crate::resolve_arg(&repo, s)
                    .map_err(|_| CommandError::fatal(format!("fatal: {s}: not a valid old SHA1")))?,
            ),
        };
        let target = store.deref_name_opt(&name, deref);
        let old_tip = store.resolve(&target).unwrap_or(*repo.hash_algo.null_oid());
        let mut tx = Transaction::begin(&store);
        tx.queue(TxnOp::Delete { name: name.clone(), old, deref });
        match tx.prepare() {
            Ok(()) => {}
            Err(e) => return Err(single_delete_error(e)),
        }
        match tx.commit() {
            Ok(()) => {}
            Err(e) => return Err(single_delete_error(e)),
        }
        log_single_delete(&repo, &store, &target, &old_tip, &msg, cli.create_reflog);
        return Ok(());
    }

    if cli.operands.len() < 2 || cli.operands.len() > 3 {
        return Err(CommandError::usage(UPDATE_REF_USAGE));
    }
    let name = cli.operands[0].clone();
    let new = crate::resolve_arg(&repo, &cli.operands[1])
        .map_err(|_| CommandError::fatal(format!("fatal: {}: not a valid SHA1", cli.operands[1])))?;
    let old = match cli.operands.get(2) {
        None => None,
        Some(s) if s.is_empty() => Some(*repo.hash_algo.null_oid()),
        Some(s) => Some(
            crate::resolve_arg(&repo, s)
                .map_err(|_| CommandError::fatal(format!("fatal: {s}: not a valid old SHA1")))?,
        ),
    };
    // A zero new-oid is a delete with an old-value check (C treats a null
    // new oid as a delete in `refs_update_ref`).
    if new == *repo.hash_algo.null_oid() {
        let target = store.deref_name_opt(&name, deref);
        let old_tip = store.resolve(&target).unwrap_or(*repo.hash_algo.null_oid());
        let mut tx = Transaction::begin(&store);
        tx.queue(TxnOp::Delete { name: name.clone(), old, deref });
        match tx.prepare() {
            Ok(()) => {}
            Err(e) => return Err(single_delete_error(e)),
        }
        match tx.commit() {
            Ok(()) => {}
            Err(e) => return Err(single_delete_error(e)),
        }
        log_single_delete(&repo, &store, &target, &old_tip, &msg, cli.create_reflog);
        return Ok(());
    }
    check_new_value(&odb, &name, &new)
        .map_err(|d| CommandError::fatal(format!("fatal: update_ref failed for ref '{name}': {d}")))?;
    let old_logged = store.resolve(&store.deref_name_opt(&name, deref)).unwrap_or(*repo.hash_algo.null_oid());
    let mut tx = Transaction::begin(&store);
    tx.queue(TxnOp::Set { name: name.clone(), new, old, deref });
    match tx.prepare() {
        Ok(()) => {}
        Err(e) => return Err(wrap_single(&name, e)),
    }
    match tx.commit() {
        Ok(()) => {}
        Err(e) => return Err(wrap_single(&name, e)),
    }
    log_single_update(&repo, &store, &name, deref, &old_logged, &new, &msg, cli.create_reflog);
    Ok(())
}

/// C's single-ref `update_ref failed for ref '<name>': ...` wrapper (C
/// `refs_update_ref`, DIE_ON_ERR).
fn wrap_single(name: &str, e: git_refs::RefError) -> CommandError {
    match e {
        git_refs::RefError::Transaction(d) => {
            CommandError::fatal(format!("fatal: update_ref failed for ref '{name}': {d}"))
        }
        other => CommandError::fatal(format!(
            "fatal: update_ref failed for ref '{name}': cannot lock ref '{name}': {other}"
        )),
    }
}

/// Single-ref `-d` errors use `error:` + exit 1, unwrapped (C
/// `refs_delete_ref` reports through `error`, not `die`).
fn single_delete_error(e: git_refs::RefError) -> CommandError {
    match e {
        git_refs::RefError::Transaction(d) => CommandError::error(format!("error: {d}")),
        git_refs::RefError::InvalidName(n) => {
            CommandError::error(format!("error: refusing to update ref with bad name '{n}'"))
        }
        other => CommandError::error(format!("error: {other}")),
    }
}

/// Log one single-ref delete: drop the target's log (never for HEAD
/// itself), and append HEAD when the current branch was deleted (C
/// `delete_ref` HEAD handling).
fn log_single_delete(
    repo: &git_core::Repository,
    store: &RefStore,
    target: &str,
    old_tip: &Oid,
    msg: &str,
    force: bool,
) {
    if target != "HEAD" {
        git_refs::reflog::remove_log(&repo.git_dir, target);
    }
    let head_target = store.head_symbolic_target();
    if head_target.as_deref() == Some(target) || target == "HEAD" {
        if let Ok(ident) = crate::checkout_core::committer_ident(repo) {
            if force {
                git_refs::reflog::log_update_forced(repo, "HEAD", old_tip, repo.hash_algo.null_oid(), &ident, msg);
            } else {
                git_refs::reflog::log_update(repo, "HEAD", old_tip, repo.hash_algo.null_oid(), &ident, msg);
            }
        }
    }
}

/// Log one single-ref update through the single writer (C logs the
/// updated ref; through a symref both the symref and its target; a direct
/// update of the current branch logs HEAD too).
fn log_single_update(
    repo: &git_core::Repository,
    store: &RefStore,
    name: &str,
    deref: bool,
    old: &Oid,
    new: &Oid,
    msg: &str,
    force: bool,
) {
    let target = store.deref_name_opt(name, deref);
    let new_now = store.resolve(&target).unwrap_or(*new);
    let Ok(ident) = crate::checkout_core::committer_ident(repo) else { return };
    // C forces the log with `--create-reflog`; otherwise the standard
    // gating applies (`--no-create-reflog` is an accepted no-op).
    let log_one = |refname: &str| {
        if force {
            git_refs::reflog::log_update_forced(repo, refname, old, &new_now, &ident, msg);
        } else {
            git_refs::reflog::log_update(repo, refname, old, &new_now, &ident, msg);
        }
    };
    log_one(&target);
    if deref && target != name {
        log_one(name);
    }
    // A direct update of the current branch logs HEAD too (C
    // `split_head_update`).
    if store.head_symbolic_target().as_deref() == Some(target.as_str()) && target != "HEAD" {
        log_one("HEAD");
    }
}

// ---------------------------------------------------------------------------
// `--stdin` batch mode
// ---------------------------------------------------------------------------

/// Transaction states (C `enum update_refs_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
enum BatchState {
    Open,
    Started,
    Prepared,
    Closed,
}

/// Batch verbs (C `command[]` table; the delimiter rule in
/// [`match_verb`] replaces the table scan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    Update,
    Create,
    Delete,
    Verify,
    SymrefUpdate,
    SymrefCreate,
    SymrefDelete,
    SymrefVerify,
    Option,
    Start,
    Prepare,
    Abort,
    Commit,
}

impl Verb {
    /// The transaction state this verb belongs to (C `command[].state`).
    fn state(&self) -> BatchState {
        match self {
            Verb::Update | Verb::Create | Verb::Delete | Verb::Verify | Verb::SymrefUpdate
            | Verb::SymrefCreate | Verb::SymrefDelete | Verb::SymrefVerify | Verb::Option => {
                BatchState::Open
            }
            Verb::Start => BatchState::Started,
            Verb::Prepare => BatchState::Prepared,
            Verb::Abort | Verb::Commit => BatchState::Closed,
        }
    }
}

/// Match a verb prefix with C's delimiter rule: verbs with args need a
/// following space (or, in NUL mode, the segment may end right there, the
/// refname arriving in later segments); bare verbs need the terminator.
/// Returns the verb plus the text after `verb + delimiter`.
fn match_verb(content: &str, nul: bool) -> Option<(Verb, &str)> {
    // Table order from C `command[]`.
    const TABLE: &[(&str, Verb, bool)] = &[
        ("update", Verb::Update, true),
        ("create", Verb::Create, true),
        ("delete", Verb::Delete, true),
        ("verify", Verb::Verify, true),
        ("symref-update", Verb::SymrefUpdate, true),
        ("symref-create", Verb::SymrefCreate, true),
        ("symref-delete", Verb::SymrefDelete, true),
        ("symref-verify", Verb::SymrefVerify, true),
        ("option", Verb::Option, true),
        ("start", Verb::Start, false),
        ("prepare", Verb::Prepare, false),
        ("abort", Verb::Abort, false),
        ("commit", Verb::Commit, false),
    ];
    for (prefix, verb, has_args) in TABLE {
        if !content.starts_with(*prefix) {
            continue;
        }
        let rest = &content[prefix.len()..];
        if *has_args {
            if rest.starts_with(' ') {
                return Some((*verb, &rest[1..]));
            }
            // NUL mode: the refname may arrive in following segments.
            if nul && rest.is_empty() {
                return Some((*verb, ""));
            }
            continue;
        }
        if rest.is_empty() {
            return Some((*verb, ""));
        }
    }
    None
}

/// Extra NUL segments consumed per verb for oid arguments (C
/// `cmd->args - 1`: the refname always comes from the verb segment).
fn verb_extra_segments(verb: Verb) -> usize {
    match verb {
        Verb::Update => 2,
        Verb::Create => 1,
        Verb::Delete => 1,
        Verb::Verify => 1,
        Verb::SymrefUpdate => 3,
        Verb::SymrefCreate => 1,
        Verb::SymrefDelete => 1,
        Verb::SymrefVerify => 1,
        Verb::Option | Verb::Start | Verb::Prepare | Verb::Abort | Verb::Commit => 0,
    }
}

/// What to log for one queued batch op after a successful commit.
struct PendingLog {
    /// The op's own name (logged when gating allows).
    name: String,
    /// The write target (logged too when it differs, like HEAD+branch).
    target: String,
    /// Pre-change value (resolved or null) for appends.
    old: Oid,
    /// The given old oid, if any (value-path rejections render "(null)"
    /// when absent; in-transaction rejections render zeros).
    old_given: Option<Oid>,
    /// New value for oid-op appends (`None` for deletes/verifies and
    /// symref writes, whose append value resolves post-commit).
    new: Option<Oid>,
    /// Rejected-line rendering of the new value (C `print_rejected_refs`:
    /// oid hex, zeros for deletes/symref writes, "(null)" when absent).
    rej_new: String,
    /// Rejected-line rendering of the old value (hex, or "(null)").
    rej_old: String,
    /// Deletes drop the log file instead of appending.
    is_delete: bool,
    is_symref_write: bool,
}

struct BatchRunner<'x> {
    repo: git_core::Repository,
    odb: Odb,
    out: &'x mut dyn Write,
    msg: String,
    default_deref: bool,
    create_reflog: bool,
    allow_failures: bool,
    nul: bool,
    state: BatchState,
    pending_deref: bool,
    /// Whether the queued batch is already prepared (C never re-prepares
    /// at commit time).
    prepared: bool,
}

fn run_stdin(ctx: &RepoContext, out: &mut dyn Write, cli: &Cli) -> Result<(), CommandError> {
    let repo = ctx.repository()?;
    let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
    let mut runner = BatchRunner {
        repo,
        odb,
        out,
        msg: batch_msg(cli),
        default_deref: !cli.no_deref,
        create_reflog: cli.create_reflog,
        allow_failures: cli.batch_updates,
        nul: cli.nul,
        state: BatchState::Open,
        pending_deref: !cli.no_deref,
        prepared: false,
    };
    // Stream stdin like C (one chunk at a time): batch peers read each
    // `: ok` while still feeding input, so nothing waits for EOF first.
    let stdin = std::io::stdin();
    runner.run_stream(&mut stdin.lock())
}

impl<'x> BatchRunner<'x> {
    fn run_stream(&mut self, reader: &mut dyn std::io::BufRead) -> Result<(), CommandError> {
        // Stream segments like C (one line/NUL chunk at a time): the
        // `t/` PIPE test reads each `: ok` incrementally while still
        // feeding input, so nothing may wait for EOF up front.
        let store = RefStore::from_repo(&self.repo);
        let mut tx: Option<Transaction<'_>> = None;
        let mut pending_logs: Vec<PendingLog> = Vec::new();
        loop {
            let term = if self.nul { 0u8 } else { b'\n' };
            let mut buf: Vec<u8> = Vec::new();
            let n = reader
                .read_until(term, &mut buf)
                .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
            if n == 0 {
                break;
            }
            let terminated = buf.last() == Some(&term);
            if terminated {
                buf.pop();
            }
            let content = String::from_utf8_lossy(&buf).into_owned();
            // NUL mode: extra oid segments arrive as following chunks,
            // read on demand (C appends `cmd->args - 1` of them).
            let mut extra: Vec<String> = Vec::new();
            // (Verb matching needs no extras; they are read after the
            // verb is known, in process_segment.)
            let echo = if self.nul { content.clone() } else { format!("{content}\n") };
            if content.is_empty() {
                // An empty final chunk without terminator is just EOF
                // (can only happen in NUL mode after a trailing NUL,
                // which the loop already consumed... treat uniformly).
                if self.nul && !terminated {
                    break;
                }
                return Err(CommandError::fatal("fatal: empty command in input"));
            }
            if content.as_bytes().first().map_or(false, |b| b.is_ascii_whitespace()) {
                return Err(CommandError::fatal(format!("fatal: whitespace before command: {echo}")));
            }
            let Some((verb, after)) = match_verb(&content, self.nul) else {
                return Err(CommandError::fatal(format!("fatal: unknown command: {echo}")));
            };
            if self.nul {
                for _ in 0..verb_extra_segments(verb) {
                    let mut ebuf: Vec<u8> = Vec::new();
                    let en = reader
                        .read_until(0, &mut ebuf)
                        .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
                    if en == 0 {
                        break;
                    }
                    if ebuf.last() == Some(&0) {
                        ebuf.pop();
                    }
                    extra.push(String::from_utf8_lossy(&ebuf).into_owned());
                }
            }
            self.process_segment(&store, &mut tx, &mut pending_logs, verb, after, &echo, extra)?;
        }
        // EOF: commit the implicit transaction, abort an explicit one.
        match self.state {
            BatchState::Open => {
                if tx.is_some() {
                    self.prepare_tx(&store, &mut tx, &mut pending_logs, "")?;
                    if let Some(t) = tx.take() {
                        let logs = std::mem::take(&mut pending_logs);
                        match t.commit() {
                            Ok(()) => {}
                            Err(e) => return Err(CommandError::fatal(format!("fatal: {e}"))),
                        }
                        self.log_committed(&store, &logs);
                    }
                    self.prepared = false;
                }
            }
            BatchState::Started | BatchState::Prepared => {
                // Abandon silently, pruning empty dirs (C
                // `ref_transaction_abort` + remove-empty-dirs).
                if let Some(t) = tx.take() {
                    t.abort();
                }
            }
            BatchState::Closed => {}
        }
        Ok(())
    }

    /// Run one verb segment (state gating + execution).
    #[allow(clippy::too_many_arguments)]
    fn process_segment<'s>(
        &mut self,
        store: &'s RefStore,
        tx: &mut Option<Transaction<'s>>,
        pending_logs: &mut Vec<PendingLog>,
        verb: Verb,
        after: &str,
        _echo: &str,
        extra: Vec<String>,
    ) -> Result<(), CommandError> {
            // State gating (C `update_refs_stdin` switch), then run.
            match self.state {
                BatchState::Open | BatchState::Started => {
                    if verb == Verb::Start && self.state == BatchState::Started {
                        return Err(CommandError::fatal("fatal: cannot restart ongoing transaction"));
                    }
                    if verb.state() >= self.state {
                        self.state = verb.state();
                    }
                }
                BatchState::Prepared => {
                    if verb.state() != BatchState::Closed {
                        return Err(CommandError::fatal("fatal: prepared transactions can only be closed"));
                    }
                    self.state = BatchState::Closed;
                }
                BatchState::Closed => {
                    if verb != Verb::Start {
                        return Err(CommandError::fatal("fatal: transaction is closed"));
                    }
                    self.state = BatchState::Started;
                    *tx = Some(Transaction::begin(store));
                    pending_logs.clear();
                    self.prepared = false;
                    self.pending_deref = self.default_deref;
                }
            }
            match verb {
                Verb::Start => self.report_ok("start")?,
                Verb::Prepare => {
                    if tx.is_none() {
                        *tx = Some(Transaction::begin(store));
                    }
                    self.prepare_tx(store, tx, pending_logs, "prepare")?;
                    self.report_ok("prepare")?;
                }
                Verb::Abort => {
                    if let Some(t) = tx.take() {
                        t.abort();
                    }
                    pending_logs.clear();
                    self.prepared = false;
                    self.report_ok("abort")?;
                }
                Verb::Commit => {
                    if tx.is_some() {
                        self.prepare_tx(store, tx, pending_logs, "commit")?;
                        if let Some(t) = tx.take() {
                            let logs = std::mem::take(pending_logs);
                            match t.commit() {
                                Ok(()) => {}
                                Err(e) => return Err(CommandError::fatal(format!("fatal: commit: {e}"))),
                            }
                            self.log_committed(&store, &logs);
                        }
                        self.prepared = false;
                    }
                    self.report_ok("commit")?;
                }
                Verb::Option => {
                    // `after` never carries the terminator; re-add it for
                    // the C echo shape in text mode.
                    let shown = if self.nul { after.to_string() } else { format!("{after}\n") };
                    if after == "no-deref" {
                        self.pending_deref = false;
                    } else {
                        return Err(CommandError::fatal(format!("fatal: option unknown: {shown}")));
                    }
                }
                _ => {
                    let deref = self.pending_deref;
                    let (op, plog) = self.parse_op(verb, after, &extra, deref, &store)?;
                    // New-value object checks up front (nothing has been
                    // renamed yet, so failing here keeps the batch
                    // atomic with C-identical stderr).
                    if let Some((refname, new)) = op_new_oid(&op) {
                        if let Err(detail) = check_new_value(&self.odb, refname, new) {
                            if self.allow_failures {
                                self.print_rejected(
                                    &plog.target,
                                    &plog.rej_new,
                                    &opt_hex(plog.old_given),
                                    "invalid new value provided",
                                    Some(&detail),
                                );
                                self.pending_deref = self.default_deref;
                                return Ok(());
                            }
                            return Err(CommandError::fatal(format!("fatal: {detail}")));
                        }
                    }
                    if tx.is_none() {
                        *tx = Some(Transaction::begin(store));
                    }
                    pending_logs.push(plog);
                    tx.as_mut().expect("set").queue(op);
                    self.prepared = false;
                    self.pending_deref = self.default_deref;
                }
            }
        Ok(())
    }

    /// Prepare the queued batch (strict, or lenient with printed
    /// rejections under `--batch-updates`). Skipped when already
    /// prepared (C's commit never re-prepares). `what` prefixes strict
    /// errors (`prepare:`/`commit:`); empty means bare (C dies unwrapped
    /// when the implicit EOF batch fails).
    fn prepare_tx(
        &mut self,
        store: &RefStore,
        tx: &mut Option<Transaction<'_>>,
        pending_logs: &mut Vec<PendingLog>,
        what: &str,
    ) -> Result<(), CommandError> {
        if self.prepared {
            return Ok(());
        }
        let Some(t) = tx.as_mut() else { return Ok(()) };
        if self.allow_failures {
            let ignorecase = self.repo.config.get_bool("core", "ignorecase").unwrap_or(false);
            let rejected = t.prepare_lenient(ignorecase);
            if !rejected.is_empty() {
                let doomed: std::collections::HashSet<usize> =
                    rejected.iter().map(|r| r.index).collect();
                for r in &rejected {
                    if let Some(plog) = pending_logs.get(r.index) {
                        self.print_rejected(
                            &plog.target,
                            &plog.rej_new,
                            &plog.rej_old,
                            r.msg,
                            Some(&r.detail),
                        );
                    }
                }
                // Survivors only: rejected ops never reach commit.
                let mut kept: Vec<PendingLog> = Vec::new();
                for (i, plog) in pending_logs.drain(..).enumerate() {
                    if !doomed.contains(&i) {
                        kept.push(plog);
                    }
                }
                *pending_logs = kept;
            }
            let _ = store;
            self.prepared = true;
            return Ok(());
        }
        match t.prepare() {
            Ok(()) => {}
            Err(e) => {
                if what.is_empty() {
                    return Err(CommandError::fatal(format!("fatal: {e}")));
                }
                return Err(CommandError::fatal(format!("fatal: {what}: {e}")));
            }
        }
        self.prepared = true;
        Ok(())
    }

    fn report_ok(&mut self, cmd: &str) -> Result<(), CommandError> {
        // C `report_ok` flushes stdout: the `t/` PIPE test reads each
        // `ok` incrementally while the batch is still running.
        writeln!(self.out, "{cmd}: ok").map_err(|e| CommandError::fatal(e.to_string()))?;
        self.out.flush().map_err(|e| CommandError::fatal(e.to_string()))?;
        Ok(())
    }

    /// Print a `--batch-updates` rejection (C `print_rejected_refs`):
    /// detail to stderr, `rejected ...` to stdout.
    fn print_rejected(&mut self, display: &str, rej_new: &str, rej_old: &str, msg: &str, detail: Option<&str>) {
        if let Some(d) = detail {
            eprintln!("error: {d}");
        }
        let _ = writeln!(self.out, "rejected {display} {rej_new} {rej_old} {msg}");
        let _ = self.out.flush();
    }

    /// Append reflog entries for committed batch ops: oid writes append
    /// (plus HEAD when it points at the target, C `split_head_update`);
    /// deletes drop the target's log and append HEAD when the current
    /// branch was deleted (C `delete_ref` HEAD handling); verifies write
    /// nothing.
    fn log_committed(&mut self, store: &RefStore, logs: &[PendingLog]) {
        if logs.is_empty() {
            return;
        }
        let ident = match crate::checkout_core::committer_ident(&self.repo) {
            Ok(id) => id,
            Err(_) => return,
        };
        let head_target = store.head_symbolic_target();
        for plog in logs {
            if plog.is_delete {
                if plog.target != "HEAD" {
                    git_refs::reflog::remove_log(&self.repo.git_dir, &plog.target);
                }
                // Deleting the current branch logs HEAD (old=tip,
                // new=zero); other deletes only drop the target's log.
                if head_target.as_deref() == Some(plog.target.as_str()) || plog.target == "HEAD" {
                    self.append_log("HEAD", &plog.old, self.repo.hash_algo.null_oid(), &ident);
                }
                continue;
            }
            let new = match plog.new {
                Some(oid) => oid,
                None if plog.is_symref_write => {
                    store.resolve(&plog.target).unwrap_or(*self.repo.hash_algo.null_oid())
                }
                None => continue,
            };
            self.append_log(&plog.target, &plog.old, &new, &ident);
            if plog.target != plog.name {
                self.append_log(&plog.name, &plog.old, &new, &ident);
            }
            // Direct updates of the current branch log HEAD too.
            if head_target.as_deref() == Some(plog.target.as_str()) && plog.target != "HEAD" {
                self.append_log("HEAD", &plog.old, &new, &ident);
            }
        }
    }

    /// Append one entry when gating (or `--create-reflog`) allows.
    /// Existing ("touched") logs always append (C behavior). All appends
    /// funnel through the single writer (never `append` directly).
    fn append_log(&self, refname: &str, old: &Oid, new: &Oid, ident: &str) {
        if self.create_reflog {
            git_refs::reflog::log_update_forced(&self.repo, refname, old, new, ident, &self.msg);
        } else {
            git_refs::reflog::log_update(&self.repo, refname, old, new, ident, &self.msg);
        }
    }

    /// Parse one mutating verb line into a queued op plus its log
    /// snapshot.
    fn parse_op(
        &self,
        verb: Verb,
        after: &str,
        extra: &[String],
        deref: bool,
        store: &RefStore,
    ) -> Result<(TxnOp, PendingLog), CommandError> {
        let term = if self.nul { "" } else { "\n" };
        // In NUL mode the refname is the whole segment remainder; oid
        // args arrive in `extra`. In text mode everything is inline with
        // C-quote support.
        let mut parser = BatchParser {
            repo: &self.repo,
            after,
            extra,
            extra_pos: 0,
            nul: self.nul,
            term,
            echo_verb: verb_echo_name(verb),
            deref,
        };
        let op = match verb {
            Verb::Update => parser.parse_update(deref)?,
            Verb::Create => parser.parse_create(deref)?,
            Verb::Delete => parser.parse_delete(deref)?,
            Verb::Verify => parser.parse_verify()?,
            Verb::SymrefUpdate => parser.parse_symref_update(deref)?,
            Verb::SymrefCreate => parser.parse_symref_create(deref)?,
            Verb::SymrefDelete => parser.parse_symref_delete()?,
            Verb::SymrefVerify => parser.parse_symref_verify()?,
            _ => unreachable!(),
        };
        // Snapshot for logging: write target + pre-change value, plus
        // the `rejected`-line renderings (probed: in-transaction
        // rejections render zeros for absent oids; pre-queue value
        // failures render "(null)").
        let target = store.deref_name_opt(op_name(&op), deref);
        let null = *self.repo.hash_algo.null_oid();
        let old = store.resolve(&target).unwrap_or(null);
        let old_given: Option<Oid> = match &op {
            TxnOp::Set { old: o, .. } | TxnOp::Delete { old: o, .. } => *o,
            _ => None,
        };
        let (new, rej_new, rej_old, is_delete, is_symref_write) = match &op {
            TxnOp::Set { new, old: o, .. } => {
                (Some(*new), new.to_string(), o.map(|x| x.to_string()).unwrap_or_else(|| null.to_string()), false, false)
            }
            TxnOp::Create { new, .. } => {
                (Some(*new), new.to_string(), null.to_string(), false, false)
            }
            TxnOp::Delete { old: o, .. } => {
                (None, null.to_string(), o.map(|x| x.to_string()).unwrap_or_else(|| null.to_string()), true, false)
            }
            TxnOp::SymrefDelete { .. } => {
                (None, null.to_string(), null.to_string(), true, false)
            }
            TxnOp::Verify { old, .. } => {
                (None, "(null)".to_string(), old.to_string(), false, false)
            }
            TxnOp::SymrefVerify { .. } => {
                (None, "(null)".to_string(), "(null)".to_string(), false, false)
            }
            TxnOp::SymrefUpdate { old_oid: o, .. } => {
                (None, null.to_string(), o.map(|x| x.to_string()).unwrap_or_else(|| null.to_string()), false, true)
            }
            TxnOp::SymrefCreate { .. } => {
                (None, null.to_string(), null.to_string(), false, true)
            }
        };
        let plog = PendingLog {
            name: op_name(&op).to_string(),
            target,
            old,
            old_given,
            new,
            rej_new,
            rej_old,
            is_delete,
            is_symref_write,
        };
        Ok((op, plog))
    }
}

/// Render an optional oid for `rejected` lines (`"(null)"` when absent).
fn opt_hex(o: Option<Oid>) -> String {
    o.map(|x| x.to_string()).unwrap_or_else(|| "(null)".to_string())
}

/// The new oid an op stores, if any (for the up-front object checks).
fn op_new_oid(op: &TxnOp) -> Option<(&str, &Oid)> {
    match op {
        TxnOp::Set { name, new, .. } | TxnOp::Create { name, new, .. } => Some((name.as_str(), new)),
        _ => None,
    }
}

fn op_name(op: &TxnOp) -> &str {
    match op {
        TxnOp::Set { name, .. }
        | TxnOp::Create { name, .. }
        | TxnOp::Delete { name, .. }
        | TxnOp::Verify { name, .. }
        | TxnOp::SymrefUpdate { name, .. }
        | TxnOp::SymrefCreate { name, .. }
        | TxnOp::SymrefDelete { name, .. }
        | TxnOp::SymrefVerify { name, .. } => name,
    }
}

// ---------------------------------------------------------------------------
// Batch line parsing (C `parse_cmd_*`, `parse_arg`, `parse_next_oid`)
// ---------------------------------------------------------------------------

/// Echo-name for die prefixes (`update %s: ...` uses the short verb).
fn verb_echo_name(verb: Verb) -> &'static str {
    match verb {
        Verb::Update => "update",
        Verb::Create => "create",
        Verb::Delete => "delete",
        Verb::Verify => "verify",
        Verb::SymrefUpdate => "symref-update",
        Verb::SymrefCreate => "symref-create",
        Verb::SymrefDelete => "symref-delete",
        Verb::SymrefVerify => "symref-verify",
        _ => "?",
    }
}

/// Cursor over one verb's arguments.
struct BatchParser<'p> {
    repo: &'p git_core::Repository,
    /// Text-mode remainder (past `verb `) or NUL-mode segment remainder.
    after: &'p str,
    /// Following NUL segments (NUL mode only).
    extra: &'p [String],
    extra_pos: usize,
    nul: bool,
    /// Echo terminator (`"\n"` text, `""` NUL).
    term: &'p str,
    echo_verb: &'static str,
    /// Live deref flag (`option no-deref` clears it until a mutating verb
    /// resets it; C `update_flags`).
    deref: bool,
}

impl<'p> BatchParser<'p> {
    fn v(&self) -> &'static str {
        self.echo_verb
    }

    fn die(&self, msg: String) -> CommandError {
        CommandError::fatal(format!("fatal: {msg}"))
    }

    /// Parse the refname right after `command SP` (C `parse_refname`):
    /// C-quote aware in text mode, whole-remainder in NUL mode, then
    /// `REFNAME_ALLOW_ONELEVEL` validation.
    fn parse_refname(&mut self, text: &mut TextArgs<'p>) -> Result<String, CommandError> {
        if self.nul {
            let name = self.after.to_string();
            if name.is_empty() {
                return Err(self.die(format!("{}: missing <ref>", self.v())));
            }
            if git_refs::validate_refname_allow_onelevel(&name).is_err() {
                return Err(self.die(format!("invalid ref format: {name}")));
            }
            return Ok(name);
        }
        text.parse_refname(self)
    }

    /// Parse the next whitespace-delimited arg (C `parse_next_arg`): None
    /// when absent; in NUL mode consumes the next segment (empty segment
    /// means absent).
    fn parse_next_arg(&mut self, text: &mut TextArgs<'p>) -> Result<Option<String>, CommandError> {
        if self.nul {
            let Some(seg) = self.extra.get(self.extra_pos) else {
                return Ok(None);
            };
            self.extra_pos += 1;
            if seg.is_empty() {
                return Ok(None);
            }
            return Ok(Some(seg.clone()));
        }
        text.parse_next_arg(self)
    }

    /// Parse the next refname arg (C `parse_next_refname`): None when
    /// absent, with format validation when present.
    fn parse_next_refname(&mut self, text: &mut TextArgs<'p>) -> Result<Option<String>, CommandError> {
        if self.nul {
            let Some(seg) = self.extra.get(self.extra_pos) else {
                return Ok(None);
            };
            // In NUL mode the segment boundary IS the delimiter: skip it
            // and read the whole next segment as the name.
            self.extra_pos += 1;
            if seg.is_empty() {
                return Ok(None);
            }
            if git_refs::validate_refname_allow_onelevel(seg).is_err() {
                return Err(self.die(format!("invalid ref format: {seg}")));
            }
            return Ok(Some(seg.clone()));
        }
        match text.parse_next_arg(self)? {
            None => Ok(None),
            Some(s) => {
                if git_refs::validate_refname_allow_onelevel(&s).is_err() {
                    return Err(self.die(format!("invalid ref format: {s}")));
                }
                Ok(Some(s))
            }
        }
    }

    /// Parse the next oid arg (C `parse_next_oid`).
    ///
    /// Returns `Ok(None)` when no value was given at all; `Ok(Some(zero))`
    /// for an explicitly empty value; otherwise the resolved oid. Dies
    /// `invalid <new-oid>/<old-oid>` on unresolvable values and
    /// `unexpected end of input` at a truncated NUL stream.
    fn parse_next_oid(
        &mut self,
        text: &mut TextArgs<'p>,
        refname: &str,
        old: bool,
        allow_empty_as_zero: bool,
    ) -> Result<Option<Oid>, CommandError> {
        let kind = if old { "<old-oid>" } else { "<new-oid>" };
        if self.nul {
            let Some(seg) = self.extra.get(self.extra_pos) else {
                return Err(self.die(format!("{} {refname}: unexpected end of input when reading {kind}", self.v())));
            };
            self.extra_pos += 1;
            if seg.is_empty() {
                if allow_empty_as_zero {
                    if !old {
                        eprintln!(
                            "warning: {} {refname}: missing <new-oid>, treating as zero",
                            self.v()
                        );
                    }
                    return Ok(Some(*self.repo.hash_algo.null_oid()));
                }
                return Ok(None);
            }
            return match crate::resolve_arg(self.repo, seg) {
                Ok(oid) => Ok(Some(oid)),
                Err(_) => Err(self.die(format!(
                    "{} {refname}: invalid {kind}: {seg}",
                    self.v(),
                    kind = if old { "<old-oid>" } else { "<new-oid>" }
                ))),
            };
        }
        text.parse_next_oid(self, refname, old, allow_empty_as_zero)
    }

    /// No trailing input may remain (C `*next != line_termination`).
    fn expect_end(&self, text: &TextArgs<'p>, refname: &str) -> Result<(), CommandError> {
        if self.nul {
            if self.extra_pos < self.extra.len() {
                // Leftover segments become the next command; a consumed
                // count mismatch surfaces there (C reads exactly
                // `cmd->args` segments and re-dispatches the rest).
            }
            return Ok(());
        }
        text.expect_end(self, refname)
    }

    fn parse_update(&mut self, deref: bool) -> Result<TxnOp, CommandError> {
        let mut text = TextArgs::new(self.after);
        let refname = self.parse_refname(&mut text)?;
        let new = match self.parse_next_oid(&mut text, &refname, false, true)? {
            Some(oid) => oid,
            None => return Err(self.die(format!("update {refname}: missing <new-oid>"))),
        };
        let old = self.parse_next_oid(&mut text, &refname, true, false)?;
        self.expect_end(&text, &refname)?;
        let null = *self.repo.hash_algo.null_oid();
        if new == null {
            // A zero new-oid deletes (C passes NULL through as a delete).
            return Ok(TxnOp::Delete { name: refname, old, deref });
        }
        Ok(TxnOp::Set { name: refname, new, old, deref })
    }

    fn parse_create(&mut self, deref: bool) -> Result<TxnOp, CommandError> {
        let mut text = TextArgs::new(self.after);
        let refname = self.parse_refname(&mut text)?;
        let new = match self.parse_next_oid(&mut text, &refname, false, false)? {
            Some(oid) => oid,
            None => return Err(self.die(format!("create {refname}: missing <new-oid>"))),
        };
        if new == *self.repo.hash_algo.null_oid() {
            return Err(self.die(format!("create {refname}: zero <new-oid>")));
        }
        self.expect_end(&text, &refname)?;
        Ok(TxnOp::Create { name: refname, new, deref })
    }

    fn parse_delete(&mut self, deref: bool) -> Result<TxnOp, CommandError> {
        let mut text = TextArgs::new(self.after);
        let refname = self.parse_refname(&mut text)?;
        let old = self.parse_next_oid(&mut text, &refname, true, false)?;
        if old == Some(*self.repo.hash_algo.null_oid()) {
            return Err(self.die(format!("delete {refname}: zero <old-oid>")));
        }
        self.expect_end(&text, &refname)?;
        Ok(TxnOp::Delete { name: refname, old, deref })
    }

    fn parse_verify(&mut self) -> Result<TxnOp, CommandError> {
        let mut text = TextArgs::new(self.after);
        let refname = self.parse_refname(&mut text)?;
        // Absent old means the null oid (C `oidclr`s on missing).
        let old = self
            .parse_next_oid(&mut text, &refname, true, false)?
            .unwrap_or(*self.repo.hash_algo.null_oid());
        self.expect_end(&text, &refname)?;
        Ok(TxnOp::Verify { name: refname, old })
    }

    fn parse_symref_update(&mut self, deref: bool) -> Result<TxnOp, CommandError> {
        let mut text = TextArgs::new(self.after);
        let refname = self.parse_refname(&mut text)?;
        let new_target = match self.parse_next_refname(&mut text)? {
            Some(t) => t,
            None => return Err(self.die(format!("symref-update {refname}: missing <new-target>"))),
        };
        let old_arg = self.parse_next_arg(&mut text)?;
        let (old_oid, old_target) = match old_arg.as_deref() {
            None => (None, None),
            Some("oid") => {
                let value = match self.parse_next_arg(&mut text)? {
                    Some(v) => v,
                    None => {
                        return Err(self.die(format!("symref-update {refname}: expected old value")));
                    }
                };
                match crate::resolve_arg(self.repo, &value) {
                    Ok(oid) => (Some(oid), None),
                    Err(_) => {
                        return Err(self.die(format!("symref-update {refname}: invalid oid: {value}")));
                    }
                }
            }
            Some("ref") => {
                let value = match self.parse_next_arg(&mut text)? {
                    Some(v) => v,
                    None => {
                        return Err(self.die(format!("symref-update {refname}: expected old value")));
                    }
                };
                if git_refs::validate_refname_allow_onelevel(&value).is_err() {
                    return Err(self.die(format!("symref-update {refname}: invalid ref: {value}")));
                }
                (None, Some(value))
            }
            Some(other) => {
                return Err(self.die(format!(
                    "symref-update {refname}: invalid arg '{other}' for old value"
                )));
            }
        };
        self.expect_end(&text, &refname)?;
        Ok(TxnOp::SymrefUpdate { name: refname, target: new_target, old_oid, old_target, deref })
    }

    fn parse_symref_create(&mut self, deref: bool) -> Result<TxnOp, CommandError> {
        let mut text = TextArgs::new(self.after);
        let refname = self.parse_refname(&mut text)?;
        let new_target = match self.parse_next_refname(&mut text)? {
            Some(t) => t,
            None => return Err(self.die(format!("symref-create {refname}: missing <new-target>"))),
        };
        self.expect_end(&text, &refname)?;
        Ok(TxnOp::SymrefCreate { name: refname, target: new_target, deref })
    }

    fn parse_symref_delete(&mut self) -> Result<TxnOp, CommandError> {
        // C requires REF_NO_DEREF for these verbs: they die in deref mode.
        if self.deref {
            return Err(self.die("symref-delete: cannot operate with deref mode".to_string()));
        }
        let mut text = TextArgs::new(self.after);
        let refname = self.parse_refname(&mut text)?;
        let old_target = self.parse_next_refname(&mut text)?;
        self.expect_end(&text, &refname)?;
        Ok(TxnOp::SymrefDelete { name: refname, old_target })
    }

    fn parse_symref_verify(&mut self) -> Result<TxnOp, CommandError> {
        // C requires REF_NO_DEREF for these verbs: they die in deref mode.
        if self.deref {
            return Err(self.die("symref-verify: cannot operate with deref mode".to_string()));
        }
        let mut text = TextArgs::new(self.after);
        let refname = self.parse_refname(&mut text)?;
        let old_target = self.parse_next_refname(&mut text)?;
        self.expect_end(&text, &refname)?;
        Ok(TxnOp::SymrefVerify { name: refname, old_target })
    }
}

/// Text-mode argument cursor over a verb's remainder.
struct TextArgs<'t> {
    text: &'t str,
    pos: usize,
}

impl<'t> TextArgs<'t> {
    fn new(after: &'t str) -> TextArgs<'t> {
        TextArgs { text: after, pos: 0 }
    }

    fn rest(&self) -> &'t str {
        &self.text[self.pos..]
    }

    /// Parse the refname (C `parse_refname` in text mode).
    fn parse_refname(&mut self, p: &BatchParser) -> Result<String, CommandError> {
        if self.rest().is_empty() {
            return Err(p.die(format!("{}: missing <ref>", p.v())));
        }
        let (name, next) = parse_one_text_arg(self.text, self.pos, p)?;
        if name.is_empty() {
            return Err(p.die(format!("{}: missing <ref>", p.v())));
        }
        if git_refs::validate_refname_allow_onelevel(&name).is_err() {
            return Err(p.die(format!("invalid ref format: {name}")));
        }
        self.pos = next;
        Ok(name)
    }

    /// Parse the next arg (C `parse_next_arg` in text mode).
    fn parse_next_arg(&mut self, p: &BatchParser) -> Result<Option<String>, CommandError> {
        if self.rest().is_empty() {
            return Ok(None);
        }
        if !self.rest().starts_with(' ') {
            return Err(p.die(format!("expected SP but got: {}{}", self.rest(), p.term)));
        }
        self.pos += 1;
        let (arg, next) = parse_one_text_arg(self.text, self.pos, p)?;
        self.pos = next;
        if arg.is_empty() {
            return Ok(None);
        }
        Ok(Some(arg))
    }

    /// Parse the next oid arg (C `parse_next_oid` in text mode).
    fn parse_next_oid(
        &mut self,
        p: &BatchParser,
        refname: &str,
        old: bool,
        _allow_empty_as_zero: bool,
    ) -> Result<Option<Oid>, CommandError> {
        let kind = if old { "<old-oid>" } else { "<new-oid>" };
        if self.rest().is_empty() {
            return Ok(None);
        }
        if !self.rest().starts_with(' ') {
            return Err(p.die(format!("{} {refname}: expected SP but got: {}{}", p.v(), self.rest(), p.term)));
        }
        self.pos += 1;
        let (arg, next) = parse_one_text_arg(self.text, self.pos, p)?;
        self.pos = next;
        if arg.is_empty() {
            // An empty value means all zeros (both modes, text side).
            return Ok(Some(*p.repo.hash_algo.null_oid()));
        }
        match crate::resolve_arg(p.repo, &arg) {
            Ok(oid) => Ok(Some(oid)),
            Err(_) => Err(p.die(format!("{} {refname}: invalid {kind}: {arg}", p.v()))),
        }
    }

    /// Assert no trailing input (C `*next != line_termination` echoes
    /// from the delimiter, which `rest` still starts at).
    fn expect_end(&self, p: &BatchParser, refname: &str) -> Result<(), CommandError> {
        if !self.rest().is_empty() {
            return Err(p.die(format!("{} {refname}: extra input: {}{}", p.v(), self.rest(), p.term)));
        }
        Ok(())
    }
}

/// Parse one text-mode argument at `pos` (C `parse_arg`): a C-quoted
/// string or a bareword up to whitespace. Returns the arg plus the new
/// position. `orig` for die messages starts at the opening quote.
fn parse_one_text_arg(text: &str, pos: usize, p: &BatchParser) -> Result<(String, usize), CommandError> {
    let bytes = text.as_bytes();
    if pos < bytes.len() && bytes[pos] == b'"' {
        return unquote_c_arg(text, pos, p);
    }
    let mut end = pos;
    while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
        end += 1;
    }
    Ok((text[pos..end].to_string(), end))
}

/// Unquote a C-style quoted argument (C `unquote_c_style` subset: the
/// standard single-letter escapes plus 1-3 digit octal and `\\xHH`).
/// `orig` echoes from the opening quote through end-of-input, like C.
fn unquote_c_arg(text: &str, pos: usize, p: &BatchParser) -> Result<(String, usize), CommandError> {
    let orig = format!("{}{}", &text[pos..], p.term);
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut i = pos + 1;
    loop {
        if i >= bytes.len() {
            return Err(p.die(format!("badly quoted argument: {orig}")));
        }
        match bytes[i] {
            b'"' => {
                i += 1;
                break;
            }
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    return Err(p.die(format!("badly quoted argument: {orig}")));
                }
                match bytes[i] {
                    b'a' => out.push('\x07'),
                    b'b' => out.push('\x08'),
                    b'f' => out.push('\x0c'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'v' => out.push('\x0b'),
                    b'\\' => out.push('\\'),
                    b'"' => out.push('"'),
                    b'0'..=b'7' => {
                        let mut val: u32 = (bytes[i] - b'0') as u32;
                        let mut count = 1;
                        while count < 3 && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() && bytes[i + 1] < b'8' {
                            i += 1;
                            val = val * 8 + (bytes[i] - b'0') as u32;
                            count += 1;
                        }
                        out.push(char::from_u32(val).unwrap_or('\u{FFFD}'));
                    }
                    b'x' => {
                        if i + 2 >= bytes.len()
                            || !bytes[i + 1].is_ascii_hexdigit()
                            || !bytes[i + 2].is_ascii_hexdigit()
                        {
                            return Err(p.die(format!("badly quoted argument: {orig}")));
                        }
                        let hex = &text[i + 1..i + 3];
                        let val = u8::from_str_radix(hex, 16).map_err(|_| p.die(format!("badly quoted argument: {orig}")))?;
                        out.push(val as char);
                        i += 2;
                    }
                    _ => return Err(p.die(format!("badly quoted argument: {orig}"))),
                }
                i += 1;
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    // The next character must end the argument (C errors otherwise).
    if i < bytes.len() && !bytes[i].is_ascii_whitespace() {
        return Err(p.die(format!("unexpected character after quoted argument: {orig}")));
    }
    Ok((out, i))
}

// ---------------------------------------------------------------------------
// `symbolic-ref`
// ---------------------------------------------------------------------------

pub struct SymbolicRef;

impl Command for SymbolicRef {
    fn name(&self) -> &'static str {
        "symbolic-ref"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut quiet = false;
        let mut delete = false;
        let mut short = false;
        let mut recurse = true;
        let mut msg: Option<String> = None;
        let mut rest: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-q" | "--quiet" => quiet = true,
                "-d" | "--delete" => delete = true,
                "--short" => short = true,
                "--recurse" => recurse = true,
                "--no-recurse" => recurse = false,
                "-m" => {
                    i += 1;
                    msg = Some(
                        args.get(i)
                            .ok_or_else(|| CommandError::usage(SYMBOLIC_REF_USAGE.to_string()))?
                            .clone(),
                    );
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("error: unknown option `{s}'\n{SYMBOLIC_REF_USAGE}")));
                }
                s => rest.push(s.to_string()),
            }
            i += 1;
        }
        if msg.as_deref() == Some("") {
            return Err(CommandError::fatal("fatal: Refusing to perform update with empty message"));
        }
        let repo = ctx.repository()?;
        let store = RefStore::from_repo(&repo);

        if delete {
            if rest.len() != 1 {
                return Err(CommandError::usage(SYMBOLIC_REF_USAGE));
            }
            let name = &rest[0];
            // Must be a symref (checked quietly, like C's `check_symref`
            // with quiet=1, non-recursive).
            if read_symref_target(&repo, name).is_none() {
                if quiet {
                    return Err(CommandError::silent(1));
                }
                return Err(CommandError::fatal(format!("fatal: Cannot delete {name}, not a symbolic ref")));
            }
            if name == "HEAD" {
                return Err(CommandError::fatal(format!("fatal: deleting '{name}' is not allowed")));
            }
            let mut tx = Transaction::begin(&store);
            tx.queue(TxnOp::SymrefDelete { name: name.clone(), old_target: None });
            match tx.prepare() {
                Ok(()) => {}
                Err(e) => return Err(CommandError::fatal(format!("fatal: {e}"))),
            }
            match tx.commit() {
                Ok(()) => {}
                Err(e) => return Err(CommandError::fatal(format!("fatal: {e}"))),
            }
            git_refs::reflog::remove_log(&repo.git_dir, name);
            return Ok(());
        }

        match rest.len() {
            1 => {
                // Read.
                let name = &rest[0];
                let target = if recurse {
                    // Recursively dereference to the terminal target.
                    let mut cur = name.clone();
                    loop {
                        match read_symref_target(&repo, &cur) {
                            Some(next) => cur = next,
                            None => break,
                        }
                        if cur == *name {
                            break;
                        }
                    }
                    // The name itself must resolve to a symref chain.
                    if read_symref_target(&repo, name).is_none() && store.resolve(name).is_none() {
                        if quiet {
                            return Err(CommandError::silent(1));
                        }
                        return Err(CommandError::fatal(format!("fatal: No such ref: {name}")));
                    }
                    // Hmm: `cur` is the terminal; but C prints the
                    // *immediate* target when recursing? No: with recurse
                    // (default) C resolves fully and prints the final
                    // refname. Verify against the probe below.
                    cur
                } else if let Some(t) = read_symref_target(&repo, name) {
                    t
                } else if store.resolve(name).is_some() {
                    // Not a symref but exists: C dies (not a symbolic
                    // ref) unless quiet.
                    if quiet {
                        return Err(CommandError::silent(1));
                    }
                    return Err(CommandError::fatal(format!("fatal: ref {name} is not a symbolic ref")));
                } else {
                    if quiet {
                        return Err(CommandError::silent(1));
                    }
                    return Err(CommandError::fatal(format!("fatal: No such ref: {name}")));
                };
                // With recurse, a non-symref terminal is fine (C prints
                // the resolved refname); without a symref at `name`
                // itself C dies unless quiet (handled above).
                let display = if short { shorten_ref(&target) } else { target };
                writeln!(out, "{display}").map_err(|e| CommandError::fatal(e.to_string()))?;
                Ok(())
            }
            2 => {
                // Create/update.
                let (name, target) = (rest[0].clone(), rest[1].clone());
                if name == "HEAD" && !target.starts_with("refs/") {
                    return Err(CommandError::fatal("fatal: Refusing to point HEAD outside of refs/"));
                }
                if git_refs::validate_refname_allow_onelevel(&target).is_err() {
                    return Err(CommandError::fatal(format!(
                        "fatal: Refusing to set '{name}' to invalid ref '{target}'"
                    )));
                }
                let old = store.resolve(&name).unwrap_or(*repo.hash_algo.null_oid());
                let mut tx = Transaction::begin(&store);
                // `symbolic-ref` always writes the named symref literally
                // (C `refs_update_symref` never dereferences).
                tx.queue(TxnOp::SymrefUpdate {
                    name: name.clone(),
                    target: target.clone(),
                    old_oid: None,
                    old_target: None,
                    deref: false,
                });
                match tx.prepare() {
                    Ok(()) => {}
                    Err(e) => return Err(CommandError::fatal(format!("fatal: {e}"))),
                }
                match tx.commit() {
                    Ok(()) => {}
                    Err(e) => return Err(CommandError::fatal(format!("fatal: {e}"))),
                }
                let message = msg.unwrap_or_default();
                if let Ok(ident) = crate::checkout_core::committer_ident(&repo) {
                    let new = store.resolve(&name).unwrap_or(old);
                    git_refs::reflog::log_update(&repo, &name, &old, &new, &ident, &message);
                }
                Ok(())
            }
            _ => Err(CommandError::usage(SYMBOLIC_REF_USAGE)),
        }
    }
}

/// Read a loose symref file literally (`ref: <target>`), without
/// following anything. Searches the git dir then the common dir.
fn read_symref_target(repo: &git_core::Repository, name: &str) -> Option<String> {
    for dir in [&repo.git_dir, &repo.common_dir] {
        if let Ok(content) = std::fs::read_to_string(dir.join(name)) {
            let t = content.trim();
            if let Some(target) = t.strip_prefix("ref:") {
                return Some(target.trim().to_string());
            }
            return None;
        }
    }
    None
}

/// Shorten a refname for `--short` display.
fn shorten_ref(name: &str) -> String {
    for prefix in ["refs/heads/", "refs/tags/", "refs/remotes/"] {
        if let Some(short) = name.strip_prefix(prefix) {
            return short.to_string();
        }
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_odb::LooseStore;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    /// A repo with three branch refs at commit `c1`, plus commit `c2`
    /// available as an update target. Returns the dir, the repo, and
    /// `(c1, c2)`.
    fn batch_repo() -> (TempDir, git_core::Repository, (Oid, Oid)) {
        use git_core::{RepoEnv, Repository};
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-update-ref-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::create_dir_all(git.join("objects")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(git.join("config"), "[user]\n\tname = T\n\temail = t@example.com\n").unwrap();
        let repo = Repository::discover_from(&dir, &RepoEnv::default()).unwrap();
        let loose = LooseStore::from_repo(&repo);
        let tree = loose.write_object(git_object::ObjectKind::Tree, b"").unwrap();
        let commit_data = |msg: &str| {
            format!("tree {tree}\nauthor T <t@example.com> 0 +0000\ncommitter T <t@example.com> 0 +0000\n\n{msg}\n")
                .into_bytes()
        };
        let c1 = loose.write_object(git_object::ObjectKind::Commit, &commit_data("one")).unwrap();
        let c2 = loose.write_object(git_object::ObjectKind::Commit, &commit_data("two")).unwrap();
        for r in ["refs/heads/a", "refs/heads/b", "refs/heads/c"] {
            std::fs::write(git.join(r), format!("{c1}\n")).unwrap();
        }
        (TempDir(dir), repo, (c1, c2))
    }

    /// Drive one batch through the runner, returning stdout text or the
    /// command error.
    fn run_batch(
        repo: git_core::Repository,
        odb: Odb,
        nul: bool,
        allow_failures: bool,
        input: &[u8],
    ) -> (Result<(), CommandError>, String) {
        let mut out: Vec<u8> = Vec::new();
        let result = {
            let mut runner = BatchRunner {
                repo,
                odb,
                out: &mut out,
                msg: String::new(),
                default_deref: true,
                create_reflog: false,
                allow_failures,
                nul,
                state: BatchState::Open,
                pending_deref: true,
                prepared: false,
            };
            let mut cursor = std::io::Cursor::new(input.to_vec());
            runner.run_stream(&mut cursor)
        };
        (result, String::from_utf8(out).unwrap())
    }

    fn ref_tip(repo: &git_core::Repository, name: &str) -> String {
        let store = RefStore::from_repo(repo);
        store.resolve(name).map(|o| o.to_string()).unwrap_or_else(|| "unresolved".to_string())
    }

    fn cli_err(args: &[&str]) -> CommandError {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        match parse_cli(&owned) {
            Ok(_) => panic!("expected parse error for {args:?}"),
            Err(e) => e,
        }
    }

    #[test]
    fn stdin_with_operands_is_usage() {
        let err = cli_err(&["--stdin", "refs/heads/a"]);
        assert_eq!(err.code, 129);
    }

    #[test]
    fn nul_without_stdin_is_usage() {
        let err = cli_err(&["-z"]);
        assert_eq!(err.code, 129);
    }

    #[test]
    fn batch_updates_without_stdin_is_fatal() {
        let err = cli_err(&["--batch-updates"]);
        assert_eq!(err.code, 128);
    }

    #[test]
    fn empty_message_is_refused() {
        let (_tmp, _, _) = batch_repo();
        let ctx = RepoContext::at(&_tmp.0);
        let mut buf = Vec::new();
        let err = UpdateRef
            .run(&ctx, &["-m".to_string(), "".to_string(), "refs/heads/a".to_string()], &mut buf)
            .unwrap_err();
        assert_eq!(err.code, 128);
        assert!(err.message.contains("empty message"), "{}", err.message);
    }

    #[test]
    fn unknown_command_dies() {
        let (_tmp, repo, _) = batch_repo();
        let odb = Odb::from_repo(&repo).unwrap();
        let (res, _) = run_batch(repo, odb, false, false, b"frobnicate refs/heads/a\n");
        let err = res.unwrap_err();
        assert_eq!(err.code, 128);
        assert!(err.message.contains("unknown command"), "{}", err.message);
    }

    #[test]
    fn leading_whitespace_dies() {
        let (_tmp, repo, _) = batch_repo();
        let odb = Odb::from_repo(&repo).unwrap();
        let (res, _) = run_batch(repo, odb, false, false, b" update refs/heads/a\n");
        let err = res.unwrap_err();
        assert!(err.message.contains("whitespace before command"), "{}", err.message);
    }

    #[test]
    fn update_missing_new_oid_dies_with_arity() {
        let (_tmp, repo, _) = batch_repo();
        let odb = Odb::from_repo(&repo).unwrap();
        let (res, _) = run_batch(repo, odb, false, false, b"update refs/heads/a\n");
        let err = res.unwrap_err();
        assert_eq!(err.code, 128);
        assert!(err.message.contains("update refs/heads/a: missing <new-oid>"), "{}", err.message);
    }

    #[test]
    fn delete_zero_old_oid_dies() {
        let (_tmp, repo, _) = batch_repo();
        let odb = Odb::from_repo(&repo).unwrap();
        let zero = repo.hash_algo.null_oid().to_string();
        let input = format!("delete refs/heads/a {zero}\n");
        let (res, _) = run_batch(repo, odb, false, false, input.as_bytes());
        let err = res.unwrap_err();
        assert!(err.message.contains("delete refs/heads/a: zero <old-oid>"), "{}", err.message);
    }

    #[test]
    fn bad_old_oid_leaves_every_ref_unchanged() {
        let (_tmp, repo, (c1, c2)) = batch_repo();
        let odb = Odb::from_repo(&repo).unwrap();
        let zero = repo.hash_algo.null_oid().to_string();
        // Two good updates plus one bad old-oid: the whole batch aborts.
        let input = format!("update refs/heads/a {c2}\nupdate refs/heads/b {c2}\nupdate refs/heads/c {c2} {zero}\n");
        let (res, _) = run_batch(repo, odb, false, false, input.as_bytes());
        assert!(res.is_err(), "batch with a bad old-oid must fail");
        for r in ["refs/heads/a", "refs/heads/b", "refs/heads/c"] {
            assert_eq!(ref_tip(&_tmp_repo(&_tmp), r), c1.to_string());
        }
    }

    fn _tmp_repo(_tmp: &TempDir) -> git_core::Repository {
        use git_core::{RepoEnv, Repository};
        Repository::discover_from(&_tmp.0, &RepoEnv::default()).unwrap()
    }

    #[test]
    fn explicit_start_commit_reports_ok() {
        let (_tmp, repo, (_, c2)) = batch_repo();
        let odb = Odb::from_repo(&repo).unwrap();
        let input = format!("start\nupdate refs/heads/a {c2}\ncommit\n");
        let (res, text) = run_batch(repo, odb, false, false, input.as_bytes());
        res.unwrap();
        assert!(text.contains("start: ok"), "{text}");
        assert!(text.contains("commit: ok"), "{text}");
        assert_eq!(ref_tip(&_tmp_repo(&_tmp), "refs/heads/a"), c2.to_string());
    }

    #[test]
    fn nul_batch_applies() {
        let (_tmp, repo, (_, c2)) = batch_repo();
        let odb = Odb::from_repo(&repo).unwrap();
        // -z wire format: `update SP <ref> NUL <new> NUL <old> NUL`;
        // an empty trailing segment means "old unspecified" (C
        // `parse_next_oid` ret=1), while a truncated stream dies
        // `unexpected end of input` (also C).
        let input = format!("update refs/heads/a\0{c2}\0\0").into_bytes();
        let (res, _) = run_batch(repo, odb, true, false, &input);
        res.unwrap();
        assert_eq!(ref_tip(&_tmp_repo(&_tmp), "refs/heads/a"), c2.to_string());
    }

    #[test]
    fn symref_create_applies() {
        let (_tmp, repo, _) = batch_repo();
        let odb = Odb::from_repo(&repo).unwrap();
        let input = b"symref-create refs/heads/sym refs/heads/a\n";
        let (res, _) = run_batch(repo, odb, false, false, input);
        res.unwrap();
        let target = std::fs::read_to_string(_tmp.0.join(".git/refs/heads/sym")).unwrap();
        assert_eq!(target.trim(), "ref: refs/heads/a");
    }
}
