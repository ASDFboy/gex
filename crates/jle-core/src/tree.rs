//! The live node tree: what the editor edits and what a running game simulates.

use glam::Mat4;
use slotmap::SlotMap;

use crate::node::{Node, NodeId};

#[derive(Clone, Default)]
pub struct SceneTree {
    pub nodes: SlotMap<NodeId, Node>,
    pub root: Option<NodeId>,
}

impl SceneTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(id)
    }

    /// Adds `node` under `parent` (or as the root when there's no root yet / no parent).
    pub fn add(&mut self, parent: Option<NodeId>, mut node: Node) -> NodeId {
        let parent = parent.filter(|p| self.contains(*p)).or(self.root);
        node.parent = parent;
        node.children.clear();
        let id = self.nodes.insert(node);
        match parent {
            Some(p) => {
                let name = self.unique_name(p, id);
                self.nodes[id].name = name;
                self.nodes[p].children.push(id);
            }
            None => self.root = Some(id),
        }
        id
    }

    /// A sibling-unique version of the node's name ("Enemy", "Enemy2", ...).
    fn unique_name(&self, parent: NodeId, id: NodeId) -> String {
        let base = self.nodes[id].name.clone();
        let taken = |n: &str| self.nodes[parent].children.iter().any(|c| *c != id && self.nodes[*c].name == n);
        if !taken(&base) {
            return base;
        }
        let stem = base.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
        (2..).map(|i| format!("{}{}", stem, i)).find(|n| !taken(n)).unwrap()
    }

    /// Removes a node and everything under it. Gives back the removed ids.
    pub fn remove(&mut self, id: NodeId) -> Vec<NodeId> {
        let mut gone = Vec::new();
        if !self.contains(id) {
            return gone;
        }
        if let Some(p) = self.nodes[id].parent {
            if let Some(pn) = self.nodes.get_mut(p) {
                pn.children.retain(|c| *c != id);
            }
        }
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(node) = self.nodes.remove(n) {
                stack.extend(node.children);
                gone.push(n);
            }
        }
        if self.root == Some(id) {
            self.root = None;
        }
        gone
    }

    /// Moves `id` under `new_parent` at `index` (end when None). Refuses to move a node
    /// into its own subtree.
    pub fn reparent(&mut self, id: NodeId, new_parent: NodeId, index: Option<usize>) -> bool {
        if id == new_parent || !self.contains(id) || !self.contains(new_parent) || self.is_ancestor(id, new_parent) {
            return false;
        }
        if let Some(p) = self.nodes[id].parent {
            self.nodes[p].children.retain(|c| *c != id);
        }
        let kids = &mut self.nodes[new_parent].children;
        let at = index.unwrap_or(kids.len()).min(kids.len());
        kids.insert(at, id);
        self.nodes[id].parent = Some(new_parent);
        let name = self.unique_name(new_parent, id);
        self.nodes[id].name = name;
        true
    }

    /// Whether `a` is above `b` in the tree.
    pub fn is_ancestor(&self, a: NodeId, b: NodeId) -> bool {
        let mut cur = self.nodes.get(b).and_then(|n| n.parent);
        while let Some(c) = cur {
            if c == a {
                return true;
            }
            cur = self.nodes.get(c).and_then(|n| n.parent);
        }
        false
    }

    /// Every node, parents before children, in sibling order.
    pub fn walk(&self) -> Vec<NodeId> {
        let mut out = Vec::with_capacity(self.nodes.len());
        if let Some(r) = self.root {
            self.walk_from(r, &mut out);
        }
        out
    }
    pub fn walk_from(&self, id: NodeId, out: &mut Vec<NodeId>) {
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(node) = self.nodes.get(n) {
                out.push(n);
                stack.extend(node.children.iter().rev());
            }
        }
    }

    pub fn depth(&self, id: NodeId) -> usize {
        let mut d = 0;
        let mut cur = self.nodes.get(id).and_then(|n| n.parent);
        while let Some(c) = cur {
            d += 1;
            cur = self.nodes.get(c).and_then(|n| n.parent);
        }
        d
    }

    /// "Root/Player/Gun".
    pub fn path_of(&self, id: NodeId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.nodes.get(c) else { break };
            parts.push(n.name.as_str());
            cur = n.parent;
        }
        parts.reverse();
        parts.join("/")
    }

    /// Finds a node by path. Absolute paths start with the root's name or "/";
    /// relative ones start from `from` and may use ".." to go up.
    pub fn find_path(&self, from: Option<NodeId>, path: &str) -> Option<NodeId> {
        let root = self.root?;
        let (mut cur, rest) = if let Some(r) = path.strip_prefix('/') {
            (root, r)
        } else if let Some(f) = from {
            (f, path)
        } else {
            (root, path)
        };
        let mut parts = rest.split('/').filter(|p| !p.is_empty() && *p != ".").peekable();
        // An absolute path may name the root itself first.
        if path.starts_with('/') || from.is_none() {
            if parts.peek() == Some(&self.nodes[root].name.as_str()) {
                parts.next();
            }
        }
        for p in parts {
            cur = if p == ".." {
                self.nodes[cur].parent?
            } else {
                *self.nodes[cur].children.iter().find(|c| self.nodes[**c].name == p)?
            };
        }
        Some(cur)
    }

    /// The first node with this name anywhere in the tree.
    pub fn find_named(&self, name: &str) -> Option<NodeId> {
        self.walk().into_iter().find(|id| self.nodes[*id].name == name)
    }

    pub fn in_group(&self, group: &str) -> Vec<NodeId> {
        self.walk().into_iter().filter(|id| self.nodes[*id].groups.iter().any(|g| g == group)).collect()
    }

    /// The node's transform combined with all of its parents'.
    pub fn global_matrix(&self, id: NodeId) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.nodes.get(c) else { break };
            m = n.transform.matrix() * m;
            cur = n.parent;
        }
        m
    }

    /// Whether the node and all of its parents are visible.
    pub fn visible_in_tree(&self, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.nodes.get(c) else { return false };
            if !n.visible {
                return false;
            }
            cur = n.parent;
        }
        true
    }

    /// Copies a subtree (from this or another tree) under `parent`. Gives the new root.
    pub fn copy_subtree(&mut self, src: &SceneTree, id: NodeId, parent: Option<NodeId>) -> NodeId {
        let mut node = src.nodes[id].clone();
        let kids = std::mem::take(&mut node.children);
        let new_id = self.add(parent, node);
        for k in kids {
            self.copy_subtree(src, k, Some(new_id));
        }
        new_id
    }
}
