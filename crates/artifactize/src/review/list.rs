//! The standalone review's list: the request search's query, its pages and the keys that move
//! through them. Matching and completion are [`super::search`]'s; this is only the screen.
use super::{Action, Area, Focus, Review, search};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Candidates the footer names after Tab before `+N`.
const SHOWN_CANDIDATES: usize = 6;

impl Review {
    /// Read the current page of the query's matches; an invalid query lists nothing.
    pub(super) async fn search(&mut self) -> Result<(), String> {
        let found = match &self.filter {
            Ok(query) => {
                let start = *self.pages.last().expect("a page");
                search::search(&self.state, query, &self.reviewer, start).await?
            }
            Err(_) => search::Found::default(),
        };
        let selected = self.selected().map(|view| view.request.id.clone());
        let (listed, entries): (Vec<_>, Vec<_>) = found.requests.into_iter().unzip();
        self.entries = entries;
        let index = selected
            .and_then(|id| listed.iter().position(|view| view.request.id == id))
            .or(self.list.selected());
        self.listed = listed;
        self.next = found.next;
        self.list.select(
            index
                .map(|index| index.min(self.listed.len().saturating_sub(1)))
                .or(Some(0))
                .filter(|_| !self.listed.is_empty()),
        );
        Ok(())
    }

    /// List `views` as if the search had found them, each from its recorded repository.
    #[cfg(test)]
    pub(crate) fn set_listed(&mut self, views: Vec<super::RequestView>) {
        self.entries = views
            .iter()
            .map(|view| {
                let repo = view
                    .request
                    .human_definition
                    .as_ref()
                    .and_then(|definition| definition.repo.value().cloned())
                    .unwrap_or_default();
                search::Entry::new(view, &repo, &self.reviewer)
            })
            .collect();
        self.listed = views;
        self.list.select((!self.listed.is_empty()).then_some(0));
    }

    pub(super) fn selected(&self) -> Option<&super::RequestView> {
        self.list
            .selected()
            .and_then(|index| self.listed.get(index))
    }

    /// The query text, what it is shown as while edited, and its error.
    pub(super) fn query_line(&self) -> (String, Option<&str>) {
        let cursor = if self.editing_query.is_some() {
            "▏"
        } else {
            ""
        };
        (
            format!("/ {}{cursor}", self.query),
            self.filter.as_ref().err().map(String::as_str),
        )
    }

    /// The page shown, counted from 1.
    pub(super) fn page(&self) -> usize {
        self.pages.len()
    }

    /// The query finds only waiting Human reviews, as the default does.
    pub(super) fn waiting_query(&self) -> bool {
        self.filter.as_ref().is_ok_and(search::Query::waiting_human)
    }

    pub(super) fn candidates(&self) -> String {
        let mut shown: Vec<&str> = self
            .candidates
            .iter()
            .take(SHOWN_CANDIDATES)
            .map(String::as_str)
            .collect();
        let more = self.candidates.len().saturating_sub(SHOWN_CANDIDATES);
        let more = format!("+{more}");
        if self.candidates.len() > SHOWN_CANDIDATES {
            shown.push(&more);
        }
        shown.join(" ")
    }

    /// A new query starts again from the first page.
    fn requery(&mut self) -> Action {
        self.filter = self.query.parse();
        self.pages = vec![0];
        Action::Refresh
    }

    pub(super) fn list_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') => self.quit(),
            KeyCode::Char('r') => Action::Refresh,
            KeyCode::Char('/') => {
                self.editing_query = Some(self.query.clone());
                self.candidates.clear();
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let next = self.list.selected().map_or(0, |index| index + 1);
                if next < self.listed.len() {
                    self.list.select(Some(next));
                }
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.list.select_previous();
                Action::None
            }
            // Older matches; PgUp goes back to newer ones.
            KeyCode::PageDown => match self.next {
                Some(next) => {
                    self.pages.push(next);
                    self.list.select(Some(0));
                    Action::Refresh
                }
                None => Action::None,
            },
            KeyCode::PageUp if self.pages.len() > 1 => {
                self.pages.pop();
                self.list.select(Some(0));
                Action::Refresh
            }
            KeyCode::Enter | KeyCode::Right => match self.selected().cloned() {
                Some(view) => self.open_view(view),
                None => Action::None,
            },
            // Straight to the tools of the selected request.
            KeyCode::Tab => match self.selected().cloned() {
                Some(view) => {
                    let action = self.open_view(view);
                    self.area = Area::Tools;
                    action
                }
                None => Action::None,
            },
            _ => Action::None,
        }
    }

    /// Keys while `/` edits the query: typing searches as it goes, Tab completes the last term,
    /// Enter keeps the query and Esc restores the one before.
    pub(super) fn query_key(&mut self, key: KeyEvent) -> Action {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter | KeyCode::Down => {
                self.editing_query = None;
                self.candidates.clear();
                Action::None
            }
            KeyCode::Esc => {
                if let Some(before) = self.editing_query.take() {
                    self.query = before;
                }
                self.candidates.clear();
                self.requery()
            }
            KeyCode::Tab => {
                let completion = search::complete(&self.query, &self.entries);
                match completion.candidates.as_slice() {
                    [] => {}
                    [only] => {
                        let space = if only.ends_with(':') { "" } else { " " };
                        self.query = format!("{}{only}{space}", completion.keep);
                    }
                    many => {
                        // Extend to what every candidate shares.
                        let shared = many.iter().skip(1).fold(many[0].clone(), |shared, next| {
                            shared
                                .chars()
                                .zip(next.chars())
                                .take_while(|(a, b)| a == b)
                                .map(|(a, _)| a)
                                .collect()
                        });
                        let current = &self.query[completion.keep.len()..];
                        if shared.chars().count() > current.chars().count() {
                            self.query = format!("{}{shared}", completion.keep);
                        }
                    }
                }
                self.candidates = completion.candidates;
                self.requery()
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.candidates.clear();
                self.requery()
            }
            KeyCode::Char('u') if control => {
                self.query.clear();
                self.requery()
            }
            KeyCode::Char(character) if !control => {
                self.query.push(character);
                self.candidates.clear();
                self.requery()
            }
            _ => Action::None,
        }
    }

    /// Keys of a read-only Detail: Esc or ← back to the list, q quits.
    pub(super) fn record_key(&mut self, key: KeyEvent) -> Action {
        if let Some(record) = &mut self.record
            && record.key(key)
        {
            return Action::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Left => {
                self.focus = Focus::List;
                Action::Refresh
            }
            KeyCode::Char('q') => self.quit(),
            _ => Action::None,
        }
    }

    /// The read-only Detail's reader, for the driver's session jobs.
    pub(super) fn record_live(&mut self) -> Option<&mut crate::monitor::session::Live> {
        self.record
            .as_mut()
            .filter(|_| self.focus == Focus::Detail)
            .and_then(|record| record.live.as_mut())
    }
}
