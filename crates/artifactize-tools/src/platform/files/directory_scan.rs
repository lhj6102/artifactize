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
