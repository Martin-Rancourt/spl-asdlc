//! Versions, tags and release branches of a product (REQ-REL-004, 008, 018,
//! 027, 028): the shape is data from `.spl-asdlc/config.toml`, compiled once.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

use crate::config;

static SEMVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v?(\d+)\.(\d+)\.(\d+)$").unwrap());
static BRANCH_STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}-[0-9]{2}-[0-9]{2}-utc$").unwrap());

/// `X.Y.Z`, ordered numerically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// `1.2.3` or `v1.2.3`.
    pub fn parse(s: &str) -> Option<Version> {
        let c = SEMVER.captures(s.trim())?;
        Some(Version {
            major: c[1].parse().ok()?,
            minor: c[2].parse().ok()?,
            patch: c[3].parse().ok()?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A product's tags, as configured.
#[derive(Debug, Clone)]
pub struct Shape {
    pub product: String,
    pub candidate_template: String,
    pub release_template: String,
    candidate_re: Regex,
    release_re: Regex,
    pub branch_prefix: String,
}

/// A validated tag template as a regex, `version` (and `rc`) captured.
pub(crate) fn template_regex(template: &str) -> Regex {
    let escaped = regex::escape(template)
        .replace(r"\{version\}", r"(?P<version>\d+\.\d+\.\d+)")
        .replace(r"\{rc\}", r"(?P<rc>\d+)");
    Regex::new(&format!("^{escaped}$")).expect("a validated template compiles")
}

impl Shape {
    /// Built from a validated configuration (its templates are checked).
    pub fn of(product: &str, release: &config::Release) -> Shape {
        Shape {
            product: product.to_string(),
            candidate_template: release.candidate_tag.clone(),
            release_template: release.release_tag.clone(),
            candidate_re: template_regex(&release.candidate_tag),
            release_re: template_regex(&release.release_tag),
            branch_prefix: release.branch_prefix.clone(),
        }
    }

    /// `(version, rc)` of a candidate tag of this product, or `None`.
    pub fn parse_candidate(&self, tag: &str) -> Option<(Version, u64)> {
        let c = self.candidate_re.captures(tag)?;
        Some((Version::parse(&c["version"])?, c["rc"].parse().ok()?))
    }

    /// The version of a final (published) tag of this product.
    pub fn parse_release(&self, tag: &str) -> Option<Version> {
        let c = self.release_re.captures(tag)?;
        Version::parse(&c["version"])
    }

    pub fn candidate_tag(&self, version: Version, rc: u64) -> String {
        self.candidate_template
            .replace("{version}", &version.to_string())
            .replace("{rc}", &rc.to_string())
    }

    pub fn release_tag(&self, version: Version) -> String {
        self.release_template
            .replace("{version}", &version.to_string())
    }

    /// The literal text before `{version}` of the release tag (`acme-v`).
    pub fn release_tag_prefix(&self) -> &str {
        self.release_template
            .split("{version}")
            .next()
            .unwrap_or_default()
    }

    /// The literal text before `{version}` of the candidate tag.
    pub fn candidate_tag_prefix(&self) -> &str {
        self.candidate_template
            .split("{version}")
            .next()
            .unwrap_or_default()
    }

    /// The newest candidate among tag names: the highest `(version, rc)`.
    pub fn newest_candidate<'a>(
        &self,
        tags: impl IntoIterator<Item = &'a str>,
    ) -> Option<(String, Version, u64)> {
        tags.into_iter()
            .filter_map(|t| {
                self.parse_candidate(t)
                    .map(|(v, rc)| (t.to_string(), v, rc))
            })
            .max_by_key(|(_, v, rc)| (*v, *rc))
    }

    /// The highest final tag strictly below `below` (REQ-REL-018): the base
    /// of a candidate's range.
    pub fn previous_release<'a>(
        &self,
        tags: impl IntoIterator<Item = &'a str>,
        below: Option<Version>,
    ) -> Option<(String, Version)> {
        tags.into_iter()
            .filter_map(|t| self.parse_release(t).map(|v| (t.to_string(), v)))
            .filter(|(_, v)| below.is_none_or(|b| *v < b))
            .max_by_key(|(_, v)| *v)
    }

    /// The lowest `N ≥ 1` whose candidate tag does not exist (REQ-REL-008).
    pub fn next_rc<'a>(
        &self,
        version: Version,
        existing: impl IntoIterator<Item = &'a str>,
    ) -> u64 {
        let taken: Vec<u64> = existing
            .into_iter()
            .filter_map(|t| self.parse_candidate(t))
            .filter(|(v, _)| *v == version)
            .map(|(_, rc)| rc)
            .collect();
        (1..).find(|n| !taken.contains(n)).unwrap_or(1)
    }

    /// A release branch of this product: `<prefix><YYYY-MM-DD-HH-MM>-utc`.
    pub fn is_release_branch(&self, name: &str) -> bool {
        name.strip_prefix(&self.branch_prefix)
            .is_some_and(|stamp| BRANCH_STAMP.is_match(stamp))
    }

    /// The branch name for a cut at `now` (UTC, to the minute).
    pub fn release_branch(&self, now: time::OffsetDateTime) -> String {
        let t = now.to_offset(time::UtcOffset::UTC);
        format!(
            "{}{:04}-{:02}-{:02}-{:02}-{:02}-utc",
            self.branch_prefix,
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute()
        )
    }
}

/// The product and version of a tag of the default shape
/// `<product>-v<X.Y.Z>[-rc.<N>]`; the product is whatever precedes the last
/// `-v<semver>` (products have hyphens of their own).
pub fn parse_product_tag(tag: &str) -> Option<(String, Version, Option<u64>)> {
    static TAG: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^(?P<product>[a-z0-9][a-z0-9-]*)-v(?P<version>\d+\.\d+\.\d+)(?:-rc\.(?P<rc>\d+))?$",
        )
        .unwrap()
    });
    let c = TAG.captures(tag)?;
    Some((
        c["product"].to_string(),
        Version::parse(&c["version"])?,
        c.name("rc").and_then(|m| m.as_str().parse().ok()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn shape(extra: &str) -> Shape {
        let text = format!(
            "schema = 1\n[product]\nname = \"acme\"\nrepository = \"example-org/acme\"\n[owner]\ntracker_user = \"o\"\n\
             [tracker]\nkind = \"linear\"\nteam = \"ABC\"\nservice_identity = \"s\"\n\
             [release]\ncoordinators = [\"c\"]\n{extra}\n[[release.angles]]\nkey = \"security\"\n\
             [review]\ncoordinators = [\"c\"]\ndiff_size_extra_angle = 0\n[[review.angles]]\nkey = \"m\"\n"
        );
        let c = Config::parse(&text).unwrap();
        Shape::of(&c.product.name, &c.release)
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_order_numerically() {
        assert!(v("0.4.118") > v("0.4.99"));
        assert_eq!(v("v1.2.3"), v("1.2.3"));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3-rc.1"), None);
    }

    #[test]
    fn parses_candidates_and_names_the_closing_tag() {
        let s = shape("");
        assert_eq!(s.parse_candidate("acme-v1.2.3-rc.4"), Some((v("1.2.3"), 4)));
        assert_eq!(s.parse_candidate("acme-v1.2.3"), None);
        assert_eq!(s.parse_candidate("other-v1.2.3-rc.1"), None);
        assert_eq!(s.release_tag(v("1.2.3")), "acme-v1.2.3");
        assert_eq!(s.candidate_tag(v("1.2.3"), 2), "acme-v1.2.3-rc.2");
    }

    #[test]
    fn the_release_tag_prefix_is_the_template_before_version() {
        let other =
            shape("candidate_tag = \"v{version}-rc.{rc}\"\nrelease_tag = \"app-v{version}\"");
        assert_eq!(other.release_tag_prefix(), "app-v");
        assert_eq!(other.candidate_tag_prefix(), "v");
        assert_eq!(
            other.parse_candidate("v1.36.3-rc.1"),
            Some((v("1.36.3"), 1))
        );
        assert_eq!(shape("").release_tag_prefix(), "acme-v");
    }

    #[test]
    fn the_newest_candidate_is_the_highest_version_then_rc() {
        let s = shape("");
        let tags = [
            "acme-v1.2.3-rc.1",
            "acme-v1.2.3-rc.10",
            "acme-v1.2.2-rc.9",
            "acme-v1.2.3",
            "other-v9.9.9-rc.1",
        ];
        assert_eq!(
            s.newest_candidate(tags).map(|(t, ..)| t).as_deref(),
            Some("acme-v1.2.3-rc.10")
        );
    }

    #[test]
    fn the_previous_release_is_strictly_below_the_candidate() {
        let s = shape("");
        let tags = [
            "acme-v1.2.2",
            "acme-v1.2.3",
            "acme-v1.1.0",
            "acme-v1.2.3-rc.1",
        ];
        assert_eq!(
            s.previous_release(tags, Some(v("1.2.3")))
                .map(|(t, _)| t)
                .as_deref(),
            Some("acme-v1.2.2"),
            "re-running on a candidate whose own final tag exists still reads its cycle"
        );
        assert_eq!(
            s.previous_release(tags, None).map(|(t, _)| t).as_deref(),
            Some("acme-v1.2.3")
        );
        assert_eq!(s.previous_release(["acme-v1.0.0"], Some(v("1.0.0"))), None);
    }

    #[test]
    fn rc_numbering_takes_the_lowest_free_number() {
        let s = shape("");
        assert_eq!(
            s.next_rc(
                v("1.2.3"),
                ["acme-v1.2.3-rc.1", "acme-v1.2.3-rc.2", "acme-v1.2.4-rc.5"]
            ),
            3
        );
        assert_eq!(s.next_rc(v("1.2.3"), ["acme-v1.2.3-rc.2"]), 1);
        assert_eq!(s.next_rc(v("1.2.3"), Vec::<&str>::new()), 1);
    }

    #[test]
    fn release_branches_are_utc_to_the_minute() {
        let s = shape("");
        assert!(s.is_release_branch("release/2026-10-05-14-03-utc"));
        assert!(
            !s.is_release_branch("release/2026-10-05"),
            "the retired shape is refused"
        );
        assert!(!s.is_release_branch("release/2026-10-05-14-03"));
        assert!(!s.is_release_branch("main"));
        let now = time::macros::datetime!(2026-10-05 10:03:59 -4);
        assert_eq!(s.release_branch(now), "release/2026-10-05-14-03-utc");
    }

    #[test]
    fn a_product_with_hyphens_of_its_own_parses() {
        assert_eq!(
            parse_product_tag("twodo-reminders-sync-v1.3.1"),
            Some(("twodo-reminders-sync".into(), v("1.3.1"), None))
        );
        assert_eq!(
            parse_product_tag("acme-v1.2.3-rc.4"),
            Some(("acme".into(), v("1.2.3"), Some(4)))
        );
        assert_eq!(parse_product_tag("v1.2.3"), None);
    }
}
