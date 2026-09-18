use std::collections::{HashMap, HashSet};
use std::io::Write;

use crate::patch::{self, BlobSource};
use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_object::{parse_commit, parse_tag, parse_tree, ObjectKind};
use git_odb::Odb;
use git_pretty::{CommitInfo, Format, Options};
use git_revision::Resolver;

pub struct Show;

#[derive(Clone)]
struct ShowOptions {
    format: Format,
    pretty: Options,
    oneline: bool,
    patch: bool,
    output: Option<String>,
    context: usize,
    no_renames: bool,
    first_parent: bool,
    paths: Vec<String>,
}

impl Command for Show {
    fn name(&self) -> &'static str {
        "show"
    }

    fn run(
        &self,
        ctx: &RepoContext,
        args: &[String],
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let mut opts = ShowOptions {
            format: Format::Medium,
            pretty: Options::default(),
            oneline: false,
            patch: true,
            output: None,
            context: 3,
            no_renames: false,
            first_parent: false,
            paths: Vec::new(),
        };
        let mut names = Vec::new();
        let mut dashdash = false;
        let mut explicit_patch = false;
        for arg in args {
            if dashdash {
                if arg.contains(['*', '?', '[']) || arg.starts_with(':') || arg.contains("..") {
                    return Err(unsupported("non-literal pathspecs"));
                }
                opts.paths.push(arg.clone());
                continue;
            }
            match arg.as_str() {
                "--" => dashdash = true,
                "--oneline" => {
                    opts.format = Format::UserTerminated("%h %s".into());
                    opts.oneline = true;
                }
                "--pretty" | "--format" => {
                    opts.format = Format::Medium;
                    opts.oneline = false;
                }
                s if s.starts_with("--pretty=") || s.starts_with("--format=") => {
                    let spec = s.split_once('=').unwrap().1;
                    opts.format = if spec.is_empty() {
                        Format::User(String::new())
                    } else {
                        Format::parse(spec)
                            .or_else(|| {
                                spec.contains('%')
                                    .then(|| Format::UserTerminated(spec.into()))
                            })
                            .ok_or_else(|| {
                                CommandError::fatal(format!(
                                    "fatal: invalid --pretty format: {spec}"
                                ))
                            })?
                    };
                    opts.oneline = matches!(opts.format, Format::Oneline);
                }
                "-s" | "--no-patch" => {
                    opts.patch = false;
                    opts.output = None;
                    explicit_patch = false;
                }
                "-p" | "--patch" | "-u" => {
                    opts.patch = true;
                    explicit_patch = true;
                }
                "--patch-with-stat" => {
                    opts.patch = true;
                    explicit_patch = true;
                    opts.output = Some("--stat".into());
                }
                "--stat" | "--shortstat" | "--numstat" | "--name-only" | "--name-status" => {
                    if opts.output.as_deref().is_some_and(|old| old != arg) {
                        return Err(unsupported("multiple diff summary formats"));
                    }
                    opts.output = Some(arg.clone());
                    opts.patch = explicit_patch;
                }
                "--no-renames" => opts.no_renames = true,
                "--first-parent" => opts.first_parent = true,
                "--no-color" | "--color=never" | "--color=auto" | "--no-decorate"
                | "--decorate=no" | "--no-ext-diff" | "--no-textconv" | "--no-walk"
                | "--no-walk=unsorted" => {}
                s if s.starts_with("--date=") => {
                    let spec = &s[7..];
                    opts.pretty.date =
                        git_pretty::date::DateMode::parse(spec).ok_or_else(|| {
                            CommandError::fatal(format!("fatal: invalid date format: {spec}"))
                        })?;
                }
                s if s.starts_with("-U") || s.starts_with("--unified=") => {
                    let value = s.strip_prefix("--unified=").unwrap_or(&s[2..]);
                    opts.context = value
                        .parse()
                        .map_err(|_| unsupported("invalid unified context"))?;
                    opts.patch = true;
                    explicit_patch = true;
                }
                s if s.starts_with('-') => return Err(unsupported(s)),
                s if s.starts_with('^') || s.split(':').next().unwrap_or(s).contains("..") => {
                    return Err(unsupported("revision walks and ranges"));
                }
                _ => names.push(arg.clone()),
            }
        }
        if opts.patch
            && matches!(
                opts.output.as_deref(),
                Some("--name-only" | "--name-status")
            )
        {
            opts.patch = false;
        }
        if opts.patch && matches!(opts.output.as_deref(), Some("--shortstat" | "--numstat")) {
            return Err(unsupported("patch combined with this summary format"));
        }
        if names.is_empty() {
            names.push("HEAD".into());
        }
        let repo = ctx.repository()?;
        if !opts.paths.is_empty() {
            if repo
                .work_tree
                .as_ref()
                .is_some_and(|root| ctx.cwd.canonicalize().ok() != root.canonicalize().ok())
            {
                return Err(unsupported("pathspecs from a repository subdirectory"));
            }
            if opts.paths.iter().any(|p| p == "." || p.is_empty()) {
                opts.paths.clear();
            } else {
                for path in &mut opts.paths {
                    *path = path.strip_prefix("./").unwrap_or(path).to_string();
                }
            }
        }
        let odb = Odb::from_repo(&repo)?;
        let resolver = Resolver::new(&repo)?;
        let mut pending = Vec::new();
        let mut seen = HashSet::new();
        for name in names {
            let oid = resolve_object(&resolver, &odb, &name, dashdash)?;
            if seen.insert(oid) {
                pending.push((name, oid));
            }
        }
        let mut shown = false;
        for (name, mut oid) in pending {
            let mut tags = HashSet::new();
            loop {
                let object = odb
                    .read(&oid)
                    .map_err(|_| CommandError::fatal(format!("fatal: bad object {name}")))?;
                match object.kind {
                    ObjectKind::Blob => {
                        out.write_all(&object.data).map_err(bad_object)?;
                        break;
                    }
                    ObjectKind::Tree => {
                        if shown {
                            writeln!(out).map_err(bad_object)?;
                        }
                        writeln!(out, "tree {name}\n").map_err(bad_object)?;
                        for entry in parse_tree(&object.data, repo.hash_algo).map_err(bad_object)? {
                            out.write_all(&entry.name).map_err(bad_object)?;
                            if entry.is_dir() {
                                out.write_all(b"/").map_err(bad_object)?;
                            }
                            writeln!(out).map_err(bad_object)?;
                        }
                        shown = true;
                        break;
                    }
                    ObjectKind::Tag => {
                        if !tags.insert(oid) || tags.len() > 64 {
                            return Err(unsupported("tag nesting deeper than 64 objects"));
                        }
                        let tag = parse_tag(&object.data, repo.hash_algo).map_err(bad_object)?;
                        if shown {
                            writeln!(out).map_err(bad_object)?;
                        }
                        out.write_all(b"tag ").map_err(bad_object)?;
                        out.write_all(&tag.tag).map_err(bad_object)?;
                        writeln!(out).map_err(bad_object)?;
                        if !opts.oneline {
                            if let Some(ident) =
                                tag.tagger.as_deref().and_then(git_pretty::Ident::parse)
                            {
                                let padding = if opts.format == Format::Fuller {
                                    "    "
                                } else {
                                    ""
                                };
                                writeln!(out, "Tagger: {padding}{} <{}>", ident.name, ident.email)
                                    .map_err(bad_object)?;
                                let date = git_pretty::date::show_date(
                                    ident.ts,
                                    &opts.pretty.date,
                                    opts.pretty.now,
                                );
                                match opts.format {
                                    Format::Medium => {
                                        writeln!(out, "Date:   {date}").map_err(bad_object)?
                                    }
                                    Format::Fuller => {
                                        writeln!(out, "TaggerDate: {date}").map_err(bad_object)?
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if object.data.windows(2).any(|w| w == b"\n\n") {
                            writeln!(out).map_err(bad_object)?;
                            out.write_all(&tag.message).map_err(bad_object)?;
                        }
                        shown = true;
                        oid = tag.object;
                    }
                    ObjectKind::Commit => {
                        let info = CommitInfo::parse(oid, &object.data, repo.hash_algo)
                            .ok_or_else(|| bad_object("invalid commit"))?;
                        if info.parents.len() > 1
                            && (opts.patch || opts.output.is_some())
                            && !opts.first_parent
                        {
                            return Err(unsupported(
                                "combined merge diffs (use --no-patch or --first-parent)",
                            ));
                        }
                        if !opts.paths.is_empty() {
                            let mut probe = opts.clone();
                            probe.patch = false;
                            probe.output = Some("--name-only".into());
                            let mut paths = Vec::new();
                            render_diff(ctx, &odb, &info, &probe, &mut paths)?;
                            if paths.is_empty() {
                                break;
                            }
                        }
                        opts.pretty.abbrev = resolver.unique_abbrev_len(&oid, 7);
                        let mut header = Vec::new();
                        git_pretty::format_commit(&opts.format, &info, &opts.pretty, &mut header)
                            .map_err(|e| CommandError::fatal(e.to_string()))?;
                        let mut diff = Vec::new();
                        render_diff(ctx, &odb, &info, &opts, &mut diff)?;
                        if matches!(opts.output.as_deref(), Some("--stat" | "--shortstat")) {
                            let needle = b" changed\n";
                            if let Some(pos) = diff.windows(needle.len()).position(|w| w == needle)
                            {
                                diff.splice(
                                    pos + needle.len() - 1..pos + needle.len() - 1,
                                    b", 0 insertions(+), 0 deletions(-)".iter().copied(),
                                );
                            }
                        }
                        if shown && !opts.format.is_oneline() && !header.is_empty() {
                            writeln!(out).map_err(bad_object)?;
                        }
                        out.write_all(&header).map_err(bad_object)?;
                        if !diff.is_empty() && !header.is_empty() {
                            if !header.ends_with(b"\n") {
                                writeln!(out).map_err(bad_object)?;
                            }
                            if !opts.oneline {
                                if opts.patch && opts.output.as_deref() == Some("--stat") {
                                    write!(out, "---").map_err(bad_object)?;
                                } else {
                                    writeln!(out).map_err(bad_object)?;
                                }
                            }
                        }
                        out.write_all(&diff).map_err(bad_object)?;
                        shown = true;
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

fn unsupported(option: &str) -> CommandError {
    CommandError::usage(format!("show: {option} not supported"))
}

fn bad_object(error: impl std::fmt::Display) -> CommandError {
    CommandError::fatal(format!("fatal: {error}"))
}

fn peel_tags(odb: &Odb, mut oid: Oid) -> Result<Oid, CommandError> {
    for _ in 0..64 {
        let object = odb.read(&oid)?;
        if object.kind != ObjectKind::Tag {
            return Ok(oid);
        }
        oid = parse_tag(&object.data, odb.algorithm())
            .map_err(bad_object)?
            .object;
    }
    Err(unsupported("tag nesting deeper than 64 objects"))
}

fn resolve_object(
    resolver: &Resolver,
    odb: &Odb,
    name: &str,
    dashdash: bool,
) -> Result<Oid, CommandError> {
    if let Some((rev, path)) = name.split_once(':') {
        if rev.is_empty() || path.starts_with('/') || path.split('/').any(|p| p == "." || p == "..")
        {
            return Err(unsupported("index or relative object paths"));
        }
        let base = peel_tags(odb, resolve_object(resolver, odb, rev, dashdash)?)?;
        let object = odb.read(&base)?;
        let tree = if object.kind == ObjectKind::Commit {
            parse_commit(&object.data, odb.algorithm())
                .map_err(bad_object)?
                .tree
        } else {
            base
        };
        if path.is_empty() {
            return Ok(tree);
        }
        return resolver.resolve(&format!("{tree}:{path}")).map_err(|_| {
            CommandError::fatal(format!("fatal: path '{path}' does not exist in '{rev}'"))
        });
    }
    for suffix in ["^{tree}", "^{commit}", "^{}", "^0"] {
        if let Some(base) = name.strip_suffix(suffix) {
            let oid = peel_tags(odb, resolve_object(resolver, odb, base, dashdash)?)?;
            let object = odb.read(&oid)?;
            return match (suffix, object.kind) {
                ("^{}", _) | ("^{tree}", ObjectKind::Tree) => Ok(oid),
                ("^{tree}", ObjectKind::Commit) => Ok(parse_commit(&object.data, odb.algorithm())
                    .map_err(bad_object)?
                    .tree),
                (_, ObjectKind::Commit) => Ok(oid),
                _ => Err(unsupported("object type does not match peel expression")),
            };
        }
    }
    let oid = resolver.resolve(name).map_err(|e| {
        if dashdash {
            CommandError::fatal(format!("fatal: bad revision '{name}'"))
        } else {
            CommandError::fatal(e.render())
        }
    })?;
    odb.read(&oid)
        .map_err(|_| CommandError::fatal(format!("fatal: bad object {name}")))?;
    Ok(oid)
}

fn render_diff(
    ctx: &RepoContext,
    odb: &Odb,
    info: &CommitInfo,
    opts: &ShowOptions,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    if !opts.patch && opts.output.is_none() {
        return Ok(());
    }
    if let Some(parent) = info.parents.first() {
        let object = odb.read(parent)?;
        let parent = parse_commit(&object.data, odb.algorithm()).map_err(bad_object)?;
        let mut args = Vec::new();
        if let Some(output) = &opts.output {
            args.push(output.clone());
        }
        if opts.patch {
            if opts.output.as_deref() == Some("--stat") {
                args.push("--patch-with-stat".into());
            } else if opts.output.is_some() {
                return Err(unsupported("patch combined with this summary format"));
            } else {
                args.push("--patch".into());
            }
        }
        args.push(format!("--unified={}", opts.context));
        if opts.no_renames {
            args.push("--no-renames".into());
        }
        args.extend([parent.tree.to_string(), info.tree.to_string(), "--".into()]);
        args.extend(opts.paths.iter().cloned());
        return crate::diff::Diff.run(ctx, &args, out);
    }
    let object = odb.read(&info.tree)?;
    let entries = parse_tree(&object.data, odb.algorithm()).map_err(bad_object)?;
    let mut loader = |oid: &Oid| odb.read(oid).ok();
    let mut changes = git_diff::compare_trees(&[], &entries, "", true, &mut loader);
    if !opts.paths.is_empty() {
        changes.retain(|c| {
            opts.paths.iter().any(|p| {
                let p = p.trim_end_matches('/');
                p == "." || p.is_empty() || c.path == p || c.path.starts_with(&format!("{p}/"))
            })
        });
    }
    if changes.is_empty() {
        return Ok(());
    }
    let extra = HashMap::new();
    let src = BlobSource { odb, extra: &extra };
    match opts.output.as_deref() {
        Some("--stat") => patch::render_stat(&changes, &src, out)?,
        Some("--shortstat") => patch::render_shortstat(&changes, &src, out)?,
        Some("--numstat") => {
            for c in &changes {
                patch::render_numstat(c, &src, out)?;
            }
        }
        Some("--name-only" | "--name-status") => {
            for c in &changes {
                patch::render_name_line(c, opts.output.as_deref() == Some("--name-status"), out)?;
            }
        }
        _ => {}
    }
    if opts.patch {
        if opts.output.is_some() {
            writeln!(out).map_err(bad_object)?;
        }
        for c in &changes {
            out.write_all(&patch::render_change_patch_ctx(
                c,
                &src,
                opts.context,
                None,
            )?)
            .map_err(bad_object)?;
        }
    }
    Ok(())
}
