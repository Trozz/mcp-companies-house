//! MCP prompts: ready-made requests that walk the model through common jobs with the tools,
//! such as screening a company or looking into a director. Each prompt is a template in
//! `prompts/<name>.txt` with `{argument}` placeholders and `[[if argument: ...]]` sections that
//! are kept only when that argument is given.

use rmcp::model::{Prompt, PromptArgument};
use serde_json::{Map, Value};

/// Longest argument value accepted, in characters.
pub const MAX_ARGUMENT_CHARS: usize = 2000;

/// One argument of a prompt: name, description and whether it is required.
pub struct ArgumentSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

pub struct PromptSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub arguments: &'static [ArgumentSpec],
    pub template: &'static str,
}

const COMPANY: ArgumentSpec = ArgumentSpec {
    name: "company",
    description: "Company name or number, e.g. \"Acme Widgets Ltd\" or 00445790.",
    required: true,
};

pub const PROMPTS: &[PromptSpec] = &[
    PromptSpec {
        name: "overview",
        title: "Company overview",
        description: "A factual summary of one company from the register: status, what it does, filing deadlines, who runs and controls it, charges, insolvency and recent filings. Optionally, a timeline of everything it has filed since a date.",
        arguments: &[
            COMPANY,
            ArgumentSpec {
                name: "since",
                description: "Also set out everything filed on or after this date, as YYYY-MM-DD, e.g. 2025-01-01.",
                required: false,
            },
        ],
        template: include_str!("../prompts/overview.txt"),
    },
    PromptSpec {
        name: "due_diligence",
        title: "Due diligence screen",
        description: "Screen a company before you deal with it: register flags to look into, disqualification searches and other companies for its directors, who controls it, how what it told you compares with the register, and what the register cannot tell you. It never clears a company.",
        arguments: &[
            COMPANY,
            ArgumentSpec {
                name: "context",
                description: "What you are about to do and anything the business has told you about itself, e.g. \"paying a £5,000 deposit; invoice gives 12 High Street, Leeds and director Jane Smith\". Its details are compared with the register.",
                required: false,
            },
        ],
        template: include_str!("../prompts/due_diligence.txt"),
    },
    PromptSpec {
        name: "director_check",
        title: "Director check",
        description: "Look up a director or other company officer: every company they are or were appointed to, what became of those companies, and any matching entry in the disqualified directors register. Takes care to pick the right person among people with the same name.",
        arguments: &[
            ArgumentSpec {
                name: "name",
                description: "The person's full name, e.g. \"Jane Anne Smith\".",
                required: true,
            },
            ArgumentSpec {
                name: "company",
                description: "A company they are or were an officer of, as a name or number. The surest way to pick the right person.",
                required: false,
            },
            ArgumentSpec {
                name: "born",
                description: "Their month and year of birth, e.g. 1975-03 or March 1975. The register shows only the month and year.",
                required: false,
            },
        ],
        template: include_str!("../prompts/director_check.txt"),
    },
    PromptSpec {
        name: "ownership_chain",
        title: "Ownership chain",
        description: "Trace who controls a company through its persons with significant control, following parent companies up to the people at the top, and say where the register's trail stops.",
        arguments: &[COMPANY],
        template: include_str!("../prompts/ownership_chain.txt"),
    },
    PromptSpec {
        name: "read_accounts",
        title: "Read the latest accounts",
        description: "Find and read a company's most recent filed accounts: the key figures for the year and the year before, the audit position and notes such as going concern, and what this type of accounts leaves out. Needs a server using the live API.",
        arguments: &[
            COMPANY,
            ArgumentSpec {
                name: "focus",
                description: "What you most want from the accounts, e.g. \"net assets and cash\" or \"turnover and staff numbers\".",
                required: false,
            },
        ],
        template: include_str!("../prompts/read_accounts.txt"),
    },
    PromptSpec {
        name: "find_companies",
        title: "Find companies",
        description: "List companies by kind of business, place, status, age or words in the name, using the register's advanced search, and show the SIC codes and filters used.",
        arguments: &[ArgumentSpec {
            name: "criteria",
            description: "What to look for, in your own words, e.g. \"active craft breweries in Leeds incorporated since 2022\".",
            required: true,
        }],
        template: include_str!("../prompts/find_companies.txt"),
    },
    PromptSpec {
        name: "name_check",
        title: "Company name check",
        description: "Check whether a name you want for a new company is already taken on the register, which existing companies have similar names and what they do, and what this check does not cover.",
        arguments: &[
            ArgumentSpec {
                name: "names",
                description: "The name you want, e.g. \"Bright Lane Bakery Ltd\", or up to five candidates separated by semicolons.",
                required: true,
            },
            ArgumentSpec {
                name: "business",
                description: "What the new company will do, e.g. \"wedding cakes\", so similarly named companies can be compared with it.",
                required: false,
            },
        ],
        template: include_str!("../prompts/name_check.txt"),
    },
    PromptSpec {
        name: "watchlist",
        title: "Check a list of companies",
        description: "Check up to 20 companies at once for status, strike-off notices and overdue or upcoming accounts and confirmation statements, most urgent first, and optionally what each has filed since a date.",
        arguments: &[
            ArgumentSpec {
                name: "companies",
                description: "Company numbers or names, separated by commas, semicolons or new lines; up to 20.",
                required: true,
            },
            ArgumentSpec {
                name: "since",
                description: "Also list what each company has filed on or after this date, as YYYY-MM-DD.",
                required: false,
            },
        ],
        template: include_str!("../prompts/watchlist.txt"),
    },
];

pub fn find(name: &str) -> Option<&'static PromptSpec> {
    PROMPTS.iter().find(|p| p.name == name)
}

/// The prompts as MCP lists them.
pub fn list() -> Vec<Prompt> {
    PROMPTS
        .iter()
        .map(|p| {
            let arguments = p
                .arguments
                .iter()
                .map(|a| {
                    PromptArgument::new(a.name)
                        .with_description(a.description)
                        .with_required(a.required)
                })
                .collect();
            Prompt::new(p.name, Some(p.description), Some(arguments)).with_title(p.title)
        })
        .collect()
}

/// The prompt's text for `arguments`: optional sections kept only for arguments given, then
/// every placeholder replaced. Inserted values are not read for placeholders themselves.
pub fn render(spec: &PromptSpec, arguments: &Map<String, Value>) -> Result<String, String> {
    let mut values: Vec<(&str, String)> = Vec::new();
    for (name, value) in arguments {
        let Some(arg) = spec.arguments.iter().find(|a| a.name == name) else {
            let known: Vec<&str> = spec.arguments.iter().map(|a| a.name).collect();
            return Err(format!(
                "{} takes no argument {name}; its arguments are {}",
                spec.name,
                known.join(", ")
            ));
        };
        let text = match value {
            Value::String(s) => s.trim().to_string(),
            Value::Null => continue,
            other => other.to_string(),
        };
        if text.chars().count() > MAX_ARGUMENT_CHARS {
            return Err(format!(
                "{name} is limited to {MAX_ARGUMENT_CHARS} characters"
            ));
        }
        if !text.is_empty() {
            values.push((arg.name, text));
        }
    }
    for arg in spec.arguments.iter().filter(|a| a.required) {
        if !values.iter().any(|(n, _)| *n == arg.name) {
            return Err(format!(
                "{} needs {}: {}",
                spec.name, arg.name, arg.description
            ));
        }
    }
    let given = |name: &str| values.iter().any(|(n, _)| *n == name);
    let kept = sections(spec.template, &given);
    Ok(tidy(&fill(&kept, &values)))
}

/// Keeps `[[if name: body]]` as `body` when `name` was given and drops it otherwise.
fn sections(template: &str, given: &dyn Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("[[if ") {
        out.push_str(&rest[..open]);
        let after = &rest[open + "[[if ".len()..];
        let (Some(colon), Some(close)) = (after.find(':'), after.find("]]")) else {
            out.push_str(&rest[open..]);
            return out;
        };
        let name = after[..colon].trim();
        if given(name) {
            out.push_str(after[colon + 1..close].trim_start());
        }
        rest = &after[close + "]]".len()..];
    }
    out.push_str(rest);
    out
}

/// Replaces each `{name}` with its value, in one pass over the template.
fn fill(text: &str, values: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').map(|close| (&after[..close], close)) {
            Some((name, close)) if let Some((_, v)) = values.iter().find(|(n, _)| *n == name) => {
                out.push_str(v);
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Removes the spaces and blank lines dropped sections leave behind.
fn tidy(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let body = line.trim_start();
        let indent = &line[..line.len() - body.len()];
        let mut squeezed = body.trim_end().to_string();
        while squeezed.contains("  ") {
            squeezed = squeezed.replace("  ", " ");
        }
        let line = format!("{indent}{squeezed}");
        if line.trim().is_empty() && out.last().is_some_and(|l| l.trim().is_empty()) {
            continue;
        }
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn every_prompt_renders_with_and_without_its_optional_arguments() {
        for spec in PROMPTS {
            let all: Map<String, Value> = spec
                .arguments
                .iter()
                .map(|a| (a.name.to_string(), json!(format!("<{}>", a.name))))
                .collect();
            let required: Map<String, Value> = spec
                .arguments
                .iter()
                .filter(|a| a.required)
                .map(|a| (a.name.to_string(), json!(format!("<{}>", a.name))))
                .collect();
            for given in [&all, &required] {
                let text = render(spec, given).unwrap();
                for left in ["[[", "]]", "{", "}"] {
                    assert!(
                        !text.contains(left),
                        "{}: {left:?} left in {text}",
                        spec.name
                    );
                }
                for a in spec.arguments {
                    let shown = text.contains(&format!("<{}>", a.name));
                    assert_eq!(
                        shown,
                        given.contains_key(a.name),
                        "{}: argument {} shown {shown}",
                        spec.name,
                        a.name
                    );
                }
                assert!(!text.contains("\n\n\n"), "{}", spec.name);
            }
        }
    }

    #[test]
    fn arguments_are_checked() {
        let spec = find("overview").unwrap();
        assert!(
            render(spec, &args(json!({})))
                .unwrap_err()
                .contains("needs company")
        );
        assert!(render(spec, &args(json!({"company": "  "}))).is_err());
        assert!(
            render(spec, &args(json!({"company": "x", "colour": "red"})))
                .unwrap_err()
                .contains("takes no argument colour")
        );
        let long = "x".repeat(MAX_ARGUMENT_CHARS + 1);
        assert!(render(spec, &args(json!({"company": long}))).is_err());
        // An empty optional argument drops its section.
        let text = render(spec, &args(json!({"company": "00445790", "since": ""}))).unwrap();
        assert!(!text.contains("filed on or after"), "{text}");
    }

    #[test]
    fn values_are_inserted_once_and_not_read_for_placeholders() {
        let spec = find("overview").unwrap();
        let text = render(spec, &args(json!({"company": "{since} [[if since: x]]"}))).unwrap();
        assert!(text.contains("\"{since} [[if since: x]]\""), "{text}");
    }

    #[test]
    fn sections_and_placeholders() {
        let given = |n: &str| n == "a";
        assert_eq!(sections("x [[if a: yes]] [[if b: no]]z", &given), "x yes z");
        assert_eq!(
            fill(
                "{a} and {b} and {c",
                &[("a", "1".into()), ("b", "{a}".into())]
            ),
            "1 and {a} and {c"
        );
        assert_eq!(tidy("a  b \n\n\n\nc\n"), "a b\n\nc");
        assert_eq!(tidy("a\n   - x   y"), "a\n   - x y");
    }

    #[test]
    fn prompts_are_listed_with_their_arguments() {
        let listed = list();
        assert_eq!(listed.len(), PROMPTS.len());
        let dd = listed.iter().find(|p| p.name == "due_diligence").unwrap();
        assert_eq!(dd.title.as_deref(), Some("Due diligence screen"));
        let a = dd.arguments.as_ref().unwrap();
        assert_eq!(a[0].name, "company");
        assert_eq!(a[0].required, Some(true));
        assert_eq!(a[1].required, Some(false));
    }
}
