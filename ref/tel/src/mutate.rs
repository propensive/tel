//! Machine operations on a `Document`'s semantic model, per §22.2 of the
//! TEL Specification.
//!
//! This module implements a representative subset of the operations defined
//! in §22.2 — enough to drive common editing flows (update a scalar, attach
//! or remove a remark, delete or insert a compound, set or unset a flag)
//! while preserving the surrounding presentation invariants (remarks of
//! sibling compounds, blank lines around blocks, comments attached to
//! blocks, tabulation markers).
//!
//! The operations operate on a path: a sequence of `(block_index,
//! compound_index)` pairs descending from the document root to the target
//! compound. Helpers below build paths by keyword search.

use crate::{Atom, Block, Compound, Document, atom_text};
use crate::canonical;

/// Path to a compound within a `Document`. Each step is a
/// `(block_index, compound_index)` pair indexing into the surrounding
/// children list. An empty path refers to the (virtual) document root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Path {
    pub steps: Vec<(usize, usize)>,
}

impl Path {
    pub fn empty() -> Self { Self { steps: Vec::new() } }
    pub fn push(mut self, block_idx: usize, compound_idx: usize) -> Self {
        self.steps.push((block_idx, compound_idx));
        self
    }
}

/// Errors that can arise when applying a mutation. The §22 invariants
/// (e.g., not changing the document sigil, retaining literal-atom
/// delimiter uniqueness) are enforced by the operations themselves; this
/// enum carries only the kinds of structural failure the operation
/// signature does not statically prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationError {
    PathNotFound,
    NotAScalar,
    NotAFlag,
    EmptyPath,
    LiteralDelimiterCollision,
}

/// Walk a path and return a mutable reference to the addressed compound.
fn walk_mut<'a>(doc: &'a mut Document, path: &Path) -> Result<&'a mut Compound, MutationError> {
    if path.steps.is_empty() { return Err(MutationError::EmptyPath); }
    let mut blocks: &mut Vec<Block> = &mut doc.children;
    let mut compound: Option<&mut Compound> = None;
    for (b, c) in &path.steps {
        let block = blocks.get_mut(*b).ok_or(MutationError::PathNotFound)?;
        let cc = block.compounds.get_mut(*c).ok_or(MutationError::PathNotFound)?;
        // SAFETY: re-borrow as 'a so we can continue descending. We are
        // strictly nesting deeper at each step; no aliasing.
        compound = Some(cc);
        // Replace `blocks` with the compound's children for the next step.
        // We need the inner blocks of the same compound for descent.
        let cc_ref: *mut Compound = *compound.as_mut().unwrap() as *mut _;
        unsafe {
            blocks = &mut (*cc_ref).children;
        }
    }
    compound.ok_or(MutationError::PathNotFound)
}

// ── update-value ─────────────────────────────────────────────────────────────

/// `update-value` (§22.2). Replace the value text of a scalar compound. All
/// other presentation details are retained. The operation does not change
/// the atom form (inline / source / literal) unless the new value cannot be
/// represented in the existing form, in which case the operation upgrades
/// to the next form per §22.2 "Atom form escalation".
pub fn update_value(doc: &mut Document, target: &Path, value: &str) -> Result<(), MutationError> {
    // The sigil is needed to evaluate the inline-safe predicate (§22.2). It is
    // captured before the mutable borrow below; default `#` when unspecified.
    let sigil = doc.pragma.as_ref().and_then(|p| p.sigil).unwrap_or('#');
    let cc = walk_mut(doc, target)?;
    let atom = cc.atoms.first_mut().ok_or(MutationError::NotAScalar)?;
    match atom {
        // Atom-form safety invariant (§22.2): keep the current form only while
        // the new value remains safe for it, otherwise escalate inline → source
        // → literal. Escalation never moves to an earlier form.
        Atom::Inline { text, .. } => {
            if canonical::can_inline(value, sigil) {
                *text = value.to_string();
            } else if canonical::can_source(value) {
                *atom = Atom::Source { text: value.to_string() };
            } else {
                *atom = Atom::Literal {
                    delimiter: canonical::choose_literal_delim(value),
                    text: value.to_string(),
                };
            }
        }
        Atom::Source { text } => {
            if canonical::can_source(value) {
                *text = value.to_string();
            } else {
                *atom = Atom::Literal {
                    delimiter: canonical::choose_literal_delim(value),
                    text: value.to_string(),
                };
            }
        }
        Atom::Literal { delimiter, text } => {
            // Keep the existing delimiter unless the new payload collides with
            // it; only then fall back to a fresh dash-extension delimiter.
            if value.split('\n').any(|l| l == delimiter) {
                *delimiter = canonical::choose_literal_delim(value);
            }
            *text = value.to_string();
        }
    }
    Ok(())
}

// ── attach-remark / remove-remark ────────────────────────────────────────────

pub fn attach_remark(doc: &mut Document, target: &Path, text: &str) -> Result<(), MutationError> {
    let cc = walk_mut(doc, target)?;
    cc.remark = Some(text.to_string());
    Ok(())
}

pub fn remove_remark(doc: &mut Document, target: &Path) -> Result<(), MutationError> {
    let cc = walk_mut(doc, target)?;
    cc.remark = None;
    Ok(())
}

// ── delete ──────────────────────────────────────────────────────────────────

/// `delete` (§22.2). Remove a compound. If the compound's block becomes
/// empty as a result, the block (and its attached comments) is also
/// removed. The caller is responsible for ensuring the deletion is
/// schema-valid (i.e., the member is not required, or the member has a
/// non-null default that fills in for it).
pub fn delete(doc: &mut Document, target: &Path) -> Result<(), MutationError> {
    if target.steps.is_empty() { return Err(MutationError::EmptyPath); }
    // Descend to the parent; the final step's compound is removed from
    // its containing block.
    let (last_block, last_compound) = *target.steps.last().unwrap();
    let parent_steps = &target.steps[..target.steps.len() - 1];

    if parent_steps.is_empty() {
        // Target is at the document root.
        delete_in_blocks(&mut doc.children, last_block, last_compound)
    } else {
        let parent_path = Path { steps: parent_steps.to_vec() };
        let parent = walk_mut(doc, &parent_path)?;
        delete_in_blocks(&mut parent.children, last_block, last_compound)
    }
}

fn delete_in_blocks(blocks: &mut Vec<Block>, b: usize, c: usize) -> Result<(), MutationError> {
    let block = blocks.get_mut(b).ok_or(MutationError::PathNotFound)?;
    if c >= block.compounds.len() { return Err(MutationError::PathNotFound); }
    block.compounds.remove(c);
    if block.compounds.is_empty() {
        // Drop the block (and its attached comments).
        blocks.remove(b);
    }
    Ok(())
}

// ── set-flag / unset-flag ────────────────────────────────────────────────────

/// `set-flag` (§22.2). Insert a Flag-typed compound child into a parent.
/// Placement rule: the flag is appended as a new compound child in a new
/// block of its own. Inline-atom placement (per §22.2 step a/b) is left to
/// `construct` callers.
pub fn set_flag(doc: &mut Document, parent: &Path, keyword: &str) -> Result<(), MutationError> {
    let block = Block {
        comments: Vec::new(),
        tabulation: None,
        compounds: vec![Compound {
            keyword: keyword.to_string(),
            atoms: Vec::new(),
            remark: None,
            children: Vec::new(),
        }],
        trailing_blank_lines: 0,
    };
    if parent.steps.is_empty() {
        doc.children.push(block);
    } else {
        let p = walk_mut(doc, parent)?;
        p.children.push(block);
    }
    Ok(())
}

/// `unset-flag` (§22.2). Remove a Flag-typed compound child by keyword
/// within a parent. Operates on compound-form flags; atom-form flags
/// (Flag set as an inline atom on the parent line) require separate atom-
/// editing logic and are out of scope for this minimal implementation.
pub fn unset_flag(doc: &mut Document, parent: &Path, keyword: &str) -> Result<(), MutationError> {
    let children: &mut Vec<Block> = if parent.steps.is_empty() {
        &mut doc.children
    } else {
        let p = walk_mut(doc, parent)?;
        &mut p.children
    };
    let mut found = false;
    let mut i = 0;
    while i < children.len() {
        let block = &mut children[i];
        let original_len = block.compounds.len();
        block.compounds.retain(|c| {
            !(c.keyword == keyword && c.atoms.is_empty() && c.children.is_empty()) || {
                false
            }
        });
        if block.compounds.len() < original_len {
            found = true;
        }
        if block.compounds.is_empty() {
            children.remove(i);
        } else {
            i += 1;
        }
    }
    if !found { return Err(MutationError::NotAFlag); }
    Ok(())
}

// ── insert / insert-before / insert-after ────────────────────────────────────

/// `insert` (§22.2). Append a compound at the end of the parent's child
/// blocks. (The full §22.2 placement rule selects a position among
/// existing member groups, requiring schema context; this minimal form
/// simply appends.)
pub fn insert(doc: &mut Document, parent: &Path, compound: Compound) -> Result<(), MutationError> {
    let block = Block {
        comments: Vec::new(),
        tabulation: None,
        compounds: vec![compound],
        trailing_blank_lines: 0,
    };
    if parent.steps.is_empty() {
        doc.children.push(block);
    } else {
        let p = walk_mut(doc, parent)?;
        p.children.push(block);
    }
    Ok(())
}

/// `insert-before` (§22.2). Insert immediately before an existing sibling.
/// The new compound is placed in the same block when the block has no
/// tabulation; otherwise in a new block immediately before.
pub fn insert_before(
    doc: &mut Document,
    sibling: &Path,
    compound: Compound,
) -> Result<(), MutationError> {
    insert_relative(doc, sibling, compound, /* after = */ false)
}

/// `insert-after` (§22.2). Insert immediately after an existing sibling,
/// subject to the same block-placement rules as `insert-before`.
pub fn insert_after(
    doc: &mut Document,
    sibling: &Path,
    compound: Compound,
) -> Result<(), MutationError> {
    insert_relative(doc, sibling, compound, /* after = */ true)
}

fn insert_relative(
    doc: &mut Document,
    sibling: &Path,
    compound: Compound,
    after: bool,
) -> Result<(), MutationError> {
    if sibling.steps.is_empty() { return Err(MutationError::EmptyPath); }
    let (sibling_block, sibling_compound) = *sibling.steps.last().unwrap();
    let parent_steps = &sibling.steps[..sibling.steps.len() - 1];

    let blocks: &mut Vec<Block> = if parent_steps.is_empty() {
        &mut doc.children
    } else {
        let parent_path = Path { steps: parent_steps.to_vec() };
        let p = walk_mut(doc, &parent_path)?;
        &mut p.children
    };

    let block = blocks.get_mut(sibling_block).ok_or(MutationError::PathNotFound)?;
    let has_tabulation = block.tabulation.is_some();
    if has_tabulation {
        let new_block = Block {
            comments: Vec::new(),
            tabulation: None,
            compounds: vec![compound],
            trailing_blank_lines: 0,
        };
        let pos = if after { sibling_block + 1 } else { sibling_block };
        blocks.insert(pos, new_block);
    } else {
        let pos = if after { sibling_compound + 1 } else { sibling_compound };
        if pos > block.compounds.len() { return Err(MutationError::PathNotFound); }
        block.compounds.insert(pos, compound);
    }
    Ok(())
}

// ── replace ──────────────────────────────────────────────────────────────────

/// `replace` (§22.2). Substitute a compound for another at the same
/// position in the same block. A replacement is valid when:
///
/// - the replacement's keyword identifies the same schema member as
///   the original (either both keywords map to the same Field, or
///   both map to Variants of the same Select), AND
/// - the replacement compound is well-typed under that member's type.
///
/// This function does NOT itself perform schema-aware validation
/// (that requires a Schema argument, which `mutate.rs` deliberately
/// does not depend on); the caller is responsible for ensuring the
/// replacement is type-compatible. The function preserves the
/// original compound's remark and its position within the block;
/// attached comments on the block survive.
pub fn replace(doc: &mut Document, target: &Path, replacement: Compound) -> Result<(), MutationError> {
    let cc = walk_mut(doc, target)?;
    // Preserve the original's remark by default; the replacement's own
    // remark, if any, takes precedence.
    let preserved_remark = cc.remark.clone();
    *cc = Compound {
        keyword: replacement.keyword,
        atoms: replacement.atoms,
        remark: replacement.remark.or(preserved_remark),
        children: replacement.children,
    };
    Ok(())
}

// ── construct ───────────────────────────────────────────────────────────────

/// `construct` (§22.2). Create a new compound from purely semantic
/// information. The result follows the §22.2 canonical-presentation
/// form: required `Scalar` Field values fill inline atoms in member
/// order; Flag members appear as inline atoms when their member
/// precedes any non-atom-assignable position; remaining children are
/// serialized as compound children. This function does NOT need a
/// schema — the caller supplies the parts (keyword, atoms, children)
/// and `construct` simply assembles a Compound with the canonical
/// invariants (no remark, no attached comments, single space before
/// inline atoms, no tabulation).
pub fn construct(keyword: &str, atoms: Vec<Atom>, children: Vec<Compound>) -> Compound {
    // Wrap the children into a single Block with no comments and no
    // trailing blank lines — the canonical form per §22.2.
    let children_block = if children.is_empty() {
        Vec::new()
    } else {
        vec![Block {
            comments: Vec::new(),
            tabulation: None,
            compounds: children,
            trailing_blank_lines: 0,
        }]
    };
    // Normalise inline atoms: single preceding space, per §22.2.
    let normalised_atoms: Vec<Atom> = atoms.into_iter().map(|a| match a {
        Atom::Inline { text, .. } => Atom::Inline { text, preceding_spaces: 1 },
        other => other,
    }).collect();
    Compound {
        keyword: keyword.to_string(),
        atoms: normalised_atoms,
        remark: None,
        children: children_block,
    }
}

/// Convenience constructor for a Scalar-valued compound (a Field whose
/// type is Scalar, filled by a single inline atom). The value is
/// carried as an Inline atom; if it cannot be inlined (contains `LF`,
/// a hard space, etc.), the caller should construct the appropriate
/// Source or Literal atom directly per §22.2 atom-form escalation.
pub fn construct_scalar(keyword: &str, value: &str) -> Compound {
    construct(keyword,
        vec![Atom::Inline { text: value.to_string(), preceding_spaces: 1 }],
        Vec::new())
}

/// Convenience constructor for a Flag-valued compound (a bare keyword).
pub fn construct_flag(keyword: &str) -> Compound {
    construct(keyword, Vec::new(), Vec::new())
}

// ── insert-into-block ───────────────────────────────────────────────────────

/// `insert-into-block` (§22.2). Append `compound` to a specific `Block`
/// identified by its parent compound (or document root, if `parent` is
/// empty) and its block index within that parent's children. This is the
/// natural way to add a row to a tabulated block; ordinary inserts use
/// `insert` / `insert-before` / `insert-after`.
///
/// For a tabulated block, the caller is responsible for ensuring the
/// tabulation has sufficient column capacity for the new compound;
/// `resize_tabulation` MUST be applied first if the new compound's
/// content would overflow any existing column.
pub fn insert_into_block(
    doc: &mut Document,
    parent: &Path,
    block_index: usize,
    compound: Compound,
) -> Result<(), MutationError> {
    let blocks: &mut Vec<Block> = if parent.steps.is_empty() {
        &mut doc.children
    } else {
        let p = walk_mut(doc, parent)?;
        &mut p.children
    };
    let block = blocks.get_mut(block_index).ok_or(MutationError::PathNotFound)?;
    block.compounds.push(compound);
    Ok(())
}

// ── reorder-within-group ────────────────────────────────────────────────────

/// `reorder-within-group` (§22.2). Move the compound at `target` to a new
/// position `new_index` among its same-keyword siblings within the same
/// block. The remark, attached comments, and trailing-blank-lines counts
/// are preserved.
///
/// A "group" here is the set of consecutive same-keyword compounds within
/// the same block: this is what §20.2's E309 contiguity rule defines as
/// a single member-fill region. `new_index` is the desired position in
/// the group (0-based), not in the block.
pub fn reorder_within_group(
    doc: &mut Document,
    target: &Path,
    new_index: usize,
) -> Result<(), MutationError> {
    if target.steps.is_empty() { return Err(MutationError::EmptyPath); }
    let (block_idx, compound_idx) = *target.steps.last().unwrap();
    let parent_steps = &target.steps[..target.steps.len() - 1];

    let blocks: &mut Vec<Block> = if parent_steps.is_empty() {
        &mut doc.children
    } else {
        let parent_path = Path { steps: parent_steps.to_vec() };
        let p = walk_mut(doc, &parent_path)?;
        &mut p.children
    };
    let block = blocks.get_mut(block_idx).ok_or(MutationError::PathNotFound)?;
    let target_keyword = block.compounds.get(compound_idx)
        .ok_or(MutationError::PathNotFound)?
        .keyword.clone();

    // Collect indices of the same-keyword group in source order.
    let group_indices: Vec<usize> = block.compounds.iter().enumerate()
        .filter_map(|(i, c)| if c.keyword == target_keyword { Some(i) } else { None })
        .collect();
    if new_index >= group_indices.len() { return Err(MutationError::PathNotFound); }

    let current_group_pos = group_indices.iter().position(|&i| i == compound_idx)
        .ok_or(MutationError::PathNotFound)?;
    if current_group_pos == new_index { return Ok(()); }

    let target_compound_idx = group_indices[new_index];
    // Remove and re-insert; group indices are preserved by removing first
    // then inserting at the (possibly adjusted) target position.
    let c = block.compounds.remove(compound_idx);
    let adjusted = if compound_idx < target_compound_idx {
        target_compound_idx
    } else {
        target_compound_idx
    };
    block.compounds.insert(adjusted, c);
    Ok(())
}

// ── reorder-groups ──────────────────────────────────────────────────────────

/// Placement instruction for `reorder_groups`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupPlacement {
    BeforeOther,
    AfterOther,
}

/// `reorder-groups` (§22.2). Move every block belonging to `group_keyword`
/// to be either before or after every block belonging to `other_keyword`,
/// among the children of `parent` (or the document root if `parent` is
/// empty). A "block belongs to" a keyword if any of its compounds has
/// that keyword.
///
/// The operation rejects with `MutationError::PathNotFound` if no blocks
/// belong to either group. It does NOT enforce schema-level E309
/// contiguity by itself, since `mutate.rs` carries no schema; the caller
/// is responsible for ensuring the move is semantically valid.
pub fn reorder_groups(
    doc: &mut Document,
    parent: &Path,
    group_keyword: &str,
    other_keyword: &str,
    placement: GroupPlacement,
) -> Result<(), MutationError> {
    let blocks: &mut Vec<Block> = if parent.steps.is_empty() {
        &mut doc.children
    } else {
        let p = walk_mut(doc, parent)?;
        &mut p.children
    };

    let group_block_indices: Vec<usize> = blocks.iter().enumerate()
        .filter_map(|(i, b)| if block_contains_keyword(b, group_keyword) { Some(i) } else { None })
        .collect();
    let other_block_indices: Vec<usize> = blocks.iter().enumerate()
        .filter_map(|(i, b)| if block_contains_keyword(b, other_keyword) { Some(i) } else { None })
        .collect();

    if group_block_indices.is_empty() || other_block_indices.is_empty() {
        return Err(MutationError::PathNotFound);
    }

    // Extract group blocks (in source order); compute the insertion point.
    let mut group_blocks: Vec<Block> = Vec::with_capacity(group_block_indices.len());
    for &i in group_block_indices.iter().rev() {
        group_blocks.push(blocks.remove(i));
    }
    group_blocks.reverse();

    // Recompute other-block positions (some may have shifted left after
    // removal of preceding group blocks).
    let other_after_removal: Vec<usize> = blocks.iter().enumerate()
        .filter_map(|(i, b)| if block_contains_keyword(b, other_keyword) { Some(i) } else { None })
        .collect();
    let insert_at = match placement {
        GroupPlacement::BeforeOther => *other_after_removal.first().unwrap(),
        GroupPlacement::AfterOther => *other_after_removal.last().unwrap() + 1,
    };
    for (offset, b) in group_blocks.into_iter().enumerate() {
        blocks.insert(insert_at + offset, b);
    }
    Ok(())
}

fn block_contains_keyword(b: &Block, keyword: &str) -> bool {
    b.compounds.iter().any(|c| c.keyword == keyword)
}

// ── resize-tabulation ───────────────────────────────────────────────────────

/// `resize-tabulation` (§22.2). Recompute the `marker_offsets` of a
/// tabulated block so that every existing row fits without violating the
/// hard-space minimum-gap rule of §16.1. Implements the **minimal-offsets
/// algorithm** specified normatively in §22.2:
///
/// 1. For each column `i`, compute `w_i` = the maximum code-point width
///    of any value (or heading) that appears in column `i`.
/// 2. `marker_offsets[0] = w_0 + 2`, and for `i ≥ 1`,
///    `marker_offsets[i] = marker_offsets[i-1] + 1 + w_i + 2`.
///
/// The headings list is preserved in length; column counts are not
/// changed by this operation. The block MUST have a tabulation, and at
/// least one row.
///
/// Each row's atoms are assumed to be column values in left-to-right
/// order; rows whose atom count is less than the column count are
/// treated as having empty values for the missing columns.
pub fn resize_tabulation(
    doc: &mut Document,
    parent: &Path,
    block_index: usize,
) -> Result<(), MutationError> {
    let blocks: &mut Vec<Block> = if parent.steps.is_empty() {
        &mut doc.children
    } else {
        let p = walk_mut(doc, parent)?;
        &mut p.children
    };
    let block = blocks.get_mut(block_index).ok_or(MutationError::PathNotFound)?;
    let tab = block.tabulation.as_mut().ok_or(MutationError::PathNotFound)?;

    // Column count is `marker_offsets.len()`: column 0 is the keyword
    // column, columns 1..n are the data columns.
    let n_cols = tab.marker_offsets.len();
    if n_cols == 0 { return Ok(()); }

    // Compute w_i for each column.
    let mut widths: Vec<usize> = vec![0; n_cols];
    // Heading widths.
    for (i, h) in tab.headings.iter().enumerate() {
        if i < n_cols {
            widths[i] = widths[i].max(h.chars().count());
        }
    }
    // Row widths.
    for row in &block.compounds {
        // Column 0 width: the row's keyword.
        widths[0] = widths[0].max(row.keyword.chars().count());
        // Columns 1..n-1: take from the row's atoms in order.
        for (col, atom) in row.atoms.iter().enumerate() {
            let col_idx = col + 1;
            if col_idx < n_cols {
                widths[col_idx] = widths[col_idx].max(atom_text(atom).chars().count());
            }
        }
    }

    // Compute the minimal offsets.
    let new_offsets = minimal_offsets(&widths);
    tab.marker_offsets = new_offsets;
    Ok(())
}

/// Pure helper: given per-column widths `w_i`, return the offsets `M_i`
/// satisfying §22.2's minimal-offsets formula. `w[0]` is the keyword
/// column width; `w[i]` for `i ≥ 1` is column `i`'s value width.
pub(crate) fn minimal_offsets(widths: &[usize]) -> Vec<usize> {
    if widths.is_empty() { return Vec::new(); }
    let mut out = Vec::with_capacity(widths.len());
    out.push(0);                      // M_0 = 0 (the keyword column starts at margin)
    if widths.len() == 1 { return out; }
    // M_1 = w_0 + 2 (keyword width + two-space gap before the sigil marker).
    let mut cursor = widths[0] + 2;
    out.push(cursor);
    for i in 2..widths.len() {
        // M_i = M_{i-1} + 1 + w_{i-1} + 2
        // The `+1` accounts for the sigil character at M_{i-1}; `w_{i-1}`
        // is the i-1-th column's value width; `+2` is the minimum gap to
        // the next marker.
        cursor = cursor + 1 + widths[i - 1] + 2;
        out.push(cursor);
    }
    out
}

// ── Path search helpers ──────────────────────────────────────────────────────

/// Find the first compound at the document root whose keyword equals
/// `keyword`. Returns its path, or `None` if not found.
pub fn find_root_by_keyword(doc: &Document, keyword: &str) -> Option<Path> {
    for (bi, block) in doc.children.iter().enumerate() {
        for (ci, c) in block.compounds.iter().enumerate() {
            if c.keyword == keyword {
                return Some(Path { steps: vec![(bi, ci)] });
            }
        }
    }
    None
}

/// Walk a compound's blocks and return the path (relative to root) of the
/// first descendant whose keyword equals `keyword`, given a path to the
/// parent compound.
pub fn find_child_by_keyword(doc: &Document, parent: &Path, keyword: &str) -> Option<Path> {
    if parent.steps.is_empty() {
        return find_root_by_keyword(doc, keyword);
    }
    // Walk down to the parent to read its children.
    let mut blocks: &Vec<Block> = &doc.children;
    for (b, c) in &parent.steps {
        let block = blocks.get(*b)?;
        let cc = block.compounds.get(*c)?;
        blocks = &cc.children;
    }
    for (bi, block) in blocks.iter().enumerate() {
        for (ci, c) in block.compounds.iter().enumerate() {
            if c.keyword == keyword {
                let mut p = parent.clone();
                p.steps.push((bi, ci));
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn small_doc() -> Document {
        // tel 1.0\n\nname Alice\nemail alice@example.org   # personal\n
        parse("tel 1.0\n\nname Alice\nemail alice@example.org   # personal\n").document
    }

    #[test]
    fn update_value_inline_replaces_text() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").expect("email present");
        update_value(&mut doc, &p, "bob@example.org").unwrap();
        let block = &doc.children[0];
        let compound = block.compounds.iter().find(|c| c.keyword == "email").unwrap();
        match compound.atoms.first().unwrap() {
            Atom::Inline { text, .. } => assert_eq!(text, "bob@example.org"),
            _ => panic!("expected inline atom"),
        }
        // Remark preserved.
        assert_eq!(compound.remark.as_deref(), Some("personal"));
    }

    #[test]
    fn attach_remark_sets_a_remark() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "name").expect("name present");
        attach_remark(&mut doc, &p, "primary").unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "name").unwrap();
        assert_eq!(c.remark.as_deref(), Some("primary"));
    }

    #[test]
    fn remove_remark_clears_it() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        remove_remark(&mut doc, &p).unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "email").unwrap();
        assert!(c.remark.is_none());
    }

    #[test]
    fn delete_removes_a_compound() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        delete(&mut doc, &p).unwrap();
        let names: Vec<&str> = doc.children.iter().flat_map(|b|
            b.compounds.iter().map(|c| c.keyword.as_str())).collect();
        assert!(!names.contains(&"email"));
    }

    #[test]
    fn set_flag_adds_a_flag_child() {
        let mut doc = small_doc();
        set_flag(&mut doc, &Path::empty(), "active").unwrap();
        let active = doc.children.iter().flat_map(|b| b.compounds.iter())
            .find(|c| c.keyword == "active").expect("active flag present");
        assert!(active.atoms.is_empty());
        assert!(active.children.is_empty());
    }

    #[test]
    fn unset_flag_removes_a_flag_child() {
        let mut doc = small_doc();
        set_flag(&mut doc, &Path::empty(), "active").unwrap();
        unset_flag(&mut doc, &Path::empty(), "active").unwrap();
        let present = doc.children.iter().flat_map(|b| b.compounds.iter())
            .any(|c| c.keyword == "active");
        assert!(!present);
    }

    #[test]
    fn insert_after_places_in_same_block_when_no_tabulation() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "name").unwrap();
        let phone = Compound {
            keyword: "phone".to_string(),
            atoms: vec![Atom::Inline {
                text: "44".to_string(), preceding_spaces: 1,
            }],
            remark: None, children: Vec::new(),
        };
        insert_after(&mut doc, &p, phone).unwrap();
        let block = &doc.children[0];
        let keywords: Vec<&str> = block.compounds.iter().map(|c| c.keyword.as_str()).collect();
        // Original order: name, email. After insert_after(name, phone): name, phone, email.
        assert_eq!(keywords, vec!["name", "phone", "email"]);
    }

    #[test]
    fn insert_before_places_in_same_block_when_no_tabulation() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        let phone = construct_scalar("phone", "44");
        insert_before(&mut doc, &p, phone).unwrap();
        let block = &doc.children[0];
        let keywords: Vec<&str> = block.compounds.iter().map(|c| c.keyword.as_str()).collect();
        // Original order: name, email. After insert_before(email, phone): name, phone, email.
        assert_eq!(keywords, vec!["name", "phone", "email"]);
    }

    #[test]
    fn update_value_upgrades_source_to_literal_for_trailing_spaces() {
        // Start with a source-form atom (multi-line). Update its value to
        // one whose lines have trailing spaces — source atoms strip trailing
        // spaces, so the value must escalate to a literal atom to preserve
        // them exactly.
        let mut doc = parse("tel 1.0\n\nnote\n    line a\n    line b\n").document;
        let p = find_root_by_keyword(&doc, "note").unwrap();
        // Confirm initial form is source.
        let c0 = doc.children[0].compounds.iter().find(|c| c.keyword == "note").unwrap();
        assert!(matches!(c0.atoms.first(), Some(Atom::Source { .. })));
        // Update with a value whose first line has a trailing space.
        update_value(&mut doc, &p, "first \nsecond").unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "note").unwrap();
        match c.atoms.first().unwrap() {
            Atom::Literal { text, delimiter } => {
                assert_eq!(text, "first \nsecond");
                assert!(!text.lines().any(|l| l == delimiter),
                        "chosen delimiter must not collide with any payload line");
            }
            other => panic!("expected literal atom after upgrade, got: {:?}", other),
        }
    }

    #[test]
    fn update_value_upgrades_to_source_for_newline() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        update_value(&mut doc, &p, "line1\nline2").unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "email").unwrap();
        match c.atoms.first().unwrap() {
            Atom::Source { text } => assert_eq!(text, "line1\nline2"),
            _ => panic!("expected source atom for multi-line value"),
        }
    }

    #[test]
    fn update_value_inline_escalates_to_source_for_hard_space() {
        // Two consecutive spaces are a hard space: not inline-safe, but a
        // single line with no leading/trailing space is source-safe.
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        update_value(&mut doc, &p, "a  b").unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "email").unwrap();
        match c.atoms.first().unwrap() {
            Atom::Source { text } => assert_eq!(text, "a  b"),
            other => panic!("expected source atom for hard-space value, got: {:?}", other),
        }
    }

    #[test]
    fn update_value_inline_escalates_to_source_for_leading_sigil_remark() {
        // A leading sigil followed by a soft space would parse as a remark
        // (§11.2), so it is not inline-safe. (An internal space-then-sigil such
        // as "a #b" *is* inline-safe — it stays a single atom in hard-space
        // mode — so it would remain inline.)
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        update_value(&mut doc, &p, "# x").unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "email").unwrap();
        match c.atoms.first().unwrap() {
            Atom::Source { text } => assert_eq!(text, "# x"),
            other => panic!("expected source atom for leading-sigil remark value, got: {:?}", other),
        }
    }

    #[test]
    fn update_value_inline_escalates_to_literal_for_leading_space() {
        // A leading space is neither inline-safe nor source-safe (a source atom
        // strips the first line's indentation), so it must become literal.
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        update_value(&mut doc, &p, " leading").unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "email").unwrap();
        match c.atoms.first().unwrap() {
            Atom::Literal { text, delimiter } => {
                assert_eq!(text, " leading");
                assert!(!text.split('\n').any(|l| l == delimiter),
                        "chosen delimiter must not collide with any payload line");
            }
            other => panic!("expected literal atom for leading-space value, got: {:?}", other),
        }
    }

    #[test]
    fn update_value_inline_escalates_to_literal_for_trailing_space() {
        // Trailing space: not inline-safe and not source-safe (source strips it).
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        update_value(&mut doc, &p, "trailing ").unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "email").unwrap();
        match c.atoms.first().unwrap() {
            Atom::Literal { text, delimiter } => {
                assert_eq!(text, "trailing ");
                assert!(!text.split('\n').any(|l| l == delimiter),
                        "chosen delimiter must not collide with any payload line");
            }
            other => panic!("expected literal atom for trailing-space value, got: {:?}", other),
        }
    }

    #[test]
    fn update_value_source_escalates_to_literal_for_blank_line() {
        // A blank line would terminate a source atom prematurely, so a source
        // atom whose new value contains a blank line must escalate to literal.
        let mut doc = parse("tel 1.0\n\nnote\n    line a\n    line b\n").document;
        let p = find_root_by_keyword(&doc, "note").unwrap();
        assert!(matches!(
            doc.children[0].compounds.iter().find(|c| c.keyword == "note").unwrap().atoms.first(),
            Some(Atom::Source { .. })));
        update_value(&mut doc, &p, "first\n\nthird").unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "note").unwrap();
        match c.atoms.first().unwrap() {
            Atom::Literal { text, delimiter } => {
                assert_eq!(text, "first\n\nthird");
                assert!(!text.split('\n').any(|l| l == delimiter),
                        "chosen delimiter must not collide with any payload line");
            }
            other => panic!("expected literal atom for blank-line value, got: {:?}", other),
        }
    }

    #[test]
    fn replace_substitutes_compound_preserving_remark() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").expect("email present");
        // Construct a replacement with no remark of its own; the
        // original's remark should be preserved.
        let replacement = construct_scalar("email", "bob@example.org");
        replace(&mut doc, &p, replacement).unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "email").unwrap();
        match c.atoms.first().unwrap() {
            Atom::Inline { text, .. } => assert_eq!(text, "bob@example.org"),
            _ => panic!("expected inline atom"),
        }
        assert_eq!(c.remark.as_deref(), Some("personal"),
                   "remark should be preserved across replace");
    }

    #[test]
    fn replace_with_explicit_remark_overrides_original() {
        let mut doc = small_doc();
        let p = find_root_by_keyword(&doc, "email").unwrap();
        let mut replacement = construct_scalar("email", "bob@example.org");
        replacement.remark = Some("work".to_string());
        replace(&mut doc, &p, replacement).unwrap();
        let c = doc.children[0].compounds.iter().find(|c| c.keyword == "email").unwrap();
        assert_eq!(c.remark.as_deref(), Some("work"),
                   "explicit remark on replacement should override original");
    }

    #[test]
    fn construct_normalises_inline_atom_preceding_space() {
        // A constructed inline atom MUST have preceding_spaces = 1
        // per §22.2, regardless of what the input claimed.
        let c = construct("foo",
            vec![Atom::Inline { text: "bar".to_string(), preceding_spaces: 5 }],
            Vec::new());
        match c.atoms.first().unwrap() {
            Atom::Inline { preceding_spaces, .. } => assert_eq!(*preceding_spaces, 1),
            _ => panic!("expected inline atom"),
        }
    }

    #[test]
    fn construct_produces_no_remark_no_tabulation_no_blank_lines() {
        let c = construct("parent",
            Vec::new(),
            vec![
                construct_scalar("child1", "v1"),
                construct_flag("child2"),
            ]);
        assert!(c.remark.is_none(), "construct must produce no remark");
        assert_eq!(c.children.len(), 1, "construct uses a single block");
        let block = &c.children[0];
        assert!(block.tabulation.is_none(), "construct adds no tabulation");
        assert!(block.comments.is_empty(), "construct adds no comments");
        assert_eq!(block.trailing_blank_lines, 0,
                   "construct produces no trailing blank lines");
        assert_eq!(block.compounds.len(), 2);
    }

    // ── Phase A: new machine operations ──

    fn tabulated_doc() -> Document {
        // A document with one tabulated block at the root.
        parse("tel 1.0\n\n# Name  # Age\nAlice   30\nBob     25\n").document
    }

    #[test]
    fn insert_into_block_appends_to_specific_block() {
        let mut doc = tabulated_doc();
        // The tabulated block is the first (and only) block at the root.
        let row = construct("Carol", vec![
            Atom::Inline { text: "40".to_string(), preceding_spaces: 1 },
        ], Vec::new());
        insert_into_block(&mut doc, &Path::empty(), 0, row).unwrap();
        let block = &doc.children[0];
        assert!(block.tabulation.is_some(), "tabulation preserved");
        let keywords: Vec<&str> = block.compounds.iter().map(|c| c.keyword.as_str()).collect();
        assert_eq!(keywords, vec!["Alice", "Bob", "Carol"]);
    }

    #[test]
    fn reorder_within_group_swaps_two_compounds_preserving_remarks() {
        // Build a doc with two `phone` siblings sharing a block, swap them.
        let mut doc = parse("tel 1.0\n\nphone 1   # home\nphone 2   # work\n").document;
        let p_first = Path { steps: vec![(0, 0)] };
        reorder_within_group(&mut doc, &p_first, 1).unwrap();
        let block = &doc.children[0];
        // After: original 2nd compound is at index 0, original 1st is at index 1.
        assert_eq!(block.compounds[0].remark.as_deref(), Some("work"));
        assert_eq!(block.compounds[1].remark.as_deref(), Some("home"));
    }

    #[test]
    fn reorder_groups_moves_b_group_before_a_group() {
        // Two blocks: one containing `email`, one containing `phone`.
        let mut doc = parse("tel 1.0\n\nemail a@x\n\nphone 1\n").document;
        reorder_groups(&mut doc, &Path::empty(), "phone", "email",
            GroupPlacement::BeforeOther).unwrap();
        let keywords: Vec<&str> = doc.children.iter()
            .flat_map(|b| b.compounds.iter().map(|c| c.keyword.as_str()))
            .collect();
        assert_eq!(keywords, vec!["phone", "email"], "phone block now precedes email block");
    }

    #[test]
    fn reorder_groups_returns_path_not_found_when_group_absent() {
        let mut doc = parse("tel 1.0\n\nemail a@x\n").document;
        let err = reorder_groups(&mut doc, &Path::empty(), "missing", "email",
            GroupPlacement::BeforeOther).unwrap_err();
        assert!(matches!(err, MutationError::PathNotFound));
    }

    #[test]
    fn resize_tabulation_widens_for_longer_value() {
        // `# Name  # Age` parses with two markers (M_0, M_1): column 0 is the
        // keyword area, column 1 is the "Name" column. The "Age" content
        // would be in a tabulation with three markers; this fixture has two.
        // Insert a row with a wider keyword "Christopher" (11 chars) and resize.
        let mut doc = tabulated_doc();
        let new_row = construct("Christopher", vec![
            Atom::Inline { text: "45".to_string(), preceding_spaces: 1 },
        ], Vec::new());
        insert_into_block(&mut doc, &Path::empty(), 0, new_row).unwrap();
        resize_tabulation(&mut doc, &Path::empty(), 0).unwrap();
        let block = &doc.children[0];
        let tab = block.tabulation.as_ref().unwrap();
        assert_eq!(tab.marker_offsets.len(), 2);
        // M_0 = 0, M_1 = w_0 + 2 = 11 + 2 = 13 (widest keyword = "Christopher").
        assert_eq!(tab.marker_offsets[0], 0);
        assert_eq!(tab.marker_offsets[1], 13);
    }

    #[test]
    fn reorder_groups_after_placement() {
        let mut doc = parse("tel 1.0\n\nemail a@x\n\nphone 1\n").document;
        reorder_groups(&mut doc, &Path::empty(), "email", "phone",
            GroupPlacement::AfterOther).unwrap();
        let keywords: Vec<&str> = doc.children.iter()
            .flat_map(|b| b.compounds.iter().map(|c| c.keyword.as_str()))
            .collect();
        assert_eq!(keywords, vec!["phone", "email"], "email moves to after phone");
    }

    #[test]
    fn resize_tabulation_handles_three_columns() {
        // Parse a three-column tabulated block. Resize and verify the M_2
        // offset matches the §22.2 formula M_2 = M_1 + 1 + w_1 + 2.
        let mut doc = parse(
            "tel 1.0\n\n# ID  # Name  # Age\nAlice   30    A\nBob     25    B\n",
        ).document;
        resize_tabulation(&mut doc, &Path::empty(), 0).unwrap();
        let block = &doc.children[0];
        let tab = block.tabulation.as_ref().unwrap();
        assert_eq!(tab.marker_offsets.len(), 3);
        // Column 0 (keyword): widest = "Alice" / "Bob" → max len 5; M_1 = 5 + 2 = 7.
        // Column 1 ("Name" heading vs "30","25" data — but wait, the row's
        // atoms in column 1 are "30" and "25"). Widest = max(4, 2, 2) = 4.
        // M_2 = M_1 + 1 + w_1 + 2 = 7 + 1 + 4 + 2 = 14.
        assert_eq!(tab.marker_offsets[0], 0);
        assert_eq!(tab.marker_offsets[1], 7);
        assert_eq!(tab.marker_offsets[2], 14);
    }

    #[test]
    fn insert_into_block_preserves_tabulation() {
        let mut doc = tabulated_doc();
        // Confirm tabulation is present beforehand.
        assert!(doc.children[0].tabulation.is_some());
        let new_row = construct("Carol", vec![
            Atom::Inline { text: "40".to_string(), preceding_spaces: 1 },
        ], Vec::new());
        insert_into_block(&mut doc, &Path::empty(), 0, new_row).unwrap();
        let block = &doc.children[0];
        // Tabulation must survive the insert.
        assert!(block.tabulation.is_some(), "tabulation preserved");
        assert_eq!(block.compounds.len(), 3, "row appended");
    }

    #[test]
    fn minimal_offsets_matches_spec_formula() {
        // Specification §22.2: M_0 = 0, M_1 = w_0 + 2,
        // M_i = M_{i-1} + 1 + w_{i-1} + 2 for i ≥ 2.
        // Two columns, widths [5, 4]: M_0=0, M_1=5+2=7. Only 2 offsets.
        let m = minimal_offsets(&[5, 4]);
        assert_eq!(m, vec![0, 7]);
        // Three columns, widths [3, 6, 2]: M_0=0, M_1=3+2=5, M_2=5+1+6+2=14.
        let m = minimal_offsets(&[3, 6, 2]);
        assert_eq!(m, vec![0, 5, 14]);
    }
}
