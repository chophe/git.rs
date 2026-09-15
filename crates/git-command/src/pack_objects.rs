//! `git pack-objects`: write a pack (and index) from object ids read on stdin.
//!
//! Supports the delta-selection options `--window`, `--depth`,
//! `--delta-base-offset` / `--no-delta-base-offset`, `--compression`, plus
//! `--thin`, `--revs`, `--stdin`, `--non-empty` (accepted for compatibility;
//! see notes inline).

use std::io::{BufRead, Write};

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_odb::pack::{write_pack_opts, PackObject, PackOptions};
use git_odb::Odb;

pub struct PackObjects;

impl PackObjects {
    /// Parse a value that may arrive as `--opt=<val>` or `--opt <val>`.
    fn take_value(
        args: &[String],
        i: &mut usize,
        flag: &str,
        inline: Option<&str>,
    ) -> Result<String, CommandError> {
        if let Some(v) = inline {
            return Ok(v.to_string());
        }
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| CommandError::usage(format!("option `{flag}' requires a value")))
    }

    fn parse_usize(s: &str, flag: &str) -> Result<usize, CommandError> {
        s.parse::<usize>()
            .map_err(|_| CommandError::usage(format!("{flag} expects a numerical value")))
    }
}

impl Command for PackObjects {
    fn name(&self) -> &'static str {
        "pack-objects"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut to_stdout = false;
        let mut base_name: Option<String> = None;
        let mut opts = PackOptions::default();
        let mut non_empty = false;

        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "--stdout" => to_stdout = true,
                "-q" | "--quiet" => {}
                "--thin" => {
                    // We never omit bases, so the pack we emit is a valid
                    // (non-thin) superset; callers that asked for thin get a
                    // pack they can still index.
                }
                "--revs" => {
                    // Accepted for compatibility with `repack`; stdin lines
                    // are still resolved as object names below.
                }
                "--stdin" => {}
                "--non-empty" => non_empty = true,
                "--delta-base-offset" => opts.allow_ofs_delta = true,
                "--no-delta-base-offset" => opts.allow_ofs_delta = false,
                s if s.starts_with("--window=") => {
                    let v = Self::take_value(args, &mut i, "--window", s.strip_prefix("--window="))?;
                    opts.window = Self::parse_usize(&v, "--window")?;
                }
                "--window" => {
                    let v = Self::take_value(args, &mut i, "--window", None)?;
                    opts.window = Self::parse_usize(&v, "--window")?;
                }
                s if s.starts_with("--depth=") => {
                    let v = Self::take_value(args, &mut i, "--depth", s.strip_prefix("--depth="))?;
                    opts.depth = Self::parse_usize(&v, "--depth")?;
                }
                "--depth" => {
                    let v = Self::take_value(args, &mut i, "--depth", None)?;
                    opts.depth = Self::parse_usize(&v, "--depth")?;
                }
                s if s.starts_with("--compression=") => {
                    let v = Self::take_value(args, &mut i, "--compression", s.strip_prefix("--compression="))?;
                    opts.compression = Self::parse_usize(&v, "--compression")?.min(9) as u32;
                }
                "--compression" => {
                    let v = Self::take_value(args, &mut i, "--compression", None)?;
                    opts.compression = Self::parse_usize(&v, "--compression")?.min(9) as u32;
                }
                s if s.starts_with("--window-memory=") => {
                    // Accepted; our delta search is bounded by `window`/`depth`
                    // only, so this is a no-op.
                }
                "--window-memory" => {
                    let _ = Self::take_value(args, &mut i, "--window-memory", None)?;
                }
                s if s.starts_with("--threads=") => {}
                "--threads" => {
                    let _ = Self::take_value(args, &mut i, "--threads", None)?;
                }
                s if s.starts_with("--max-pack-size=") => {}
                "--max-pack-size" => {
                    let _ = Self::take_value(args, &mut i, "--max-pack-size", None)?;
                }
                // Advisory flags accepted as no-ops.
                "--keep-true-parents"
                | "--progress"
                | "--all-progress"
                | "--all-progress-implied"
                | "--honor-pack-keep"
                | "--incremental"
                | "--local" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("pack-objects: unknown option '{s}'")));
                }
                b => base_name = Some(b.to_string()),
            }
            i += 1;
        }
        if to_stdout && base_name.is_some() {
            return Err(CommandError::usage("pack-objects: --stdout and <base-name> are mutually exclusive"));
        }

        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut line = String::new();
        let mut oids: Vec<Oid> = Vec::new();
        loop {
            line.clear();
            if handle
                .read_line(&mut line)
                .map_err(|e| CommandError::fatal(e.to_string()))?
                == 0
            {
                break;
            }
            let oid_s = line.trim();
            if oid_s.is_empty() {
                continue;
            }
            let oid = Oid::from_hex(oid_s, algo)
                .map_err(|_| CommandError::error(format!("not a valid object name: '{oid_s}'")))?;
            oids.push(oid);
        }

        if non_empty && oids.is_empty() {
            // C git's `--non-empty` silently succeeds without writing a pack
            // when there is nothing to pack.
            return Ok(());
        }

        let mut pack_objs = Vec::with_capacity(oids.len());
        for oid in &oids {
            let obj = odb
                .read(oid)
                .map_err(|e| CommandError::error(format!("{oid}: {e}")))?;
            pack_objs.push(PackObject {
                oid: *oid,
                kind: obj.kind,
                data: obj.data,
            });
        }

        let (pack, idx) = write_pack_opts(&pack_objs, algo, opts).map_err(CommandError::from)?;

        if to_stdout {
            out.write_all(&pack).map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        if let Some(base) = base_name {
            std::fs::write(format!("{base}.pack"), &pack)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            std::fs::write(format!("{base}.idx"), &idx)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}
