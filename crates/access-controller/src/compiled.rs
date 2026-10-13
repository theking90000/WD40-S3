use policy_registry::{Effect, PolicySnapshot, ResourcePattern};

use crate::{Action, Actions, CompilationLimits, Decision, Error, Revision};

#[derive(Clone, Copy, Default)]
struct Permissions {
    allow: Actions,
    deny: Actions,
}

impl Permissions {
    fn add(&mut self, effect: Effect, actions: Actions) {
        match effect {
            Effect::Allow => self.allow |= actions,
            Effect::Deny => self.deny |= actions,
        }
    }

    fn merge(&mut self, other: Self) {
        self.allow |= other.allow;
        self.deny |= other.deny;
    }

    fn authorize(self, action: Action) -> Decision {
        if self.deny.contains(action) || !self.allow.contains(action) {
            Decision::Deny
        } else {
            Decision::Allow
        }
    }
}

/// Byte ranges can split a UTF-8 character. They are never sliced as strings.
#[derive(Clone, Copy)]
struct Label {
    source: usize,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy)]
struct Edge {
    first: u8,
    child: usize,
}

struct Node {
    label: Option<Label>,
    prefixes: Permissions,
    exact: Permissions,
    children: Vec<Edge>,
}

impl Node {
    fn new(label: Option<Label>) -> Self {
        Self {
            label,
            prefixes: Permissions::default(),
            exact: Permissions::default(),
            children: Vec::new(),
        }
    }
}

/// An immutable compressed radix trie with separate exact and prefix masks.
/// Nodes live in a flat arena, so evaluation and destruction are nonrecursive.
/// Resource buffers are moved from the snapshot; labels reference byte ranges
/// without copying strings when an edge is split.
pub struct CompiledPolicy {
    revision: Revision,
    sources: Vec<Vec<u8>>,
    nodes: Vec<Node>,
}

impl CompiledPolicy {
    /// Consumes a coherent registry snapshot without performing I/O.
    /// Empty policies and action sets are valid. Resource strings are opaque.
    pub fn from_snapshot(
        snapshot: PolicySnapshot,
        limits: CompilationLimits,
    ) -> Result<Self, Error> {
        let mut compiled = Self {
            revision: snapshot.revision,
            sources: Vec::new(),
            nodes: Vec::new(),
        };
        compiled.push_node(Node::new(None), limits)?;
        let mut statements = 0_usize;
        let mut resource_bytes = 0_usize;
        for (_, policy) in snapshot.policies {
            for statement in policy.statements {
                statements = statements
                    .checked_add(1)
                    .filter(|&count| count <= limits.max_statements)
                    .ok_or(Error::CompilationLimit("statements"))?;
                let (key, exact) = match statement.resource {
                    ResourcePattern::Exact(key) => (key, true),
                    ResourcePattern::Prefix(key) => (key, false),
                };
                resource_bytes = resource_bytes
                    .checked_add(key.capacity())
                    .filter(|&count| count <= limits.max_resource_bytes)
                    .ok_or(Error::CompilationLimit("resource bytes"))?;
                if statement.actions.is_empty() {
                    continue;
                }
                let source = compiled.sources.len();
                compiled.sources.try_reserve(1)?;
                compiled.sources.push(key.into_bytes());
                if !compiled.insert(source, statement.effect, statement.actions, exact, limits)? {
                    // Existing labels already represent this rule's resource.
                    let _ = compiled.sources.pop();
                }
            }
        }
        Ok(compiled)
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Evaluates in O(key byte length), with no allocation or I/O.
    /// Every matching parent prefix contributes, regardless of specificity.
    pub fn authorize(&self, action: Action, key: &str) -> Decision {
        let Some(mut node) = self.nodes.first() else {
            return Decision::Deny;
        };
        let mut permissions = node.prefixes;
        let mut remaining = key.as_bytes();
        loop {
            if permissions.deny.contains(action) {
                return Decision::Deny;
            }
            let Some(&first) = remaining.first() else {
                permissions.merge(node.exact);
                return permissions.authorize(action);
            };
            let Ok(position) = node
                .children
                .binary_search_by_key(&first, |edge| edge.first)
            else {
                return permissions.authorize(action);
            };
            let Some(child) = node
                .children
                .get(position)
                .and_then(|edge| self.nodes.get(edge.child))
            else {
                return Decision::Deny;
            };
            let Some(label) = child.label.and_then(|label| self.label_bytes(label)) else {
                return Decision::Deny;
            };
            if label.is_empty() {
                return Decision::Deny;
            }
            let Some(suffix) = remaining.strip_prefix(label) else {
                return permissions.authorize(action);
            };
            remaining = suffix;
            node = child;
            permissions.merge(node.prefixes);
        }
    }

    fn label_bytes(&self, label: Label) -> Option<&[u8]> {
        self.sources.get(label.source)?.get(label.start..label.end)
    }

    fn node(&self, id: usize) -> Result<&Node, Error> {
        self.nodes.get(id).ok_or(Error::Internal("missing node"))
    }

    fn node_mut(&mut self, id: usize) -> Result<&mut Node, Error> {
        self.nodes
            .get_mut(id)
            .ok_or(Error::Internal("missing node"))
    }

    fn push_node(&mut self, node: Node, limits: CompilationLimits) -> Result<usize, Error> {
        if self.nodes.len() >= limits.max_nodes {
            return Err(Error::CompilationLimit("nodes"));
        }
        let id = self.nodes.len();
        self.nodes.try_reserve(1)?;
        self.nodes.push(node);
        Ok(id)
    }

    /// Returns whether a new label references the incoming resource buffer.
    fn insert(
        &mut self,
        source: usize,
        effect: Effect,
        actions: Actions,
        exact: bool,
        limits: CompilationLimits,
    ) -> Result<bool, Error> {
        let length = self
            .sources
            .get(source)
            .ok_or(Error::Internal("missing resource"))?
            .len();
        let mut current = 0;
        let mut offset = 0;
        while offset < length {
            let first = *self
                .sources
                .get(source)
                .and_then(|key| key.get(offset))
                .ok_or(Error::Internal("missing resource byte"))?;
            let position = self
                .node(current)?
                .children
                .binary_search_by_key(&first, |edge| edge.first);
            let position = match position {
                Ok(position) => position,
                Err(position) => {
                    let mut child = Node::new(Some(Label {
                        source,
                        start: offset,
                        end: length,
                    }));
                    Self::add_permissions(&mut child, effect, actions, exact);
                    let child = self.push_node(child, limits)?;
                    let children = &mut self.node_mut(current)?.children;
                    if position > children.len() {
                        return Err(Error::Internal("invalid edge position"));
                    }
                    children.try_reserve(1)?;
                    children.insert(position, Edge { first, child });
                    return Ok(true);
                }
            };
            let child = self
                .node(current)?
                .children
                .get(position)
                .ok_or(Error::Internal("missing edge"))?
                .child;
            let label = self
                .node(child)?
                .label
                .ok_or(Error::Internal("missing label"))?;
            let bytes = self
                .label_bytes(label)
                .ok_or(Error::Internal("invalid label"))?;
            let suffix = self
                .sources
                .get(source)
                .and_then(|key| key.get(offset..))
                .ok_or(Error::Internal("invalid resource offset"))?;
            let common = bytes
                .iter()
                .zip(suffix)
                .take_while(|(left, right)| left == right)
                .count();
            if common == 0 {
                return Err(Error::Internal("edge does not match its first byte"));
            }
            offset = offset
                .checked_add(common)
                .ok_or(Error::Internal("resource offset overflow"))?;
            if common < bytes.len() {
                let split = label
                    .start
                    .checked_add(common)
                    .ok_or(Error::Internal("label offset overflow"))?;
                let first = *self
                    .sources
                    .get(label.source)
                    .and_then(|key| key.get(split))
                    .ok_or(Error::Internal("missing split byte"))?;
                let mut branch = Node::new(Some(Label {
                    end: split,
                    ..label
                }));
                branch.children.try_reserve(1)?;
                branch.children.push(Edge { first, child });
                let branch = self.push_node(branch, limits)?;
                self.node_mut(child)?.label = Some(Label {
                    start: split,
                    ..label
                });
                self.node_mut(current)?
                    .children
                    .get_mut(position)
                    .ok_or(Error::Internal("missing parent edge"))?
                    .child = branch;
                current = branch;
            } else {
                current = child;
            }
        }
        Self::add_permissions(self.node_mut(current)?, effect, actions, exact);
        Ok(false)
    }

    fn add_permissions(node: &mut Node, effect: Effect, actions: Actions, exact: bool) {
        if exact {
            node.exact.add(effect, actions);
        } else {
            node.prefixes.add(effect, actions);
        }
    }
}
