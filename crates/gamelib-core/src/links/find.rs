//! Finding a game on the sites that host it, rather than checking a URL the user already has.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::redirect::Policy;

use super::SiteHandler;
use crate::Result;
use crate::http::USER_AGENT;

pub use crate::model::FoundLink;

/// What a site gets to search with: the Steam game's id and name.
pub struct FindQuery {
    pub appid: u32,
    pub title: String,
}

/// HTTP client for fetching site pages. Unlike [`super::resolve::client`] this one follows
/// redirects, because the sites 301 between hostnames.
pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .redirect(Policy::limited(10))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .gzip(true)
        .build()?)
}

/// Asks every registered handler, concatenates the results, best first. A handler that errors
/// or panics is skipped, the rest of the search goes on. Handlers run at the same time, since
/// each one blocks on its own site; joining in registry order keeps the output deterministic.
pub fn find_all(
    handlers: &[Box<dyn SiteHandler>],
    query: &FindQuery,
    http: &Client,
) -> Vec<FoundLink> {
    let found: Vec<Vec<FoundLink>> = std::thread::scope(|scope| {
        let workers: Vec<_> = handlers
            .iter()
            .map(|handler| {
                scope.spawn(|| {
                    catch_unwind(AssertUnwindSafe(|| {
                        handler.find(query, http).unwrap_or_default()
                    }))
                    .unwrap_or_default()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap_or_default())
            .collect()
    });
    let mut out: Vec<FoundLink> = found.into_iter().flatten().collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// The slice between the first `start` and the next `end` after it.
pub fn extract_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = haystack.find(start)? + start.len();
    let rest = &haystack[from..];
    let to = from + rest.find(end)?;
    Some(&haystack[from..to])
}

/// Decodes the handful of HTML entities the sites emit. `&amp;` goes last so `&amp;#038;`
/// does not double-decode.
pub fn decode_entities(s: &str) -> String {
    s.replace("&#038;", "&")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&#8211;", "\u{2013}")
        .replace("&#8212;", "\u{2014}")
        .replace("&amp;", "&")
}

/// Lowercase, ASCII alphanumerics kept, every other run of characters becomes one `-`. This
/// mirrors how the sites build their URL slugs.
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::thread;

    use super::*;
    use crate::links::SiteHandler;
    use crate::model::{LinkKind, SiteInfo};

    /// A handler that announces its start and then waits until the test releases it, so the
    /// test can tell overlapping execution from a serial loop without measuring time.
    struct Gated {
        info: SiteInfo,
        name: &'static str,
        score: f32,
        started: Sender<&'static str>,
        release: Mutex<Receiver<()>>,
    }

    impl SiteHandler for Gated {
        fn info(&self) -> &SiteInfo {
            &self.info
        }

        fn find(&self, _query: &FindQuery, _http: &Client) -> Result<Vec<FoundLink>> {
            self.started.send(self.name).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Ok(vec![FoundLink {
                site_id: self.info.id.clone(),
                url: format!("https://{}.example/game", self.info.id),
                label: self.info.name.clone(),
                kind: LinkKind::Page,
                version: None,
                size: None,
                notes: None,
                score: self.score,
                needs_browser: false,
                direct: false,
            }])
        }
    }

    #[test]
    fn handlers_run_at_the_same_time_and_results_sort_by_score() {
        let (started, started_rx) = channel();
        let (release_slow, hold_slow) = channel();
        let (release_fast, hold_fast) = channel();
        let gated = |id: &'static str, score: f32, hold| Gated {
            info: SiteInfo {
                id: id.into(),
                name: id.into(),
                homepage: None,
                domains: Vec::new(),
                color: "#000000".into(),
                browser_required: false,
            },
            name: id,
            score,
            started: started.clone(),
            release: Mutex::new(hold),
        };
        let handlers: Vec<Box<dyn SiteHandler>> = vec![
            Box::new(gated("slow", 0.3, hold_slow)),
            Box::new(gated("fast", 0.9, hold_fast)),
        ];
        let query = FindQuery {
            appid: 570,
            title: "Dota 2".into(),
        };
        let http = client().unwrap();

        let found = thread::scope(|scope| {
            let caller = scope.spawn(|| find_all(&handlers, &query, &http));
            // Neither handler may be released until both have entered `find`: a serial
            // `find_all` would leave the second start pending and time out here.
            let first = started_rx.recv_timeout(Duration::from_secs(10));
            let second = started_rx.recv_timeout(Duration::from_secs(10));
            // Release even when a start is missing, so the scope's worker can always finish.
            release_slow.send(()).unwrap();
            release_fast.send(()).unwrap();
            let mut names = [first.unwrap(), second.unwrap()];
            names.sort();
            assert_eq!(names, ["fast", "slow"]);
            caller.join().unwrap()
        });

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].site_id, "fast");
        assert_eq!(found[1].site_id, "slow");
    }

    #[test]
    fn extracts_between() {
        assert_eq!(extract_between("a<b>c</b>d", "<b>", "</b>"), Some("c"));
        assert_eq!(extract_between("no start", "<b>", "</b>"), None);
        assert_eq!(extract_between("no end", "<b>", "</b>"), None);
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_entities("a&amp;b"), "a&b");
        assert_eq!(decode_entities("&#038;"), "&");
        assert_eq!(decode_entities("&amp;#038;"), "&#038;");
        assert_eq!(decode_entities("&quot;x&quot;"), "\"x\"");
        assert_eq!(decode_entities("&#039; &apos;"), "' '");
        assert_eq!(decode_entities("&lt;tag&gt;"), "<tag>");
        assert_eq!(decode_entities("a&nbsp;b"), "a b");
        assert_eq!(decode_entities("&#8211; &#8212;"), "\u{2013} \u{2014}");
    }

    #[test]
    fn slugifies() {
        assert_eq!(
            slugify("The Witcher 3: Wild Hunt"),
            "the-witcher-3-wild-hunt"
        );
        assert_eq!(slugify("ELDEN RING NIGHTREIGN"), "elden-ring-nightreign");
        assert_eq!(
            slugify("ACE COMBAT 7: SKIES UNKNOWN"),
            "ace-combat-7-skies-unknown"
        );
    }
}
