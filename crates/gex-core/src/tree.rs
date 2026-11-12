//! The live object tree: what the editor edits and what a running game simulates.

use glam::Mat4;
use slotmap::SlotMap;

use crate::object::{Object, ObjectId};

#[derive(Clone, Default)]
pub struct BlueprintTree {
    pub objects: SlotMap<ObjectId, Object>,
    pub root: Option<ObjectId>,
}

impl BlueprintTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }
    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.get_mut(id)
    }
    pub fn contains(&self, id: ObjectId) -> bool {
        self.objects.contains_key(id)
    }

    /// Adds `object` under `parent` (or as the root when there's no root yet / no parent).
    pub fn add(&mut self, parent: Option<ObjectId>, mut object: Object) -> ObjectId {
        let parent = parent.filter(|p| self.contains(*p)).or(self.root);
        object.parent = parent;
        object.children.clear();
        let id = self.objects.insert(object);
        match parent {
            Some(p) => {
                let name = self.unique_name(p, id);
                self.objects[id].name = name;
                self.objects[p].children.push(id);
            }
            None => self.root = Some(id),
        }
        id
    }

    /// A sibling-unique version of the object's name ("Enemy", "Enemy2", ...).
    fn unique_name(&self, parent: ObjectId, id: ObjectId) -> String {
        let base = self.objects[id].name.clone();
        let taken = |n: &str| self.objects[parent].children.iter().any(|c| *c != id && self.objects[*c].name == n);
        if !taken(&base) {
            return base;
        }
        let stem = base.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
        (2..).map(|i| format!("{}{}", stem, i)).find(|n| !taken(n)).unwrap()
    }

    /// Removes an object and everything under it. Gives back the removed ids.
    pub fn remove(&mut self, id: ObjectId) -> Vec<ObjectId> {
        let mut gone = Vec::new();
        if !self.contains(id) {
            return gone;
        }
        if let Some(p) = self.objects[id].parent {
            if let Some(pn) = self.objects.get_mut(p) {
                pn.children.retain(|c| *c != id);
            }
        }
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(object) = self.objects.remove(n) {
                stack.extend(object.children);
                gone.push(n);
            }
        }
        if self.root == Some(id) {
            self.root = None;
        }
        gone
    }

    /// Moves `id` under `new_parent` at `index` (end when None). Refuses to move an object
    /// into its own subtree.
    pub fn reparent(&mut self, id: ObjectId, new_parent: ObjectId, index: Option<usize>) -> bool {
        if id == new_parent || !self.contains(id) || !self.contains(new_parent) || self.is_ancestor(id, new_parent) {
            return false;
        }
        if let Some(p) = self.objects[id].parent {
            self.objects[p].children.retain(|c| *c != id);
        }
        let kids = &mut self.objects[new_parent].children;
        let at = index.unwrap_or(kids.len()).min(kids.len());
        kids.insert(at, id);
        self.objects[id].parent = Some(new_parent);
        let name = self.unique_name(new_parent, id);
        self.objects[id].name = name;
        true
    }

    /// Whether `a` is above `b` in the tree.
    pub fn is_ancestor(&self, a: ObjectId, b: ObjectId) -> bool {
        let mut cur = self.objects.get(b).and_then(|n| n.parent);
        while let Some(c) = cur {
            if c == a {
                return true;
            }
            cur = self.objects.get(c).and_then(|n| n.parent);
        }
        false
    }

    /// Every object, parents before children, in sibling order.
    pub fn walk(&self) -> Vec<ObjectId> {
        let mut out = Vec::with_capacity(self.objects.len());
        if let Some(r) = self.root {
            self.walk_from(r, &mut out);
        }
        out
    }
    pub fn walk_from(&self, id: ObjectId, out: &mut Vec<ObjectId>) {
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(object) = self.objects.get(n) {
                out.push(n);
                stack.extend(object.children.iter().rev());
            }
        }
    }

    pub fn depth(&self, id: ObjectId) -> usize {
        let mut d = 0;
        let mut cur = self.objects.get(id).and_then(|n| n.parent);
        while let Some(c) = cur {
            d += 1;
            cur = self.objects.get(c).and_then(|n| n.parent);
        }
        d
    }

    /// "Root/Player/Gun".
    pub fn path_of(&self, id: ObjectId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.objects.get(c) else { break };
            parts.push(n.name.as_str());
            cur = n.parent;
        }
        parts.reverse();
        parts.join("/")
    }

    /// Finds an object by path. Absolute paths start with the root's name or "/";
    /// relative ones start from `from` and may use ".." to go up.
    pub fn find_path(&self, from: Option<ObjectId>, path: &str) -> Option<ObjectId> {
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
            if parts.peek() == Some(&self.objects[root].name.as_str()) {
                parts.next();
            }
        }
        for p in parts {
            cur = if p == ".." {
                self.objects[cur].parent?
            } else {
                *self.objects[cur].children.iter().find(|c| self.objects[**c].name == p)?
            };
        }
        Some(cur)
    }

    /// The first object with this name anywhere in the tree.
    pub fn find_named(&self, name: &str) -> Option<ObjectId> {
        self.walk().into_iter().find(|id| self.objects[*id].name == name)
    }

    pub fn in_group(&self, group: &str) -> Vec<ObjectId> {
        self.walk().into_iter().filter(|id| self.objects[*id].groups.iter().any(|g| g == group)).collect()
    }

    /// The object's transform combined with all of its parents'.
    pub fn global_matrix(&self, id: ObjectId) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.objects.get(c) else { break };
            m = n.transform.matrix() * m;
            cur = n.parent;
        }
        m
    }

    /// Whether the object and all of its parents are visible.
    pub fn visible_in_tree(&self, id: ObjectId) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.objects.get(c) else { return false };
            if !n.visible {
                return false;
            }
            cur = n.parent;
        }
        true
    }

    /// Copies a subtree (from this or another tree) under `parent`. Gives the new root.
    pub fn copy_subtree(&mut self, src: &BlueprintTree, id: ObjectId, parent: Option<ObjectId>) -> ObjectId {
        let mut object = src.objects[id].clone();
        let kids = std::mem::take(&mut object.children);
        let new_id = self.add(parent, object);
        for k in kids {
            self.copy_subtree(src, k, Some(new_id));
        }
        new_id
    }
}
