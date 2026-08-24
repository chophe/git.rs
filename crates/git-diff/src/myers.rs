//! Line-based diff (edit script computation).
//!
//! Computes a minimal edit script between two line sequences via longest
//! common subsequence (LCS) dynamic programming. On ties the backtracker
//! prefers insertion over deletion, which matches git's output for the common
//! insertion/deletion cases.

/// A single edit operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The line is unchanged (present in both inputs).
    Keep,
    /// The line was removed from the first input.
    Delete,
    /// The line was added from the second input.
    Insert,
}

/// Split bytes into lines (each slice includes its trailing `\n`, except a
/// final unterminated line).
pub fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            lines.push(&data[start..=i]);
            start = i + 1;
        }
    }
    if start < data.len() {
        lines.push(&data[start..]);
    }
    lines
}

/// Compute the edit script transforming `a` into `b`.
///
/// Uses the classic Myers O(ND) algorithm. The tie-breaking (prefer the
/// right/diagonal move, i.e. delete over insert on equal forward cost)
/// reproduces git's `-old +new` ordering for single-line replacements.
pub fn diff(a: &[&[u8]], b: &[&[u8]]) -> Vec<Op> {
    let n = a.len() as i64;
    let m = b.len() as i64;
    let max = (n + m) as usize;
    let offset = max as i64;

    let mut v = vec![0i64; 2 * max + 1];
    let mut trace: Vec<Vec<i64>> = Vec::new();
    let mut d_found = 0usize;

    v[(offset + 1) as usize] = 0;
    'outer: for d in 0..=max {
        trace.push(v.clone());
        let d = d as i64;
        let mut k = -d;
        while k <= d {
            let x = if k == -d || (k != d && v[(offset + k - 1) as usize] < v[(offset + k + 1) as usize])
            {
                v[(offset + k + 1) as usize]
            } else {
                v[(offset + k - 1) as usize] + 1
            };
            let mut x = x;
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(offset + k) as usize] = x;
            if x >= n && y >= m {
                d_found = d as usize;
                break 'outer;
            }
            k += 2;
        }
    }

    // Backtrack to recover the edit script.
    let mut ops: Vec<Op> = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..=d_found as i64).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && v[(offset + k - 1) as usize] < v[(offset + k + 1) as usize])
        {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v[(offset + prev_k) as usize];
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            ops.push(Op::Keep);
            x -= 1;
            y -= 1;
        }
        if d == 0 {
            break;
        }
        if x == prev_x {
            ops.push(Op::Insert);
        } else {
            ops.push(Op::Delete);
        }
        x = prev_x;
        y = prev_y;
    }
    ops.reverse();
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ls(text: &str) -> Vec<&[u8]> {
        split_lines(text.as_bytes())
    }

    #[test]
    fn splits_lines() {
        assert_eq!(ls("a\nb\n"), vec![b"a\n".as_slice(), b"b\n".as_slice()]);
        // Unterminated final line.
        assert_eq!(ls("a\nb"), vec![b"a\n".as_slice(), b"b".as_slice()]);
    }

    #[test]
    fn identical_inputs_produce_no_ops() {
        let a = ls("a\nb\nc\n");
        let b = ls("a\nb\nc\n");
        assert_eq!(diff(&a, &b), vec![Op::Keep; 3]);
    }

    #[test]
    fn insertion_is_an_insert() {
        let a = ls("a\nb\n");
        let b = ls("a\nX\nb\n");
        let ops = diff(&a, &b);
        assert_eq!(
            ops,
            vec![Op::Keep, Op::Insert, Op::Keep]
        );
    }

    #[test]
    fn deletion_is_a_delete() {
        let a = ls("a\nb\nc\n");
        let b = ls("a\nc\n");
        let ops = diff(&a, &b);
        assert_eq!(ops, vec![Op::Keep, Op::Delete, Op::Keep]);
    }
}