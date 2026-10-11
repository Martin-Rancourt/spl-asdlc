//! Notifications (`docs/spec/05-release-cycle.md` §12, REQ-REL-094/095):
//! signals sent where a human reads them, never the only record (the
//! tracker comment is). A notice no configured channel takes has nothing to
//! send: done, never a failure. A send through a channel that takes it and
//! fails (or cannot be built) is an error the caller logs and retries; it is
//! never marked sent.

pub mod telegram;

use crate::config::{self, NotifyKind, SecretRef};
use crate::http::UreqTransport;

/// One channel.
pub trait Notifier {
    /// A name for logs (`telegram`), never the secret target.
    fn name(&self) -> String;
    /// The longest message the channel accepts, in its own unit.
    fn max_len(&self) -> usize;
    /// The length of a text in the channel's unit.
    fn measure(&self, text: &str) -> usize;
    /// Send a text as it is (the channel adapts syntax, never the words).
    fn send(&self, text: &str) -> Result<(), String>;
}

/// Cut a text on whole lines so it fits, with a visible mark; the first line
/// is never cut unless nothing else fits.
pub fn fit(text: &str, n: &dyn Notifier) -> String {
    if n.measure(text) <= n.max_len() {
        return text.to_string();
    }
    let mut lines = text.lines();
    let head = lines.next().unwrap_or("");
    let body: Vec<&str> = lines.collect();
    crate::method::release::note::published_message(head, &body.join("\n"), n.max_len(), &|s| {
        n.measure(s)
    })
}

/// Where the bridge sends a notice: the configured channels, or a dry run's
/// record of what it would send.
pub trait Signal {
    /// Send `text` to every channel subscribed to `event`; the names of the
    /// channels it reached (none when no channel takes it: nothing to send),
    /// or an error naming those it did not.
    fn signal(&self, event: &str, text: &str) -> Result<Vec<String>, String>;

    /// Whether any channel takes `event`; an event nobody subscribed to is
    /// not sent, and not a failure.
    fn takes(&self, _event: &str) -> bool {
        true
    }
}

impl Signal for Channels {
    fn signal(&self, event: &str, text: &str) -> Result<Vec<String>, String> {
        self.notify(event, text)
    }

    fn takes(&self, event: &str) -> bool {
        self.subscribed(event) > 0
    }
}

/// The configured channels and the events each one takes.
pub struct Channels {
    channels: Vec<(Vec<String>, Box<dyn Notifier>)>,
}

impl Channels {
    pub fn new(channels: Vec<(Vec<String>, Box<dyn Notifier>)>) -> Channels {
        Channels { channels }
    }

    /// Build the channels of a configuration, reading their secrets from
    /// `env`; a reference that resolves to nothing is an error naming it.
    pub fn from_config(
        notify: &[config::Notify],
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Channels, String> {
        let resolve = |r: &SecretRef| match r {
            SecretRef::Env(name) => env(name)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| format!("{r} is not set on this host")),
        };
        let mut channels: Vec<(Vec<String>, Box<dyn Notifier>)> = Vec::new();
        for n in notify {
            match n.kind {
                NotifyKind::Telegram => {
                    let token = n
                        .token
                        .as_ref()
                        .ok_or("a telegram channel needs its token reference")
                        .map_err(str::to_string)
                        .and_then(resolve)?;
                    let chat = resolve(&n.target)?;
                    channels.push((
                        n.events.clone(),
                        Box::new(telegram::Telegram::new(
                            &token,
                            &chat,
                            Box::new(UreqTransport::default()),
                        )),
                    ));
                }
                other => return Err(format!("{other:?} is not shipped by this build")),
            }
        }
        Ok(Channels { channels })
    }

    pub fn subscribed(&self, event: &str) -> usize {
        self.channels
            .iter()
            .filter(|(e, _)| e.iter().any(|x| x == event))
            .count()
    }

    /// Send `text` to every channel subscribed to `event`. No channel takes
    /// it: nothing to send, no channel named, not a failure (a project may
    /// configure none); one failing channel fails the notice.
    pub fn notify(&self, event: &str, text: &str) -> Result<Vec<String>, String> {
        let targets: Vec<&Box<dyn Notifier>> = self
            .channels
            .iter()
            .filter(|(e, _)| e.iter().any(|x| x == event))
            .map(|(_, n)| n)
            .collect();
        let mut sent = Vec::new();
        let mut failed = Vec::new();
        for n in targets {
            match n.send(&fit(text, n.as_ref())) {
                Ok(()) => sent.push(n.name()),
                Err(e) => failed.push(format!("{}: {e}", n.name())),
            }
        }
        if failed.is_empty() {
            Ok(sent)
        } else {
            Err(format!("not sent through {}", failed.join("; ")))
        }
    }
}

/// A channel that records what it was asked to send, for tests.
#[derive(Default)]
pub struct Recorder {
    pub sent: std::cell::RefCell<Vec<String>>,
    pub fail: bool,
    pub max: usize,
    /// No channel is configured: no event is taken, nothing is sent.
    pub no_channel: bool,
}

impl Signal for Recorder {
    fn signal(&self, event: &str, text: &str) -> Result<Vec<String>, String> {
        if self.no_channel {
            return Ok(Vec::new());
        }
        self.send(&format!("[{event}] {text}"))?;
        Ok(vec![self.name()])
    }

    fn takes(&self, _event: &str) -> bool {
        !self.no_channel
    }
}

impl Notifier for Recorder {
    fn name(&self) -> String {
        "recorder".into()
    }
    fn max_len(&self) -> usize {
        if self.max == 0 { 4096 } else { self.max }
    }
    fn measure(&self, text: &str) -> usize {
        text.chars().count()
    }
    fn send(&self, text: &str) -> Result<(), String> {
        if self.fail {
            return Err("refused".into());
        }
        self.sent.borrow_mut().push(text.to_string());
        Ok(())
    }
}

impl<T: Notifier + ?Sized> Notifier for std::rc::Rc<T> {
    fn name(&self) -> String {
        (**self).name()
    }
    fn max_len(&self) -> usize {
        (**self).max_len()
    }
    fn measure(&self, text: &str) -> usize {
        (**self).measure(text)
    }
    fn send(&self, text: &str) -> Result<(), String> {
        (**self).send(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn a_notice_no_channel_takes_has_nothing_to_send_and_is_no_failure() {
        let c = Channels::new(vec![]);
        assert!(!c.takes("release-note"));
        assert_eq!(c.notify("release-note", "x").unwrap(), Vec::<String>::new());
        let other = Rc::new(Recorder::default());
        let c = Channels::new(vec![(vec!["escalation".into()], Box::new(other.clone()))]);
        assert!(!c.takes("release-note"));
        assert!(c.notify("release-note", "x").unwrap().is_empty());
        assert!(other.sent.borrow().is_empty());
    }

    #[test]
    fn a_notice_goes_to_the_channels_of_its_event_only() {
        let a = Rc::new(Recorder::default());
        let b = Rc::new(Recorder::default());
        let c = Channels::new(vec![
            (vec!["release-note".into()], Box::new(a.clone())),
            (vec!["escalation".into()], Box::new(b.clone())),
        ]);
        assert_eq!(c.notify("release-note", "hello").unwrap(), vec!["recorder"]);
        assert_eq!(a.sent.borrow().len(), 1);
        assert!(b.sent.borrow().is_empty());
        assert_eq!(c.subscribed("escalation"), 1);
    }

    #[test]
    fn a_failed_channel_fails_the_notice() {
        let ok = Rc::new(Recorder::default());
        let bad = Rc::new(Recorder {
            fail: true,
            ..Recorder::default()
        });
        let c = Channels::new(vec![
            (vec!["escalation".into()], Box::new(ok)),
            (vec!["escalation".into()], Box::new(bad)),
        ]);
        assert!(
            c.notify("escalation", "x")
                .unwrap_err()
                .contains("not sent through recorder: refused")
        );
    }

    #[test]
    fn a_long_notice_is_cut_on_whole_lines_with_its_first_line_kept() {
        let r = Rc::new(Recorder {
            max: 60,
            ..Recorder::default()
        });
        let c = Channels::new(vec![(vec!["stagnation".into()], Box::new(r.clone()))]);
        let text = format!(
            "Release stuck: `acme-v1.2.3-rc.1`.\n{}",
            (0..20)
                .map(|i| format!("- line {i}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        c.notify("stagnation", &text).unwrap();
        let sent = r.sent.borrow()[0].clone();
        assert!(sent.starts_with("Release stuck: `acme-v1.2.3-rc.1`."));
        assert!(sent.chars().count() <= 60);
    }

    #[test]
    fn a_channel_whose_secret_is_not_on_the_host_is_named() {
        let config = crate::config::spec_example::config();
        let err = Channels::from_config(&config.notify, &|_| None)
            .err()
            .unwrap();
        assert!(err.contains("env:SPL_ASDLC_TELEGRAM"), "{err}");
        let ok = Channels::from_config(&config.notify, &|k| Some(format!("value-of-{k}"))).unwrap();
        assert_eq!(ok.subscribed("release-note"), 1);
    }
}
