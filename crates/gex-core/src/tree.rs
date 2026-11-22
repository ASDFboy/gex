//! The live object tree: what the editor edits and what a running game simulates.

use glam::Mat4;
use slotmap::SlotMap;

use crate::object::{Object, ObjectId};

#[derive(Clone, Default)]
pub struct BlueprintTree {
    pub objects: SlotMap<ObjectId, Object>,
    pub root: Option<ObjectId>,
    /// Goes up whenever objects are added, removed or moved (so lists made from the tree,
    /// like which objects get an event, know when to be made again).
    pub rev: u64,
    kinds: KindCache,
}

/// Objects of some kinds, in tree order, kept until the tree changes (see `of_kinds`).
#[derive(Default)]
struct KindCache(std::sync::Mutex<(u64, std::collections::HashMap<u64, std::sync::Arc<Vec<ObjectId>>>)>);

impl Clone for KindCache {
    fn clone(&self) -> Self {
        KindCache::default()
    }
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
        self.rev += 1;
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
        let siblings = &self.objects[parent].children;
        if !siblings.iter().any(|c| *c != id && self.objects[*c].name == base) {
            return base;
        }
        // Taken: the first free "<stem>N". The numbers in use are gathered in one look at the
        // siblings (trying each number in turn against every sibling slows right down when a
        // game spawns hundreds of the same thing).
        let stem = base.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
        let used: std::collections::HashSet<u64> = siblings
            .iter()
            .filter(|c| **c != id)
            .filter_map(|c| self.objects[*c].name.strip_prefix(stem.as_str()))
            .filter(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) && !rest.starts_with('0'))
            .filter_map(|rest| rest.parse().ok())
            .collect();
        let n = (2..).find(|i| !used.contains(i)).unwrap();
        format!("{}{}", stem, n)
    }

    /// Removes an object and everything under it. Gives back the removed ids.
    pub fn remove(&mut self, id: ObjectId) -> Vec<ObjectId> {
        let mut gone = Vec::new();
        if !self.contains(id) {
            return gone;
        }
        self.rev += 1;
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
        self.rev += 1;
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
    /// The objects of these kinds, in tree order (as `walk` gives them). Kept until objects
    /// are added, removed or moved, so systems that look for one kind every frame (particles,
    /// animations, cameras, sounds) don't walk the whole tree each time.
    pub fn of_kinds(&self, kinds: &[crate::object::ObjectKind]) -> std::sync::Arc<Vec<ObjectId>> {
        let mask = kinds.iter().fold(0u64, |m, k| m | 1 << (*k as u32));
        let mut c = self.kinds.0.lock().unwrap();
        if c.0 != self.rev {
            c.1.clear();
            c.0 = self.rev;
        }
        if let Some(v) = c.1.get(&mask) {
            return v.clone();
        }
        let v = std::sync::Arc::new(self.walk().into_iter().filter(|id| kinds.contains(&self.objects[*id].kind)).collect::<Vec<_>>());
        c.1.insert(mask, v.clone());
        v
    }

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
