//! Rendering a generated `derivedConfig` define, for the cases that are about what it emits.
//!
//! Neither Helm nor a Go template engine is available to this suite, and what a case needs is
//! narrow: the closed dialect [`derived_for`](crate::helm::scaffold::derived_for) and a chart's own
//! context-unwrapping lines are written in. So this evaluates exactly that dialect, with Go's
//! whitespace trimming and truth rules, and panics on anything outside it, so a generator that
//! starts emitting something new fails here loudly rather than rendering as nothing.
//!
//! The dialect: `$name := .field` (unwrapping the define's dict argument); `if`, `with` and `end`;
//! the functions `or`, `not`, `eq` and `kindIs "invalid"`; references into the values; and output
//! as `X`, `X | quote`, `. | quote` or `toYaml X | nindent N`.

use serde_json::Value as Json;

/// One `{{ ... }}` action or one run of text between them, after trimming.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Text(String),
    Action(String),
}

/// The define's body as tokens, with every `{{-` and `-}}` trim applied.
fn tokens(template: &str) -> Vec<Token> {
    let mut found = Vec::new();
    let mut rest = template;
    let mut trim_next = false;
    while let Some(start) = rest.find("{{") {
        let mut text = rest[..start].to_owned();
        if trim_next {
            text = text.trim_start().to_owned();
        }
        let after = &rest[start + 2..];
        let end = after.find("}}").expect("every action is closed");
        let mut action = &after[..end];
        if let Some(trimmed) = action.strip_prefix('-') {
            text = text.trim_end().to_owned();
            action = trimmed;
        }
        trim_next = false;
        if let Some(trimmed) = action.strip_suffix('-') {
            trim_next = true;
            action = trimmed;
        }
        found.push(Token::Text(text));
        found.push(Token::Action(action.trim().to_owned()));
        rest = &after[end + 2..];
    }
    let mut text = rest.to_owned();
    if trim_next {
        text = text.trim_start().to_owned();
    }
    found.push(Token::Text(text));
    found
}

/// What a template is evaluated against.
struct Scope<'a> {
    /// The define's argument: `{"ctx": {"Values": ...}, "service": ...}`.
    argument: &'a Json,
    /// The variables bound so far.
    variables: Vec<(String, Json)>,
    /// `.` inside a `with`, and the argument outside one.
    dot: Vec<Json>,
}

impl Scope<'_> {
    fn dot(&self) -> &Json {
        self.dot.last().unwrap_or(self.argument)
    }

    /// One reference: `.`, `.field.path`, `$name` or `$name.field.path`.
    fn reference(&self, text: &str) -> Json {
        let (base, path) = if let Some(rest) = text.strip_prefix('$') {
            let (name, path) = rest.split_once('.').unwrap_or((rest, ""));
            let bound = self
                .variables
                .iter()
                .rev()
                .find(|(held, _)| held == name)
                .unwrap_or_else(|| panic!("`${name}` is used before it is bound"));
            (bound.1.clone(), path.to_owned())
        } else if text == "." {
            (self.dot().clone(), String::new())
        } else if let Some(path) = text.strip_prefix('.') {
            (self.dot().clone(), path.to_owned())
        } else {
            panic!("`{text}` is not a reference this dialect writes");
        };
        let mut current = base;
        for segment in path.split('.').filter(|segment| !segment.is_empty()) {
            current = match current {
                Json::Object(fields) => fields.get(segment).cloned().unwrap_or(Json::Null),
                // Go's nil pointer evaluation: a field of nothing is an error, not nothing.
                other => panic!("`{text}` reads `{segment}` of {other}"),
            };
        }
        current
    }

    /// One expression: a literal, a reference, or a function applied to its arguments.
    fn evaluate(&self, text: &str) -> Json {
        let words = split(text);
        match words.as_slice() {
            [only] => self.atom(only),
            [function, arguments @ ..] => {
                let values: Vec<Json> = arguments.iter().map(|word| self.atom(word)).collect();
                match (function.as_str(), values.as_slice()) {
                    ("or", [_, _, ..]) => values
                        .iter()
                        .find(|value| truthy(value))
                        .or(values.last())
                        .cloned()
                        .unwrap_or(Json::Null),
                    ("not", [value]) => Json::Bool(!truthy(value)),
                    ("eq", [left, right]) => Json::Bool(left == right),
                    ("kindIs", [Json::String(kind), value]) if kind == "invalid" => {
                        Json::Bool(value.is_null())
                    }
                    _ => panic!("`{text}` is not a call this dialect writes"),
                }
            }
            [] => panic!("an empty expression"),
        }
    }

    fn atom(&self, word: &str) -> Json {
        if let Some(inner) = word
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
        {
            return self.evaluate(inner);
        }
        if let Some(literal) = word
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
        {
            return Json::String(literal.to_owned());
        }
        self.reference(word)
    }

    /// What an output action prints.
    fn output(&self, action: &str) -> String {
        if let Some(rest) = action.strip_prefix("toYaml ") {
            let (value, indent) = rest
                .split_once(" | nindent ")
                .unwrap_or_else(|| panic!("`{action}` is not a `toYaml` this dialect writes"));
            let indent: usize = indent.parse().expect("an indent is a number");
            // JSON is YAML, which is all a case reading the result back needs.
            return format!("\n{}{}", " ".repeat(indent), self.evaluate(value));
        }
        if let Some(value) = action.strip_suffix(" | quote") {
            return match self.evaluate(value) {
                Json::String(text) => Json::String(text).to_string(),
                other => Json::String(printed(&other)).to_string(),
            };
        }
        printed(&self.evaluate(action))
    }
}

/// A value as Go's `{{ }}` prints it. Never `null`: printing an unset value is the defect.
fn printed(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        Json::Null => panic!("an unset value was printed"),
        other => other.to_string(),
    }
}

/// Go's truth: false, zero, and every empty value are false.
fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(held) => *held,
        Json::Number(number) => number.as_f64().is_some_and(|held| held != 0.0),
        Json::String(text) => !text.is_empty(),
        Json::Array(items) => !items.is_empty(),
        Json::Object(fields) => !fields.is_empty(),
    }
}

/// An expression's words, a parenthesised group or a quoted string counting as one.
fn split(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    for character in text.chars() {
        match character {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth -= 1,
            ' ' if !quoted && depth == 0 => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// The body of define `name` in a template file, between its opening and closing actions.
pub(super) fn define_body<'a>(template: &'a str, name: &str) -> &'a str {
    let opening = format!("{{{{- define \"{name}\" -}}}}\n");
    let start = template.find(&opening).expect("the define is there") + opening.len();
    let rest = &template[start..];
    // A generated define closes with `{{- end -}}` on a line of its own, the only such line.
    let end = rest.find("\n{{- end -}}").expect("the define is closed");
    &rest[..=end]
}

/// One open `if` or `with`.
struct Frame {
    /// Whether its body renders.
    renders: bool,
    /// Whether it moved `.`, which only `with` does.
    moved_dot: bool,
}

/// One define body rendered for one service against the chart's values.
pub(super) fn render(body: &str, values: &Json, service: &str) -> String {
    let argument = serde_json::json!({"ctx": {"Values": values}, "service": service});
    let mut scope = Scope {
        argument: &argument,
        variables: Vec::new(),
        dot: Vec::new(),
    };
    let mut open: Vec<Frame> = Vec::new();
    let mut out = String::new();
    for token in tokens(body) {
        let rendering = open.iter().all(|frame| frame.renders);
        match token {
            Token::Text(text) => {
                if rendering {
                    out.push_str(&text);
                }
            }
            Token::Action(action) => {
                if let Some(condition) = action.strip_prefix("if ") {
                    open.push(Frame {
                        renders: rendering && truthy(&scope.evaluate(condition)),
                        moved_dot: false,
                    });
                } else if let Some(value) = action.strip_prefix("with ") {
                    let held = if rendering {
                        scope.evaluate(value)
                    } else {
                        Json::Null
                    };
                    open.push(Frame {
                        renders: rendering && truthy(&held),
                        moved_dot: true,
                    });
                    scope.dot.push(held);
                } else if action == "end" {
                    let frame = open.pop().expect("an `end` closes something");
                    if frame.moved_dot {
                        scope.dot.pop();
                    }
                } else if let Some((name, value)) = action.split_once(":=") {
                    let name = name.trim().strip_prefix('$').expect("a variable");
                    let bound = scope.evaluate(value.trim());
                    scope.variables.push((name.to_owned(), bound));
                } else if rendering {
                    out.push_str(&scope.output(&action));
                }
            }
        }
    }
    assert!(open.is_empty(), "every block is closed");
    out
}

#[test]
fn the_evaluator_follows_go_s_trimming_and_truth() {
    let values = serde_json::json!({"a": {"n": 0, "s": "", "t": "x"}});
    let body = "{{- $ctx := .ctx -}}\n\
                {{- if not (kindIs \"invalid\" $ctx.Values.a.n) }}\nn: {{ $ctx.Values.a.n }}\n{{- end }}\n\
                {{- with $ctx.Values.a.s }}\ns: {{ . | quote }}\n{{- end }}\n\
                {{- with $ctx.Values.a.t }}\nt: {{ . | quote }}\n{{- end }}\n\
                {{- if eq .service \"api\" }}\napi: true\n{{- end }}\n";
    assert_eq!(
        render(body, &values, "api"),
        "\nn: 0\nt: \"x\"\napi: true\n"
    );
    assert_eq!(render(body, &values, "worker"), "\nn: 0\nt: \"x\"\n");
}
