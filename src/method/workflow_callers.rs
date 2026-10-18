//! The jobs of a GitHub workflow that call a reusable workflow, read from
//! the workflow's text: what `doctor` judges of a caller's inputs, and where
//! the bridge finds the version of the method a project is pinned on
//! (`crate::method::pin`). Pure: no network.

/// A job that calls a reusable workflow: its id, the called file's name and
/// the inputs it passes on one line each (`with:`).
#[derive(Debug, PartialEq, Eq)]
pub struct Caller {
    pub job: String,
    /// The called file's name (`asdlc-release-publish.yml`).
    pub called: String,
    /// The `uses` value whole (`<owner>/<repo>/.github/workflows/<file>@<ref>`).
    pub uses: String,
    pub inputs: std::collections::BTreeMap<String, String>,
}

/// The jobs of a workflow that call a reusable workflow (`jobs.<id>.uses`),
/// read line by line: the shape GitHub's own syntax gives a caller (block
/// mappings indented with spaces, `with:` as a block or a flow mapping of
/// plain scalars). An input whose value spans lines is read as empty.
pub fn reusable_callers(text: &str) -> Vec<Caller> {
    // (indent, content) of each line that holds something, comments cut.
    let lines: Vec<(usize, &str)> = text
        .lines()
        .filter_map(|l| {
            let content = strip_comment(l.trim_start_matches(' ')).trim_end();
            (!content.is_empty()).then(|| (l.len() - l.trim_start_matches(' ').len(), content))
        })
        .collect();
    let mut out = Vec::new();
    let Some(start) = lines.iter().position(|(i, c)| *i == 0 && *c == "jobs:") else {
        return out;
    };
    let jobs: Vec<(usize, &str)> = lines[start + 1..]
        .iter()
        .take_while(|(i, _)| *i > 0)
        .copied()
        .collect();
    let Some(job_indent) = jobs.first().map(|(i, _)| *i) else {
        return out;
    };
    let mut at = 0;
    while at < jobs.len() {
        let (indent, content) = jobs[at];
        at += 1;
        if indent != job_indent {
            continue;
        }
        let Some((id, _)) = key_value(content) else {
            continue;
        };
        let body: Vec<(usize, &str)> = jobs[at..]
            .iter()
            .take_while(|(i, _)| *i > job_indent)
            .copied()
            .collect();
        let Some(key_indent) = body.first().map(|(i, _)| *i) else {
            continue;
        };
        let mut uses = None;
        let mut inputs = std::collections::BTreeMap::new();
        let mut k = 0;
        while k < body.len() {
            let (i, c) = body[k];
            k += 1;
            if i != key_indent {
                continue;
            }
            match key_value(c) {
                Some(("uses", v)) => uses = Some(unquote(v).to_string()),
                Some(("with", v)) if v.starts_with('{') => {
                    let flow = v.trim_start_matches('{').trim_end_matches('}');
                    for pair in flow.split(',') {
                        if let Some((key, value)) = key_value(pair.trim()) {
                            inputs.insert(key.to_string(), unquote(value).to_string());
                        }
                    }
                }
                Some(("with", "")) => {
                    let children: Vec<(usize, &str)> = body[k..]
                        .iter()
                        .take_while(|(ci, _)| *ci > key_indent)
                        .copied()
                        .collect();
                    let input_indent = children.first().map_or(0, |(ci, _)| *ci);
                    for (ci, cc) in children {
                        if ci == input_indent {
                            if let Some((key, value)) = key_value(cc) {
                                inputs.insert(key.to_string(), scalar(value));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(uses) = uses {
            let path = uses.split('@').next().unwrap_or_default();
            let called = path.rsplit('/').next().unwrap_or_default().to_string();
            out.push(Caller {
                job: id.to_string(),
                called,
                uses,
                inputs,
            });
        }
        at += body.len();
    }
    out
}

/// `key: value` (a value possibly empty), the key unquoted; `None` for a
/// line that is not a mapping entry (a list item, a scalar's text).
fn key_value(content: &str) -> Option<(&str, &str)> {
    if content.starts_with('-') {
        return None;
    }
    let (key, value) = match content.split_once(": ") {
        Some((k, v)) => (k, v.trim()),
        None => (content.strip_suffix(':')?, ""),
    };
    let key = unquote(key.trim());
    (!key.is_empty() && !key.contains(' ')).then_some((key, value))
}

/// A one-line scalar: a block scalar (`|`, `>`) spans lines and reads empty.
fn scalar(value: &str) -> String {
    if value.starts_with('|') || value.starts_with('>') {
        String::new()
    } else {
        unquote(value).to_string()
    }
}

fn unquote(value: &str) -> &str {
    let v = value.trim();
    for q in ['\'', '"'] {
        if let Some(inner) = v.strip_prefix(q).and_then(|s| s.strip_suffix(q)) {
            return inner;
        }
    }
    v
}

/// The line without its comment: a `#` that starts it or follows a space,
/// outside a quoted scalar (one whose quote opens it, so that an apostrophe
/// in plain text opens nothing).
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut previous = ' ';
    for (i, c) in line.char_indices() {
        let opens = previous.is_whitespace() || matches!(previous, ':' | '{' | ',' | '[');
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if (c == '\'' || c == '"') && opens => quote = Some(c),
            None if c == '#' && previous.is_whitespace() => return &line[..i],
            None => {}
        }
        previous = c;
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_callers_of_a_reusable_workflow_are_read_with_the_inputs_they_pass() {
        let example = include_str!("../../docs/setup/examples/release-publish.yml");
        let callers = reusable_callers(example);
        let jobs: Vec<(&str, &str)> = callers
            .iter()
            .map(|c| (c.job.as_str(), c.called.as_str()))
            .collect();
        // `publish` runs steps: its `- uses:` are actions, not a call.
        assert_eq!(
            jobs,
            [
                ("verify", "asdlc-release-publish.yml"),
                ("close", "asdlc-release-publish.yml")
            ]
        );
        assert_eq!(callers[0].inputs["stage"], "verify");
        assert_eq!(callers[0].inputs["qa-angles"], r#"["security", "product"]"#);
        assert!(!callers[0].inputs.contains_key("gate-workflow"));

        let text = "\
name: x # a comment
on:
  push:
jobs:
  # a comment
  'verify':
    uses: acme/spl-asdlc/.github/workflows/asdlc-release-publish.yml@v1 # pinned
    with:
      gate-workflow: \"pre-release-qa.yml\"   # the gate
      note: it's # a comment after an apostrophe
      long: >-
        gate-workflow: not-an-input.yml
  cut:
    uses: \"./.github/workflows/asdlc-release-cut.yml\"
    with: {action: open, gate-workflow: 'gate.yml'}
  build:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/checkout@v7
      with:
        gate-workflow: a-step.yml
";
        let callers = reusable_callers(text);
        assert_eq!(callers.len(), 2, "{callers:#?}");
        assert_eq!(callers[0].job, "verify");
        assert_eq!(callers[0].called, "asdlc-release-publish.yml");
        assert_eq!(callers[0].inputs["gate-workflow"], "pre-release-qa.yml");
        assert_eq!(callers[0].inputs["note"], "it's");
        assert_eq!(callers[0].inputs["long"], "");
        assert_eq!(callers[1].job, "cut");
        assert_eq!(callers[1].called, "asdlc-release-cut.yml");
        assert_eq!(callers[1].inputs["gate-workflow"], "gate.yml");
        assert_eq!(callers[1].inputs["action"], "open");
        assert!(reusable_callers("on: push\n").is_empty());
    }
}
