//! Structural diff between two versions of a project's outlines: which
//! declarations were added, removed, renamed, moved, or changed.

use std::collections::HashMap;

use serde::Serialize;

use super::outline::{FileOutline, Symbol, SymbolKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// One file before and after. A side is absent when the file did not exist then.
pub struct FileVersions {
    pub path: String,
    pub status: FileStatus,
    /// Previous path of a renamed file.
    pub from: Option<String>,
    pub before: Option<FileOutline>,
    pub after: Option<FileOutline>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Removed,
    Added,
    Renamed,
    Moved,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    #[serde(rename = "change")]
    pub action: ChangeKind,
    /// Dotted path of the symbol, before the change for a rename or move.
    pub symbol: String,
    pub kind: SymbolKind,
    /// Dotted path after a rename or move.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Destination file of a move to another file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_file: Option<String>,
    /// What changed on a symbol that stayed in place.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
    /// A removal with nothing of the same kind added beside it: the name is
    /// gone, with no candidate replacement.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unreplaced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub file: String,
    pub status: FileStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub changes: Vec<Change>,
}

/// Counts over a whole diff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    pub files: usize,
    pub added: usize,
    pub removed: usize,
    pub renamed: usize,
    pub moved: usize,
    pub changed: usize,
    /// Removals with nothing of the same kind added beside them.
    pub unreplaced: usize,
}

impl Totals {
    pub fn of(files: &[FileDiff]) -> Self {
        let mut totals = Self { files: files.len(), ..Self::default() };
        for change in files.iter().flat_map(|file| &file.changes) {
            match change.action {
                ChangeKind::Added => totals.added += 1,
                ChangeKind::Removed => totals.removed += 1,
                ChangeKind::Renamed => totals.renamed += 1,
                ChangeKind::Moved => totals.moved += 1,
                ChangeKind::Changed => totals.changed += 1,
            }
            totals.unreplaced += usize::from(change.unreplaced);
        }
        totals
    }
}

/// A subtree with no counterpart at its own position on the other side.
struct Orphan<'a> {
    file: usize,
    /// Sibling group the symbol was compared in; a rename stays inside one.
    scope: usize,
    path: String,
    symbol: &'a Symbol,
}

/// Where an added symbol lives.
#[derive(Clone)]
struct Destination {
    file: usize,
    path: String,
    /// The symbol is the root of its added subtree, not a member of one.
    root: bool,
}

/// Names that left and names that arrived, counted across every orphaned subtree.
#[derive(Default)]
struct Moves {
    sources: HashMap<(String, SymbolKind), usize>,
    destinations: HashMap<(String, SymbolKind), Vec<Destination>>,
}

impl Moves {
    fn leave(&mut self, symbol: &Symbol) {
        *self.sources.entry((symbol.name.clone(), symbol.kind)).or_default() += 1;
        for child in &symbol.children {
            self.leave(child);
        }
    }

    fn arrive(&mut self, symbol: &Symbol, file: usize, path: &str, root: bool) {
        self.destinations
            .entry((symbol.name.clone(), symbol.kind))
            .or_default()
            .push(Destination { file, path: path.to_string(), root });
        for child in &symbol.children {
            self.arrive(child, file, &join(path, &child.name), false);
        }
    }

    /// A symbol moved when its name and kind left one place and arrived in one place.
    fn destination(&self, symbol: &Symbol) -> Option<Destination> {
        let key = (symbol.name.clone(), symbol.kind);
        match (self.sources.get(&key), self.destinations.get(&key).map(Vec::as_slice)) {
            (Some(1), Some([destination])) => Some(destination.clone()),
            _ => None,
        }
    }
}

/// Compares every file's outline and returns the files whose declarations
/// changed, plus added, deleted, and renamed files.
pub fn diff_outlines(files: &[FileVersions]) -> Vec<FileDiff> {
    let empty = FileOutline::default();
    let mut differ = Differ {
        changes: vec![Vec::new(); files.len()],
        removed: Vec::new(),
        added: Vec::new(),
        scopes: 0,
    };
    for (index, file) in files.iter().enumerate() {
        let before = file.before.as_ref().unwrap_or(&empty);
        let after = file.after.as_ref().unwrap_or(&empty);
        differ.diff(index, &before.symbols, &after.symbols, "", "");
    }
    // Settling a rename compares the pair's members, which can orphan more symbols.
    while !differ.removed.is_empty() || !differ.added.is_empty() {
        differ.settle(files);
    }

    files
        .iter()
        .zip(differ.changes)
        .filter(|(file, changes)| file.status != FileStatus::Modified || !changes.is_empty())
        .map(|(file, mut changes)| {
            changes.sort_by(|a, b| (a.action, &a.symbol).cmp(&(b.action, &b.symbol)));
            mark_unreplaced(&mut changes);
            FileDiff {
                file: file.path.clone(),
                status: file.status,
                from: file.from.clone(),
                changes,
            }
        })
        .collect()
}

struct Differ<'a> {
    changes: Vec<Vec<Change>>,
    removed: Vec<Orphan<'a>>,
    added: Vec<Orphan<'a>>,
    scopes: usize,
}

impl<'a> Differ<'a> {
    /// Pairs the symbols of one sibling group that stayed in place and orphans the rest.
    fn diff(
        &mut self,
        file: usize,
        old: &'a [Symbol],
        new: &'a [Symbol],
        old_prefix: &str,
        new_prefix: &str,
    ) {
        let scope = self.scopes;
        self.scopes += 1;
        let mut unmatched_new: Vec<&'a Symbol> = new.iter().collect();
        let mut unmatched_old = Vec::new();

        // Same name and kind: the symbol stayed in place.
        for before in old {
            let found = unmatched_new
                .iter()
                .position(|after| after.name == before.name && after.kind == before.kind);
            match found {
                Some(position) => {
                    let after = unmatched_new.remove(position);
                    self.stayed(file, before, after, old_prefix, new_prefix);
                }
                None => unmatched_old.push(before),
            }
        }

        // Same name, another kind: `const run` became `function run`.
        unmatched_old.retain(|before| {
            let Some(position) = unmatched_new.iter().position(|after| after.name == before.name)
            else {
                return true;
            };
            let after = unmatched_new.remove(position);
            self.stayed(file, before, after, old_prefix, new_prefix);
            false
        });

        self.removed.extend(unmatched_old.into_iter().map(|symbol| Orphan {
            file,
            scope,
            path: join(old_prefix, &symbol.name),
            symbol,
        }));
        self.added.extend(unmatched_new.into_iter().map(|symbol| Orphan {
            file,
            scope,
            path: join(new_prefix, &symbol.name),
            symbol,
        }));
    }

    fn stayed(
        &mut self,
        file: usize,
        before: &'a Symbol,
        after: &'a Symbol,
        old_prefix: &str,
        new_prefix: &str,
    ) {
        let old_path = join(old_prefix, &before.name);
        let details = differences(before, after);
        if !details.is_empty() {
            self.push(file, ChangeKind::Changed, &old_path, after.kind, None, None, details);
        }
        let new_path = join(new_prefix, &after.name);
        self.diff(file, &before.children, &after.children, &old_path, &new_path);
    }

    /// Settles the current orphans as moved, renamed, removed, or added.
    fn settle(&mut self, files: &[FileVersions]) {
        let removed = std::mem::take(&mut self.removed);
        let added = std::mem::take(&mut self.added);

        let mut moves = Moves::default();
        for orphan in &removed {
            moves.leave(orphan.symbol);
        }
        for orphan in &added {
            moves.arrive(orphan.symbol, orphan.file, &orphan.path, true);
        }

        let mut arrived: Vec<(usize, String)> = Vec::new();
        let mut unmoved = Vec::new();
        for orphan in &removed {
            let Some(destination) = moves.destination(orphan.symbol) else {
                unmoved.push(orphan);
                continue;
            };
            self.push_move(orphan.file, &orphan.path, orphan.symbol.kind, &destination, files);
            if destination.root {
                arrived.push((destination.file, destination.path));
            }
        }
        let unarrived: Vec<&Orphan<'a>> = added
            .iter()
            .filter(|orphan| !arrived.contains(&(orphan.file, orphan.path.clone())))
            .collect();

        // A rename pairs the only two look-alikes of a kind within one sibling group.
        let alike = |before: &Orphan<'_>, after: &Orphan<'_>| {
            before.scope == after.scope
                && before.symbol.kind == after.symbol.kind
                && looks_renamed(before.symbol, after.symbol)
        };
        let mut renamed_to: Vec<&Orphan<'a>> = Vec::new();
        for before in unmoved {
            let candidates: Vec<&&Orphan<'a>> =
                unarrived.iter().filter(|after| alike(before, after)).collect();
            let rename = match candidates.as_slice() {
                [after] if removed.iter().filter(|other| alike(other, after)).count() == 1 => {
                    Some(**after)
                }
                _ => None,
            };
            let Some(after) = rename else {
                // Members of a removed symbol may still have moved elsewhere.
                for child in &before.symbol.children {
                    self.moved_descendants(child, before.file, &before.path, &moves, files);
                }
                let kind = before.symbol.kind;
                self.push(
                    before.file,
                    ChangeKind::Removed,
                    &before.path,
                    kind,
                    None,
                    None,
                    Vec::new(),
                );
                continue;
            };
            renamed_to.push(after);
            let details = differences(before.symbol, after.symbol);
            let to = Some(after.path.clone());
            self.push(
                before.file,
                ChangeKind::Renamed,
                &before.path,
                before.symbol.kind,
                to,
                None,
                details,
            );
            self.diff(
                before.file,
                &before.symbol.children,
                &after.symbol.children,
                &before.path,
                &after.path,
            );
        }

        for orphan in unarrived {
            if !renamed_to.iter().any(|renamed| std::ptr::eq(*renamed, orphan)) {
                let kind = orphan.symbol.kind;
                self.push(
                    orphan.file,
                    ChangeKind::Added,
                    &orphan.path,
                    kind,
                    None,
                    None,
                    Vec::new(),
                );
            }
        }
    }

    fn moved_descendants(
        &mut self,
        symbol: &Symbol,
        file: usize,
        parent: &str,
        moves: &Moves,
        files: &[FileVersions],
    ) {
        let path = join(parent, &symbol.name);
        if let Some(destination) = moves.destination(symbol) {
            self.push_move(file, &path, symbol.kind, &destination, files);
            return;
        }
        for child in &symbol.children {
            self.moved_descendants(child, file, &path, moves, files);
        }
    }

    fn push_move(
        &mut self,
        file: usize,
        path: &str,
        kind: SymbolKind,
        destination: &Destination,
        files: &[FileVersions],
    ) {
        let to_file = (destination.file != file).then(|| files[destination.file].path.clone());
        let to = Some(destination.path.clone());
        self.push(file, ChangeKind::Moved, path, kind, to, to_file, Vec::new());
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        file: usize,
        action: ChangeKind,
        symbol: &str,
        kind: SymbolKind,
        to: Option<String>,
        to_file: Option<String>,
        details: Vec<String>,
    ) {
        self.changes[file].push(Change {
            action,
            symbol: symbol.to_string(),
            kind,
            to,
            to_file,
            details,
            unreplaced: false,
        });
    }
}

/// Marks each removal that has no addition of the same kind in the same place.
/// A removal with one beside it may be an unproven rename; one without is gone.
fn mark_unreplaced(changes: &mut [Change]) {
    let owner = |path: &str| path.rsplit_once('.').map_or("", |(owner, _)| owner).to_string();
    let added: Vec<(String, SymbolKind)> = changes
        .iter()
        .filter(|change| change.action == ChangeKind::Added)
        .map(|change| (owner(&change.symbol), change.kind))
        .collect();
    for change in changes.iter_mut().filter(|change| change.action == ChangeKind::Removed) {
        change.unreplaced = !added.contains(&(owner(&change.symbol), change.kind));
    }
}

/// A rename is claimed only with evidence beyond "one went, one came": mostly
/// the same members, the same signature for callables, or the same producer
/// for plain values. A bare `()` signature also needs the same length.
fn looks_renamed(before: &Symbol, after: &Symbol) -> bool {
    if !before.children.is_empty() || !after.children.is_empty() {
        let shared = before
            .children
            .iter()
            .filter(|child| after.children.iter().any(|other| other.name == child.name))
            .count();
        return shared > 0 && shared * 2 >= before.children.len().max(after.children.len());
    }
    if before.kind.is_callable() {
        let same_length = before.end_line - before.line == after.end_line - after.line;
        return before.signature.is_some()
            && before.signature == after.signature
            && (before.signature.as_deref() != Some("()") || same_length);
    }
    before.via.is_some() && before.via == after.via
}

fn differences(before: &Symbol, after: &Symbol) -> Vec<String> {
    let mut details = Vec::new();
    if before.kind != after.kind {
        details.push(format!("{} → {}", before.kind.as_str(), after.kind.as_str()));
    }
    match (before.exported, after.exported) {
        (false, true) => details.push("now exported".to_string()),
        (true, false) => details.push("no longer exported".to_string()),
        _ => {}
    }
    if before.signature != after.signature
        && let (Some(old), Some(new)) = (&before.signature, &after.signature)
    {
        let (old, new) = changed_part(old, new);
        details.push(format!("signature {old} → {new}"));
    }
    if before.exposed_as != after.exposed_as {
        let keys = |keys: &[String]| {
            if keys.is_empty() { "its own name".to_string() } else { keys.join(", ") }
        };
        details.push(format!(
            "exposed as {} → {}",
            keys(&before.exposed_as),
            keys(&after.exposed_as)
        ));
    }
    let name = |value: &Option<String>| value.clone().unwrap_or_else(|| "none".to_string());
    if before.via != after.via {
        details.push(format!("made by {} → {}", name(&before.via), name(&after.via)));
    }
    if before.extends != after.extends {
        details.push(format!("extends {} → {}", name(&before.extends), name(&after.extends)));
    }
    details
}

/// Signatures longer than this are cut down to the part that changed.
const WHOLE_SIGNATURE_LIMIT: usize = 60;

/// The differing middle of two long signatures, widened to whole names and
/// marked with `…` where text was cut. Short signatures are returned whole.
fn changed_part(old: &str, new: &str) -> (String, String) {
    let (old_chars, new_chars): (Vec<char>, Vec<char>) =
        (old.chars().collect(), new.chars().collect());
    if old_chars.len().max(new_chars.len()) <= WHOLE_SIGNATURE_LIMIT {
        return (old.to_string(), new.to_string());
    }
    let shortest = old_chars.len().min(new_chars.len());
    let mut prefix =
        old_chars.iter().zip(&new_chars).take_while(|(left, right)| left == right).count();
    let mut suffix = old_chars
        .iter()
        .rev()
        .zip(new_chars.iter().rev())
        .take(shortest - prefix)
        .take_while(|(left, right)| left == right)
        .count();
    // Keep whole names: `AiGenerate` → `AiGenerateParams`, not `` → `Params`.
    let in_name =
        |character: char| character.is_alphanumeric() || matches!(character, '_' | '$' | '.');
    while prefix > 0 && in_name(old_chars[prefix - 1]) {
        prefix -= 1;
    }
    while suffix > 0 && in_name(old_chars[old_chars.len() - suffix]) {
        suffix -= 1;
    }
    let cut = |chars: &[char]| {
        let middle: String = chars[prefix..chars.len() - suffix].iter().collect();
        if middle.trim().is_empty() {
            return "…".to_string();
        }
        format!(
            "{}{}{}",
            if prefix > 0 { "…" } else { "" },
            middle.trim(),
            if suffix > 0 { "…" } else { "" }
        )
    };
    (cut(&old_chars), cut(&new_chars))
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_string() } else { format!("{prefix}.{name}") }
}

#[cfg(test)]
#[path = "outline_diff_test.rs"]
mod tests;
