//! A small Vim: normal and insert modes over a text buffer and a cursor (a char index).
//! Pure text logic, so the script editor can feed it keys and it can be tested alone.

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
}

#[derive(Default, Debug)]
pub struct Vim {
    pub mode: Mode,
    /// A key waiting for its second half ("d" of "dd", "g" of "gg").
    pending: Option<char>,
    /// The last yank: text, and whether it was whole lines.
    register: (String, bool),
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
}

/// What a key did, for the editor to act on.
#[derive(PartialEq, Debug)]
pub enum Outcome {
    /// Handled (text and/or cursor may have changed).
    Done,
    /// ":" was pressed: open the command line.
    Command,
}

fn line_bounds(c: &[char], at: usize) -> (usize, usize) {
    let at = at.min(c.len());
    let start = c[..at].iter().rposition(|x| *x == '\n').map(|p| p + 1).unwrap_or(0);
    let end = c[at..].iter().position(|x| *x == '\n').map(|p| at + p).unwrap_or(c.len());
    (start, end)
}

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

impl Vim {
    /// Handles one key in normal mode. `text` and `cursor` are updated in place.
    pub fn key(&mut self, k: char, ctrl: bool, text: &mut String, cursor: &mut usize) -> Outcome {
        let mut c: Vec<char> = text.chars().collect();
        let mut cur = (*cursor).min(c.len());
        let (ls, le) = line_bounds(&c, cur);
        let col = cur - ls;
        let mut changed = false;
        let before = (text.clone(), *cursor);
        if ctrl && k == 'r' {
            if let Some((t, p)) = self.redo.pop() {
                self.undo.push(before);
                *text = t;
                *cursor = p;
            }
            return Outcome::Done;
        }
        if let Some(p) = self.pending.take() {
            match (p, k) {
                ('g', 'g') => cur = 0,
                ('d', 'd') => {
                    let end = if le < c.len() { le + 1 } else { le };
                    let start = if le == c.len() && ls > 0 { ls - 1 } else { ls };
                    self.register = (c[ls..end.min(c.len())].iter().collect::<String>().trim_end_matches('\n').to_string(), true);
                    c.drain(start..end);
                    cur = start.min(c.len());
                    let (s2, _) = line_bounds(&c, cur);
                    cur = s2;
                    changed = true;
                }
                ('y', 'y') => self.register = (c[ls..le].iter().collect(), true),
                ('d', 'w') => {
                    let mut e = cur;
                    while e < c.len() && is_word(c[e]) {
                        e += 1;
                    }
                    while e < c.len() && c[e] == ' ' {
                        e += 1;
                    }
                    if e == cur && e < c.len() {
                        e += 1;
                    }
                    self.register = (c[cur..e].iter().collect(), false);
                    c.drain(cur..e);
                    changed = true;
                }
                ('c', 'w') => {
                    let mut e = cur;
                    while e < c.len() && is_word(c[e]) {
                        e += 1;
                    }
                    c.drain(cur..e.max(cur));
                    changed = true;
                    self.mode = Mode::Insert;
                }
                _ => {}
            }
        } else {
            match k {
                'h' => cur = cur.saturating_sub(1).max(ls),
                'l' => cur = (cur + 1).min(le.saturating_sub(1).max(ls)),
                'j' | 'k' => {
                    let target = if k == 'j' {
                        if le >= c.len() {
                            None
                        } else {
                            Some(le + 1)
                        }
                    } else if ls == 0 {
                        None
                    } else {
                        Some(ls - 1)
                    };
                    if let Some(t) = target {
                        let (s2, e2) = line_bounds(&c, t);
                        cur = (s2 + col).min(e2);
                    }
                }
                'w' => {
                    let mut i = cur;
                    let w = i < c.len() && is_word(c[i]);
                    while i < c.len() && is_word(c[i]) == w && !c[i].is_whitespace() {
                        i += 1;
                    }
                    while i < c.len() && c[i].is_whitespace() {
                        i += 1;
                    }
                    cur = i.min(c.len());
                }
                'b' => {
                    let mut i = cur;
                    while i > 0 && c[i - 1].is_whitespace() {
                        i -= 1;
                    }
                    let w = i > 0 && is_word(c[i - 1]);
                    while i > 0 && is_word(c[i - 1]) == w && !c[i - 1].is_whitespace() {
                        i -= 1;
                    }
                    cur = i;
                }
                'e' => {
                    let mut i = (cur + 1).min(c.len());
                    while i < c.len() && c[i].is_whitespace() {
                        i += 1;
                    }
                    while i + 1 < c.len() && is_word(c[i + 1]) {
                        i += 1;
                    }
                    cur = i.min(c.len());
                }
                '0' => cur = ls,
                '$' => cur = le.saturating_sub(1).max(ls),
                '^' => cur = (ls..le).find(|i| !c[*i].is_whitespace()).unwrap_or(ls),
                'G' => cur = line_bounds(&c, c.len()).0,
                'g' | 'd' | 'y' | 'c' => self.pending = Some(k),
                'x' => {
                    if cur < le {
                        self.register = (c[cur].to_string(), false);
                        c.remove(cur);
                        changed = true;
                    }
                }
                'D' => {
                    self.register = (c[cur..le].iter().collect(), false);
                    c.drain(cur..le);
                    changed = true;
                }
                'p' | 'P' => {
                    let (r, lines) = self.register.clone();
                    if lines {
                        let at = if k == 'p' { if le < c.len() { le + 1 } else { le } } else { ls };
                        let ins: Vec<char> = if k == 'p' && le >= c.len() { format!("\n{}", r).chars().collect() } else { format!("{}\n", r).chars().collect() };
                        let n = ins.len();
                        c.splice(at..at, ins);
                        cur = if k == 'p' && le >= c.len() - n { at + 1 } else { at };
                    } else {
                        let at = if k == 'p' { (cur + 1).min(c.len()) } else { cur };
                        let ins: Vec<char> = r.chars().collect();
                        let n = ins.len();
                        c.splice(at..at, ins);
                        cur = at + n.saturating_sub(1);
                    }
                    changed = true;
                }
                'i' => self.mode = Mode::Insert,
                'a' => {
                    cur = (cur + 1).min(le);
                    self.mode = Mode::Insert;
                }
                'I' => {
                    cur = (ls..le).find(|i| !c[*i].is_whitespace()).unwrap_or(ls);
                    self.mode = Mode::Insert;
                }
                'A' => {
                    cur = le;
                    self.mode = Mode::Insert;
                }
                'o' | 'O' => {
                    let indent: String = c[ls..le].iter().take_while(|x| **x == ' ' || **x == '\t').collect();
                    let at = if k == 'o' { le } else { ls };
                    let ins: Vec<char> = if k == 'o' { format!("\n{}", indent).chars().collect() } else { format!("{}\n", indent).chars().collect() };
                    let n = ins.len();
                    c.splice(at..at, ins);
                    cur = if k == 'o' { at + n } else { at + indent.chars().count() };
                    changed = true;
                    self.mode = Mode::Insert;
                }
                'u' => {
                    if let Some((t, p)) = self.undo.pop() {
                        self.redo.push(before);
                        *text = t;
                        *cursor = p;
                    }
                    return Outcome::Done;
                }
                ':' => return Outcome::Command,
                _ => {}
            }
        }
        if changed || self.mode == Mode::Insert {
            self.undo.push(before.clone());
            self.redo.clear();
        }
        if changed {
            *text = c.iter().collect();
        }
        *cursor = cur.min(text.chars().count());
        Outcome::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(v: &mut Vim, keys: &str, text: &mut String, cur: &mut usize) {
        for k in keys.chars() {
            v.key(k, false, text, cur);
        }
    }

    #[test]
    fn motions_and_edits() {
        let mut v = Vim::default();
        let mut t = "hello world\nsecond line\nthird".to_string();
        let mut c = 0;
        run(&mut v, "w", &mut t, &mut c);
        assert_eq!(c, 6);
        run(&mut v, "j", &mut t, &mut c);
        assert_eq!(c, 18, "same column on the next line");
        run(&mut v, "dd", &mut t, &mut c);
        assert_eq!(t, "hello world\nthird");
        run(&mut v, "p", &mut t, &mut c);
        assert_eq!(t, "hello world\nthird\nsecond line");
        run(&mut v, "u", &mut t, &mut c);
        assert_eq!(t, "hello world\nthird");
        run(&mut v, "gg", &mut t, &mut c);
        assert_eq!(c, 0);
        run(&mut v, "x", &mut t, &mut c);
        assert_eq!(t, "ello world\nthird");
        run(&mut v, "$", &mut t, &mut c);
        assert_eq!(c, 9);
        run(&mut v, "o", &mut t, &mut c);
        assert_eq!(v.mode, Mode::Insert);
        assert_eq!(t, "ello world\n\nthird");
        assert_eq!(c, 11);
        v.mode = Mode::Normal;
        assert_eq!(v.key(':', false, &mut t, &mut c), Outcome::Command);
    }
}
