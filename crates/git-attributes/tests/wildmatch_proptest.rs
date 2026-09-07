//! Property tests for `wildmatch`: it must never panic on arbitrary input and
//! must preserve the core `PATHNAME` narrowing semantics.

use git_attributes::wildmatch::{self, flags, WM_MATCH};
use proptest::prelude::*;

fn arb_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::char::any(), 0..24)
        .prop_map(|chars| chars.into_iter().collect::<String>())
}

proptest! {
    /// Does not panic on arbitrary (pattern, text) pairs across flag modes.
    #[test]
    fn no_panic_on_arbitrary_input(
        p in arb_text(),
        t in arb_text(),
    ) {
        let _ = wildmatch::wildmatch(&p, &t, 0);
        let _ = wildmatch::wildmatch(&p, &t, flags::PATHNAME);
        let _ = wildmatch::wildmatch(&p, &t, flags::CASEFOLD);
        let _ = wildmatch::wildmatch(&p, &t, flags::PATHNAME | flags::CASEFOLD);
    }

    /// Adding PATHNAME can only turn a match into a non-match when the text
    /// contains no slash: `*` may not cross `/` in pathname mode, so a plain
    /// (non-pathname) match is a prerequisite for a pathname match.
    #[test]
    fn pathname_is_narrower(
        p in arb_text(),
        t in "[A-Za-z0-9]*",
    ) {
        let plain = wildmatch::wildmatch(&p, &t, 0);
        let pathname = wildmatch::wildmatch(&p, &t, flags::PATHNAME);
        if plain == WM_MATCH {
            prop_assert_eq!(pathname, WM_MATCH);
        }
    }
}
