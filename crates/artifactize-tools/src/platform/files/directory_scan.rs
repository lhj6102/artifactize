//! Windows handle-directory query lifecycle, independent of buffer decoding and OS calls.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectoryScan {
    First,
    Following,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Query {
    Restart,
    Continue,
}

impl DirectoryScan {
    /// Mark the first query consumed before calling the OS, as the prior restart flag did.
    pub fn query(&mut self) -> Option<Query> {
        match self {
            Self::First => {
                *self = Self::Following;
                Some(Query::Restart)
            }
            Self::Following => Some(Query::Continue),
            Self::Done => None,
        }
    }

    /// Both end-of-directory and any query/record error end this iterator without a retry.
    pub fn finish(&mut self) {
        *self = Self::Done;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_query_restarts_then_buffer_refills_continue() {
        let mut scan = DirectoryScan::First;
        assert_eq!(scan.query(), Some(Query::Restart));
        assert_eq!(scan, DirectoryScan::Following);
        assert_eq!(scan.query(), Some(Query::Continue));
        assert_eq!(scan.query(), Some(Query::Continue));
    }

    #[test]
    fn end_of_directory_or_errors_never_query_again() {
        for queries_before_end in [1, 2, 3] {
            let mut scan = DirectoryScan::First;
            for query in 0..queries_before_end {
                assert_eq!(
                    scan.query(),
                    Some(if query == 0 {
                        Query::Restart
                    } else {
                        Query::Continue
                    })
                );
            }
            // ERROR_NO_MORE_FILES, other query errors, and malformed records share the
            // same terminal transition; the caller still distinguishes their returned item.
            scan.finish();
            assert_eq!(scan, DirectoryScan::Done);
            assert_eq!(scan.query(), None);
            assert_eq!(scan.query(), None);
        }
    }
}
