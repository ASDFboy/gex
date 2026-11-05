//! Tile grids and sprite-sheet animations, stored as plain text properties so they show
//! (and diff) nicely in scene files.

/// "0,1,1,-1\n2,2,2,2" → rows of tile ids (-1 = empty). Blank or "." cells are empty too.
pub fn parse(text: &str) -> Vec<Vec<i32>> {
    text.split(['\n', '|'])
        .map(|row| {
            let row = row.trim();
            if row.is_empty() {
                return Vec::new();
            }
            row.split([',', ' ']).filter(|c| !c.is_empty()).map(|c| c.trim().parse::<i32>().unwrap_or(-1)).collect()
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .skip_while(|r| r.is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

pub fn to_text(grid: &[Vec<i32>]) -> String {
    let mut rows: Vec<String> = grid.iter().map(|r| r.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")).collect();
    while rows.last().is_some_and(|r| r.split(',').all(|c| c == "-1" || c.is_empty())) {
        rows.pop();
    }
    rows.join("\n")
}

pub fn get(grid: &[Vec<i32>], col: i32, row: i32) -> i32 {
    if col < 0 || row < 0 {
        return -1;
    }
    grid.get(row as usize).and_then(|r| r.get(col as usize)).copied().unwrap_or(-1)
}

/// Sets a cell, growing the grid as needed. Negative positions are ignored.
pub fn set(grid: &mut Vec<Vec<i32>>, col: i32, row: i32, id: i32) {
    if col < 0 || row < 0 {
        return;
    }
    let (c, r) = (col as usize, row as usize);
    while grid.len() <= r {
        grid.push(Vec::new());
    }
    let line = &mut grid[r];
    while line.len() <= c {
        line.push(-1);
    }
    line[c] = id;
}

/// Which tile ids are solid: "" = every tile; else "1,2,5-9".
pub fn solid_set(spec: &str) -> Option<Vec<i32>> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for part in spec.split([',', ' ']).filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<i32>(), b.trim().parse::<i32>()) {
                    out.extend(a.min(b)..=a.max(b));
                }
            }
            None => {
                if let Ok(v) = part.parse() {
                    out.push(v);
                }
            }
        }
    }
    Some(out)
}

/// The frames of a named animation in "idle=0; walk=1-4; jump=5,6,7".
pub fn animation_frames(spec: &str, name: &str) -> Option<Vec<u32>> {
    for part in spec.split([';', '\n']) {
        let Some((n, frames)) = part.split_once('=') else { continue };
        if n.trim() != name {
            continue;
        }
        let ids = solid_set(frames)?;
        return Some(ids.into_iter().filter(|f| *f >= 0).map(|f| f as u32).collect());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_and_animations() {
        let mut g = parse("0,1,-1\n\n2, 3\n");
        assert_eq!(g, vec![vec![0, 1, -1], vec![], vec![2, 3]]);
        set(&mut g, 4, 3, 7);
        assert_eq!(get(&g, 4, 3), 7);
        assert_eq!(get(&g, 9, 9), -1);
        assert_eq!(parse(&to_text(&g)), g);
        assert_eq!(animation_frames("idle=0; walk=1-4; jump = 5,7", "walk"), Some(vec![1, 2, 3, 4]));
        assert_eq!(animation_frames("idle=0", "run"), None);
        assert_eq!(solid_set(""), None);
        assert_eq!(solid_set("1, 3-4"), Some(vec![1, 3, 4]));
    }
}
