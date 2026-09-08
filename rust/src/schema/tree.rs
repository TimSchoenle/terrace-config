//! A flat list of dotted key paths, as the tree those paths describe.
//!
//! The Markdown rendering has no use for this: a row is one full path and nothing else. The two
//! renderings that write a *file* have nothing but — a TOML file groups its keys under
//! `[csp.cloudflare]` headers, and a JSON Schema nests one `properties` object per level. One
//! walk shared between them, so the two cannot disagree about which keys belong to which table.

use super::Key;

/// One level of the configuration: the keys directly under a path, and the levels below it.
///
/// Order is first-appearance order at every level, which is declaration order for a schema that
/// came out of one derive. It is not necessarily contiguous order: [`Schema::merge`] can append a
/// second root's `csp.*` keys behind a first root's `github.*` ones, and a TOML file cannot open
/// the `[csp]` table twice. Grouping here is what makes that case render at all.
///
/// [`Schema::merge`]: super::Schema::merge
pub(super) struct Node<'a> {
    /// This level's own path segment. Empty at the root.
    pub(super) segment: &'a str,
    /// The keys that live directly at this level, in the order they were described.
    pub(super) keys: Vec<&'a Key>,
    /// The levels below this one, in the order they were first reached.
    pub(super) children: Vec<Node<'a>>,
}

impl<'a> Node<'a> {
    /// Group `keys` by the path each one carries.
    ///
    /// Split on `.`, because that is what figment splits on: a key whose *name* contains a dot
    /// nests in the loader exactly as it nests here, and a renderer that treated the two
    /// differently would describe a file the loader reads another way.
    pub(super) fn of(keys: impl IntoIterator<Item = &'a Key>) -> Self {
        let mut root = Self::new("");
        for key in keys {
            let segments: Vec<&str> = key.path.split('.').collect();
            root.insert(&segments, key);
        }
        root
    }

    fn new(segment: &'a str) -> Self {
        Self {
            segment,
            keys: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Place `key` at the end of `segments`, opening the levels on the way.
    fn insert(&mut self, segments: &[&'a str], key: &'a Key) {
        let [head, rest @ ..] = segments else {
            // A path is never empty — `"".split('.')` still yields one segment — so this is
            // unreachable rather than a case with a sensible answer.
            return;
        };
        if rest.is_empty() {
            self.keys.push(key);
            return;
        }
        let existing = self
            .children
            .iter()
            .position(|child| child.segment == *head);
        let index = if let Some(index) = existing {
            index
        } else {
            self.children.push(Self::new(head));
            self.children.len() - 1
        };
        self.children[index].insert(rest, key);
    }

    /// Whether anything at or below this level must be supplied.
    ///
    /// A table is required exactly when it holds a required key, however deep: `github.username`
    /// having no default is what makes the `[github]` table itself mandatory.
    pub(super) fn required(&self) -> bool {
        self.keys.iter().any(|key| key.required) || self.children.iter().any(Self::required)
    }

    /// Whether a level of this name opens below this one.
    ///
    /// The one case where a key and a table collide: `csp` described as a leaf *and* `csp.mode`
    /// described beside it — a field that wanted `#[config(nested)]` and did not get it. TOML
    /// cannot express both, so the renderer has to know.
    pub(super) fn opens(&self, segment: &str) -> bool {
        self.children.iter().any(|child| child.segment == segment)
    }
}

/// The last segment of a dotted path — the key's own name, as a file spells it.
pub(super) fn name(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}
