#!/usr/bin/env python3
"""
base256: Select 256 Unicode characters for lossless byte encoding.

Assignment constraint: character at position i has codepoint % 256 == i.
Primary pool: Unicode letters with codepoints U+0000..U+07FF (1-2 UTF-8 bytes).
Extended pool: Latin/Greek/Cyrillic letters up to U+FFFF (3 UTF-8 bytes), used
only in a post-hoc pass to improve the worst-case visual pair.
"""

import unicodedata
import sys
import os
import numpy as np
from PIL import Image, ImageDraw, ImageFont

DEFAULT_FONT_PATHS = [
    os.path.expanduser("~/Library/Fonts/JetBrainsMono[wght].ttf"),
    os.path.expanduser("~/Library/Fonts/JetBrainsMono-Regular.ttf"),
    "/Library/Fonts/JetBrainsMono-Regular.ttf",
    "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
    "/usr/local/share/fonts/JetBrainsMono-Regular.ttf",
]

RENDER_SIZE = 32   # pixels per side for rendered glyph canvas
FONT_SIZE   = 24   # point size; chosen so most glyphs fit in RENDER_SIZE


def find_font() -> str:
    path = os.environ.get("JETBRAINS_MONO_PATH")
    if path:
        return path
    for path in DEFAULT_FONT_PATHS:
        if os.path.exists(path):
            return path
    raise FileNotFoundError(
        "JetBrains Mono not found. Install it or set JETBRAINS_MONO_PATH."
    )


def render_char(char: str, font: ImageFont.FreeTypeFont) -> np.ndarray:
    """Render char centred on a RENDER_SIZE×RENDER_SIZE greyscale canvas.
    Returns float32 array in [0, 1].
    """
    img  = Image.new("L", (RENDER_SIZE, RENDER_SIZE), 0)
    bbox = font.getbbox(char)
    if not bbox:
        return np.zeros((RENDER_SIZE, RENDER_SIZE), dtype=np.float32)
    gw, gh = bbox[2] - bbox[0], bbox[3] - bbox[1]
    x = (RENDER_SIZE - gw) // 2 - bbox[0]
    y = (RENDER_SIZE - gh) // 2 - bbox[1]
    ImageDraw.Draw(img).text((x, y), char, fill=255, font=font)
    return np.asarray(img, dtype=np.float32) / 255.0


def ink_reaches_baseline(char: str, font: ImageFont.FreeTypeFont, tolerance: int = 2) -> bool:
    """True if the lowest inked pixel row is within `tolerance` rows of the baseline.
    Pillow renders with y=0 at the ascender line, so baseline = row ascent.
    """
    ascent, descent = font.getmetrics()
    img = Image.new("L", (RENDER_SIZE, ascent + descent + 4), 0)
    ImageDraw.Draw(img).text((0, 0), char, fill=255, font=font)
    arr  = np.asarray(img, dtype=np.uint8)
    rows = np.where(arr.max(axis=1) > 20)[0]
    return len(rows) > 0 and int(rows.max()) >= ascent - tolerance


def extract_contour(arr: np.ndarray, threshold: float = 0.1) -> np.ndarray:
    """Return (N, 2) float32 array of boundary pixel coordinates.
    Boundary = lit pixels (> threshold) with at least one unlit 8-neighbour.
    Falls back to all lit pixels for very thin glyphs.
    """
    binary = arr > threshold
    pad    = np.pad(binary, 1, constant_values=False)
    interior = (
        pad[1:-1, 1:-1] & pad[0:-2, 1:-1] & pad[2:,   1:-1] &
        pad[1:-1, 0:-2] & pad[1:-1, 2:  ] &
        pad[0:-2, 0:-2] & pad[0:-2, 2:  ] &
        pad[2:,   0:-2] & pad[2:,   2:  ]
    )
    pts = np.argwhere(binary & ~interior)
    return (pts if len(pts) else np.argwhere(binary)).astype(np.float32)


def mhd(A: np.ndarray, B: np.ndarray) -> float:
    """Modified Hausdorff distance: mean of symmetric per-point nearest-neighbour distances.
    MHD(A,B) = (mean_{a∈A} min_{b∈B} ||a-b|| + mean_{b∈B} min_{a∈A} ||a-b||) / 2
    """
    if len(A) == 0 and len(B) == 0:
        return 0.0
    if len(A) == 0 or len(B) == 0:
        return float(RENDER_SIZE * 2)
    D = np.sqrt(((A[:, np.newaxis] - B[np.newaxis]) ** 2).sum(axis=2))
    return float((D.min(axis=1).mean() + D.min(axis=0).mean()) / 2)


def mhd_row(contour: np.ndarray, others: list[np.ndarray]) -> np.ndarray:
    """MHD from contour to each element of others. Returns float32 array."""
    return np.array([mhd(contour, o) for o in others], dtype=np.float32)


def case_partner(char: str) -> str | None:
    """Return the single-character case partner, or None if caseless or multi-char fold."""
    u, l = char.upper(), char.lower()
    if len(u) == 1 and u != char:
        return u
    if len(l) == 1 and l != char:
        return l
    return None


def is_admissible(char: str, font: ImageFont.FreeTypeFont) -> bool:
    """True if char passes all quality filters: reaches baseline, ≤1 diacritic, has ink."""
    if not ink_reaches_baseline(char, font):
        return False
    nfd = unicodedata.normalize("NFD", char)
    if sum(1 for c in nfd if unicodedata.category(c).startswith("M")) >= 2:
        return False
    return render_char(char, font).max() > 0.05


def filter_by_pairs(candidates: dict[int, list[str]]) -> dict[int, list[str]]:
    """Remove any paired character whose case partner has no candidate at its position.
    Applied twice to handle mutual dependencies.
    """
    result = {pos: list(chars) for pos, chars in candidates.items()}
    for _ in range(2):
        available = {pos: set(chars) for pos, chars in result.items()}
        result = {
            pos: [c for c in chars
                  if case_partner(c) is None
                  or case_partner(c) in available.get(ord(case_partner(c)) % 256, set())]
            for pos, chars in result.items()
        }
    return result


def build_candidates(codepoint_range: range, font: ImageFont.FreeTypeFont,
                     extra_filter=None) -> tuple[dict[int, list[str]], dict[str, np.ndarray]]:
    """Scan codepoint_range for admissible Unicode letters.
    Returns (candidates_by_position, contours_by_char).
    extra_filter(char) -> bool may impose additional constraints.
    """
    candidates: dict[int, list[str]] = {i: [] for i in range(256)}
    contours:   dict[str, np.ndarray] = {}
    for cp in codepoint_range:
        char = chr(cp)
        if not unicodedata.category(char).startswith("L"):
            continue
        if extra_filter and not extra_filter(char):
            continue
        if char not in contours:
            if not is_admissible(char, font):
                continue
            contours[char] = extract_contour(render_char(char, font))
        candidates[cp % 256].append(char)
    return filter_by_pairs(candidates), contours


def apply_swap(dist_mat: np.ndarray, row_mins: np.ndarray, contour_list: list,
               assignments: dict, p: int, pos: int, new_char: str,
               new_row: np.ndarray, contours: dict) -> None:
    """Update dist_mat, row_mins, contour_list, assignments in place for a single swap."""
    dist_mat[p]    = new_row
    dist_mat[:, p] = new_row
    dist_mat[p, p] = np.inf
    row_mins[:]    = dist_mat.min(axis=1)
    contour_list[p]  = contours[new_char]
    assignments[pos] = new_char


def main() -> None:
    font_path = find_font()
    print(f"Font: {font_path}", file=sys.stderr)
    font = ImageFont.truetype(font_path, FONT_SIZE)

    # ---- Build primary candidate pool: all Unicode letters U+0000..U+07FF ----
    print("Scanning primary candidates (U+0000..U+07FF)…", file=sys.stderr)
    candidates, contours = build_candidates(range(0x800), font)

    # ---- Seeds: digits 0-9, Latin A-Z and a-z --------------------------------
    # Digits are not Unicode letters but are fixed unconditionally.
    SEEDS = ([chr(c) for c in range(ord('0'), ord('9') + 1)] +
             [chr(c) for c in range(ord('A'), ord('Z') + 1)] +
             [chr(c) for c in range(ord('a'), ord('z') + 1)])
    seed_positions = {ord(ch) for ch in SEEDS}

    for ch in SEEDS:
        if ch not in contours:
            contours[ch] = extract_contour(render_char(ch, font))

    empty = [i for i in range(256) if not candidates[i] and i not in seed_positions]
    if empty:
        print(f"WARNING: no candidates for positions: {empty}", file=sys.stderr)

    # ---- Greedy pass: minimax assignment -------------------------------------
    # Assign positions in order 0..255.  For each position i, score each
    # candidate c as:
    #   score(c) = min_{j already assigned} MHD(c, A[j])
    # or, if c has a case partner p at unassigned position j:
    #   score(c,p) = min(score(c), score(p), MHD(c,p))
    # Choose the candidate (or pair) with the highest score and assign it.
    assignments:    dict[int, str]   = {}
    pair_of:        dict[int, int]   = {}
    assigned_contours: list          = []
    cand_set = {pos: set(chars) for pos, chars in candidates.items()}

    for ch in SEEDS:
        assignments[ord(ch)] = ch
        assigned_contours.append(contours[ch])
        print(f"Position {ord(ch):3d}: {ch}  [seed]", file=sys.stderr)

    for i in range(256):
        if i in assignments or not candidates[i]:
            continue

        best_c, best_p, best_score = candidates[i][0], None, float("-inf")

        for c in candidates[i]:
            p = case_partner(c)
            j = ord(p) % 256 if p else None
            dists_c = mhd_row(contours[c], assigned_contours) if assigned_contours else np.array([np.inf])

            if p and j != i and j not in assignments and p in cand_set.get(j, set()):
                dists_p = mhd_row(contours[p], assigned_contours) if assigned_contours else np.array([np.inf])
                score   = min(float(dists_c.min()), float(dists_p.min()),
                              mhd(contours[c], contours[p]))
            else:
                score, p = float(dists_c.min()), None

            if score > best_score:
                best_c, best_p, best_score = c, p, score

        assignments[i] = best_c
        assigned_contours.append(contours[best_c])
        print(f"Position {i:3d}: U+{ord(best_c):04X} {best_c}  [min-dist {best_score:.3f}]", file=sys.stderr)

        if best_p is not None:
            j = ord(best_p) % 256
            assignments[j] = best_p
            assigned_contours.append(contours[best_p])
            pair_of[i] = j
            pair_of[j] = i
            print(f"Position {j:3d}: U+{ord(best_p):04X} {best_p}  [paired with {i}]", file=sys.stderr)

    # ---- Build full pairwise distance matrix ---------------------------------
    pos_list   = sorted(assignments.keys())
    pos_to_idx = {p: idx for idx, p in enumerate(pos_list)}
    n          = len(pos_list)
    contour_list = [contours[assignments[p]] for p in pos_list]

    print("Building distance matrix…", file=sys.stderr)
    dist_mat = np.array([[mhd(contour_list[i], contour_list[j]) for j in range(n)]
                         for i in range(n)], dtype=np.float32)
    np.fill_diagonal(dist_mat, np.inf)
    row_mins = dist_mat.min(axis=1)

    # ---- Refinement sweep: local minimax improvement -------------------------
    # Each non-seed position (or pair) is reconsidered.  A swap is accepted if
    # the new character's nearest-neighbour distance exceeds the current one,
    # strictly increasing min_j MHD(A[i], A[j]).  Repeat until no swap improves.
    print("Refining…", file=sys.stderr)
    sweep = 0
    while True:
        sweep += 1
        global_min_before = float(row_mins.min())
        changes   = 0
        processed = set()

        for i_pos in pos_list:
            if i_pos in seed_positions or i_pos in processed:
                continue

            j_pos = pair_of.get(i_pos)
            pi    = pos_to_idx[i_pos]

            if j_pos is not None and j_pos not in seed_positions:
                processed |= {i_pos, j_pos}
                pj = pos_to_idx[j_pos]
                old_score = min(float(row_mins[pi]), float(row_mins[pj]))
                best_ci, best_cj = assignments[i_pos], assignments[j_pos]
                best_score = old_score
                best_ri = best_rj = None

                for c in candidates[i_pos]:
                    p = case_partner(c)
                    if p is None or ord(p) % 256 != j_pos or p not in cand_set.get(j_pos, set()):
                        continue
                    ri = mhd_row(contours[c], contour_list); ri[pi] = np.inf
                    ri[pj] = mhd(contours[c], contours[p])
                    rj = mhd_row(contours[p], contour_list); rj[pj] = np.inf
                    rj[pi] = ri[pj]
                    score = min(float(ri.min()), float(rj.min()))
                    if score > best_score:
                        best_score, best_ci, best_cj, best_ri, best_rj = score, c, p, ri, rj

                if best_ci != assignments[i_pos]:
                    old_ci, old_cj = assignments[i_pos], assignments[j_pos]
                    apply_swap(dist_mat, row_mins, contour_list, assignments, pi, i_pos, best_ci, best_ri, contours)
                    apply_swap(dist_mat, row_mins, contour_list, assignments, pj, j_pos, best_cj, best_rj, contours)
                    changes += 1
                    print(f"  {i_pos:3d}: {old_ci} → {best_ci}  +  {j_pos:3d}: {old_cj} → {best_cj}"
                          f"  (min {old_score:.3f} → {best_score:.3f})", file=sys.stderr)

            else:
                processed.add(i_pos)
                if len(candidates[i_pos]) <= 1:
                    continue
                best_char, best_row_min, best_row = assignments[i_pos], float(row_mins[pi]), None

                for char in candidates[i_pos]:
                    if char == assignments[i_pos]:
                        continue
                    row = mhd_row(contours[char], contour_list); row[pi] = np.inf
                    if float(row.min()) > best_row_min:
                        best_row_min, best_char, best_row = float(row.min()), char, row

                if best_char != assignments[i_pos]:
                    old_char = assignments[i_pos]
                    apply_swap(dist_mat, row_mins, contour_list, assignments, pi, i_pos, best_char, best_row, contours)
                    changes += 1
                    print(f"  {i_pos:3d}: {old_char} → {best_char}"
                          f"  (nn-dist {row_mins[pi]:.3f} → {best_row_min:.3f})", file=sys.stderr)

        print(f"Sweep {sweep}: {changes} change(s),"
              f" global min-dist {global_min_before:.3f} → {float(row_mins.min()):.3f}", file=sys.stderr)
        if changes == 0:
            break

    # ---- Extended refinement: target closest pair with 3-byte candidates -----
    # Build extended pool: Latin/Greek/Cyrillic letters U+0800..U+FFFF.
    print("Building extended candidates (U+0800..U+FFFF, Latin/Greek/Cyrillic)…", file=sys.stderr)
    lgc = {"LATIN", "GREEK", "CYRILLIC"}

    def lgc_filter(char: str) -> bool:
        return (unicodedata.name(char, "").split() or [""])[0] in lgc \
               and ord(char) % 256 not in seed_positions

    ext_cands, ext_contours = build_candidates(range(0x800, 0x10000), font, lgc_filter)
    contours.update(ext_contours)
    cand_set_ext = {pos: set(chars) for pos, chars in ext_cands.items()}
    print(f"Extended candidates: {sum(len(v) for v in ext_cands.values())}"
          f" across {sum(1 for v in ext_cands.values() if v)} positions.", file=sys.stderr)

    print("Extended refinement (targeting closest pair)…", file=sys.stderr)
    while True:
        current_min = float(row_mins.min())
        pi_close, pj_close = divmod(int(np.argmin(dist_mat)), n)
        i_pos, j_pos = pos_list[pi_close], pos_list[pj_close]

        print(f"Closest pair: {assignments[i_pos]!r} (pos {i_pos})"
              f" / {assignments[j_pos]!r} (pos {j_pos})"
              f"  dist={dist_mat[pi_close, pj_close]:.4f}"
              f"  global-min={current_min:.4f}", file=sys.stderr)

        best_score, best_action = current_min, None

        for target_pi, target_pos in [(pi_close, i_pos), (pj_close, j_pos)]:
            if target_pos in seed_positions:
                continue
            partner_pos = pair_of.get(target_pos)

            if partner_pos is not None and partner_pos not in seed_positions:
                pp = pos_to_idx[partner_pos]
                for c in ext_cands[target_pos]:
                    cp_p = case_partner(c)
                    if cp_p is None or ord(cp_p) % 256 != partner_pos:
                        continue
                    if cp_p not in cand_set_ext.get(partner_pos, set()):
                        continue
                    ri = mhd_row(contours[c], contour_list); ri[target_pi] = np.inf
                    ri[pp] = mhd(contours[c], contours[cp_p])
                    rj = mhd_row(contours[cp_p], contour_list); rj[pp] = np.inf
                    rj[target_pi] = ri[pp]
                    temp = dist_mat.copy()
                    temp[target_pi] = ri; temp[:, target_pi] = ri
                    temp[pp]        = rj; temp[:, pp]        = rj
                    temp[target_pi, target_pi] = temp[pp, pp] = np.inf
                    sc = float(temp.min())
                    if sc > best_score:
                        best_score  = sc
                        best_action = ("pair", target_pi, target_pos, c, pp, partner_pos, cp_p, ri, rj)
            else:
                for c in ext_cands[target_pos]:
                    if case_partner(c) is not None:
                        continue
                    row = mhd_row(contours[c], contour_list); row[target_pi] = np.inf
                    temp = dist_mat.copy()
                    temp[target_pi] = row; temp[:, target_pi] = row
                    temp[target_pi, target_pi] = np.inf
                    sc = float(temp.min())
                    if sc > best_score:
                        best_score  = sc
                        best_action = ("single", target_pi, target_pos, c, row)

        if best_action is None:
            print("No improvement found — stopping extended refinement.", file=sys.stderr)
            break

        if best_action[0] == "pair":
            _, pi, i_p, ci, pj, j_p, cj, ri, rj = best_action
            old_ci, old_cj = assignments[i_p], assignments[j_p]
            apply_swap(dist_mat, row_mins, contour_list, assignments, pi, i_p, ci, ri, contours)
            apply_swap(dist_mat, row_mins, contour_list, assignments, pj, j_p, cj, rj, contours)
            print(f"  {i_p:3d}: {old_ci} → {ci}  +  {j_p:3d}: {old_cj} → {cj}"
                  f"  (global-min {current_min:.4f} → {best_score:.4f})", file=sys.stderr)
        else:
            _, pi, i_p, ci, row = best_action
            old_ci = assignments[i_p]
            apply_swap(dist_mat, row_mins, contour_list, assignments, pi, i_p, ci, row, contours)
            print(f"  {i_p:3d}: {old_ci} → {ci}"
                  f"  (global-min {current_min:.4f} → {best_score:.4f})", file=sys.stderr)

    # ---- Emit results --------------------------------------------------------
    print("".join(assignments.get(i, "\ufffd") for i in range(256)))

    print("# pos\tcp\tchar\tname", file=sys.stderr)
    for i in range(256):
        if i in assignments:
            char = assignments[i]
            print(f"{i}\t{ord(char)}\t{char}\t{unicodedata.name(char, 'UNKNOWN')}", file=sys.stderr)
        else:
            print(f"{i}\t-\t-\tNO ASSIGNMENT", file=sys.stderr)


if __name__ == "__main__":
    main()
