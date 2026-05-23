#!/usr/bin/env python3
"""
base256: Find optimal 256 Unicode characters for byte encoding.

Each character at position i has codepoint % 256 == i, is a Unicode letter,
and fits in 1 or 2 UTF-8 bytes (codepoints U+0000..U+07FF).

Characters are selected iteratively using a minimax visual similarity
strategy: for each position, pick the candidate that is least visually
similar to all already-assigned characters.
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


def find_font():
    env = os.environ.get("JETBRAINS_MONO_PATH")
    if env:
        return env
    for path in DEFAULT_FONT_PATHS:
        if os.path.exists(path):
            return path
    raise FileNotFoundError(
        "JetBrains Mono not found. Install it from "
        "https://www.jetbrains.com/lp/mono/ or set JETBRAINS_MONO_PATH."
    )


def render_char(char: str, font: ImageFont.FreeTypeFont, size: int = RENDER_SIZE) -> np.ndarray:
    """Render a single character centred on a size×size greyscale canvas.

    Returns a float32 array in [0, 1].  Empty (tofu) glyphs return all-zeros.
    """
    img = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(img)
    try:
        bbox = font.getbbox(char)          # (left, top, right, bottom)
    except Exception:
        return np.zeros((size, size), dtype=np.float32)
    if bbox is None:
        return np.zeros((size, size), dtype=np.float32)
    glyph_w = bbox[2] - bbox[0]
    glyph_h = bbox[3] - bbox[1]
    # Centre the glyph
    x = (size - glyph_w) // 2 - bbox[0]
    y = (size - glyph_h) // 2 - bbox[1]
    draw.text((x, y), char, fill=255, font=font)
    return np.asarray(img, dtype=np.float32) / 255.0


def has_visible_pixels(arr: np.ndarray, threshold: float = 0.05) -> bool:
    return bool(arr.max() > threshold)


def ink_reaches_baseline(char: str, font: ImageFont.FreeTypeFont, tolerance: int = 2) -> bool:
    """Return True if the glyph's actual ink reaches the baseline.

    Pillow's getbbox() returns the full advance box (same height for every
    character in a monospaced font), so we render the character at a known
    position and inspect the pixel rows directly.

    Drawing at y=0 with Pillow's default 'la' anchor places the ascender line
    at row 0, so the baseline falls at row = ascent.  We find the lowest row
    containing ink and require it to be within `tolerance` rows of the baseline.
    """
    ascent, descent = font.getmetrics()
    canvas_h = ascent + descent + 4
    img = Image.new("L", (RENDER_SIZE, canvas_h), 0)
    ImageDraw.Draw(img).text((0, 0), char, fill=255, font=font)
    arr = np.asarray(img, dtype=np.uint8)
    rows_with_ink = np.where(arr.max(axis=1) > 20)[0]
    if len(rows_with_ink) == 0:
        return False
    return int(rows_with_ink.max()) >= ascent - tolerance


def extract_contour(arr: np.ndarray, threshold: float = 0.1) -> np.ndarray:
    """Return (N, 2) float32 array of contour pixel coordinates (row, col).

    Contour = pixels above threshold that have at least one below-threshold
    neighbour (i.e. on the boundary).  Falls back to all 'on' pixels if the
    glyph is so thin that erosion removes everything.
    """
    binary = arr > threshold
    # 3×3 erosion via manual neighbourhood checks (avoids scipy dependency)
    pad = np.pad(binary, 1, constant_values=False)
    interior = (
        pad[1:-1, 1:-1] & pad[0:-2, 1:-1] & pad[2:,   1:-1] &
        pad[1:-1, 0:-2] & pad[1:-1, 2:  ] &
        pad[0:-2, 0:-2] & pad[0:-2, 2:  ] &
        pad[2:,   0:-2] & pad[2:,   2:  ]
    )
    contour = binary & ~interior
    pts = np.argwhere(contour)
    if len(pts) == 0:
        pts = np.argwhere(binary)
    return pts.astype(np.float32)


def _modified_hausdorff(A: np.ndarray, B: np.ndarray) -> float:
    """Modified Hausdorff distance (mean of per-point nearest-neighbour distances).

    MHD(A,B) = ( mean_{a∈A} min_{b∈B} d(a,b)
               + mean_{b∈B} min_{a∈A} d(a,b) ) / 2

    Returns 0 if both sets are empty, RENDER_SIZE*2 if one is empty.
    """
    if len(A) == 0 and len(B) == 0:
        return 0.0
    if len(A) == 0 or len(B) == 0:
        return float(RENDER_SIZE * 2)
    # Pairwise squared distances via broadcasting, then sqrt
    diff = A[:, np.newaxis, :] - B[np.newaxis, :, :]   # (|A|, |B|, 2)
    D    = np.sqrt((diff ** 2).sum(axis=2))             # (|A|, |B|)
    return float((D.min(axis=1).mean() + D.min(axis=0).mean()) / 2)


def hausdorff_row(contour: np.ndarray, contours: list[np.ndarray]) -> np.ndarray:
    """Modified Hausdorff distance from contour to each entry in contours.

    Returns a (n,) float32 array of distances.  Higher = more dissimilar.
    """
    out = np.empty(len(contours), dtype=np.float32)
    for i, c in enumerate(contours):
        out[i] = _modified_hausdorff(contour, c)
    return out


def case_partner(char: str) -> str | None:
    """Return the single-character case partner (upper↔lower), or None if caseless.

    Handles multi-character folds (e.g. 'ß'.upper() == 'SS') by returning None.
    """
    u = char.upper()
    if len(u) == 1 and u != char:
        return u
    l = char.lower()
    if len(l) == 1 and l != char:
        return l
    return None


def find_candidates() -> dict[int, list[str]]:
    """Return, for each position 0-255, the list of Unicode letter candidates.

    A candidate must be a Unicode letter and have a codepoint in 0x0000..0x07FF
    (i.e. 1 or 2 UTF-8 bytes), with codepoint % 256 == position.
    """
    candidates: dict[int, list[str]] = {i: [] for i in range(256)}
    for cp in range(0x800):          # inclusive: 0x000..0x7FF
        char = chr(cp)
        if unicodedata.category(char).startswith("L"):
            candidates[cp % 256].append(char)
    return candidates


def main() -> None:
    font_path = find_font()
    print(f"Font: {font_path}", file=sys.stderr)
    font = ImageFont.truetype(font_path, FONT_SIZE)

    # ---- Build candidate lists ------------------------------------------------
    print("Scanning Unicode letters (U+0000..U+07FF)…", file=sys.stderr)
    candidates = find_candidates()

    # ---- Pre-render every candidate and extract contours ----------------------
    print("Rendering glyphs…", file=sys.stderr)
    rendered:  dict[str, np.ndarray] = {}
    contours:  dict[str, np.ndarray] = {}
    for chars in candidates.values():
        for char in chars:
            if char not in rendered:
                if not ink_reaches_baseline(char, font):
                    continue
                # Reject characters with two or more diacritics
                nfd = unicodedata.normalize("NFD", char)
                if sum(1 for c in nfd if unicodedata.category(c).startswith("M")) >= 2:
                    continue
                arr = render_char(char, font)
                if has_visible_pixels(arr):
                    rendered[char] = arr
                    contours[char] = extract_contour(arr)

    # Keep only renderable candidates that reach the baseline
    for pos in candidates:
        candidates[pos] = [c for c in candidates[pos] if c in rendered]

    # ---- Pair filtering: a paired character is only valid if its case partner
    # is also a renderable candidate for its own position.  Apply two passes to
    # handle mutual dependencies (if c needs p and p needs c, both survive; if
    # either has no valid partner in rendered, both are dropped together).
    for _ in range(2):
        cand_set = {pos: set(chars) for pos, chars in candidates.items()}
        for pos in candidates:
            filtered = []
            for c in candidates[pos]:
                p = case_partner(c)
                if p is None:
                    filtered.append(c)          # caseless — always valid
                elif p in cand_set.get(ord(p) % 256, set()):
                    filtered.append(c)          # partner is available
            candidates[pos] = filtered

    # ---- Seeds: Latin alphabet A-Z, a-z, and digits 0-9 ----------------------
    # Note: digits are not Unicode letters, so they are fixed seeds that bypass
    # the letter/candidate requirements.  All other positions must be letters.
    SEEDS = ([chr(c) for c in range(ord('0'), ord('9') + 1)] +
             [chr(c) for c in range(ord('A'), ord('Z') + 1)] +
             [chr(c) for c in range(ord('a'), ord('z') + 1)])
    seed_positions = {ord(ch) for ch in SEEDS}

    # Ensure digits are rendered and have contours even though they are not
    # in the candidates dict (which only contains Unicode letters).
    for ch in SEEDS:
        if ch not in contours:
            arr = render_char(ch, font)
            contours[ch] = extract_contour(arr)

    # ---- Warn about empty positions -------------------------------------------
    empty = [i for i in range(256) if not candidates[i] and i not in seed_positions]
    if empty:
        print(f"WARNING: no renderable candidates for positions: {empty}", file=sys.stderr)

    # ---- Greedy initial pass --------------------------------------------------
    # Objective: minimax distance.  For each candidate, compute its minimum
    # Hausdorff distance to any already-assigned character; pick the candidate
    # whose minimum is largest.  A character that is pixel-identical to any
    # assigned glyph (distance 0) will naturally score worst.
    #
    # Paired characters (upper↔lower) are assigned simultaneously.  A pair is
    # scored as the minimum across: min-dist-of-c-to-assigned,
    # min-dist-of-p-to-assigned, and the mutual distance between c and p.
    assignments:    dict[int, str] = {}
    pair_of:        dict[int, int] = {}   # maps position → its paired position
    assigned_contours: list[np.ndarray] = []

    for ch in SEEDS:
        pos = ord(ch)
        assignments[pos] = ch
        assigned_contours.append(contours[ch])
        name = unicodedata.name(ch, "UNKNOWN")
        print(f"Position {pos:3d}: U+{pos:04X} {ch}  {name}  [seed]", file=sys.stderr)

    cand_set = {pos: set(chars) for pos, chars in candidates.items()}

    for i in range(256):
        if i in assignments:
            continue
        chars = candidates[i]
        if not chars:
            continue

        best_c      = chars[0]
        best_p      = None
        best_score  = float("-inf")

        for c in chars:
            p = case_partner(c)
            j = ord(p) % 256 if p else None

            dists_c = hausdorff_row(contours[c], assigned_contours) if assigned_contours else np.array([float("inf")])

            if p and j != i and j not in assignments and p in cand_set.get(j, set()):
                dists_p = hausdorff_row(contours[p], assigned_contours) if assigned_contours else np.array([float("inf")])
                cp      = _modified_hausdorff(contours[c], contours[p])
                score   = min(float(dists_c.min()), float(dists_p.min()), cp)
            else:
                score = float(dists_c.min())
                p = None

            if score > best_score:
                best_score = score
                best_c     = c
                best_p     = p

        assignments[i] = best_c
        assigned_contours.append(contours[best_c])
        name = unicodedata.name(best_c, "UNKNOWN")
        cp = ord(best_c)
        print(f"Position {i:3d}: U+{cp:04X} {best_c}  {name}  [min-dist {best_score:.3f}]", file=sys.stderr)

        if best_p is not None:
            j = ord(best_p) % 256
            assignments[j] = best_p
            assigned_contours.append(contours[best_p])
            pair_of[i] = j
            pair_of[j] = i
            name = unicodedata.name(best_p, "UNKNOWN")
            print(f"Position {j:3d}: U+{ord(best_p):04X} {best_p}  {name}  [paired with {i}]",
                  file=sys.stderr)

    # ---- Build distance matrix for refinement --------------------------------
    pos_list   = sorted(assignments.keys())
    pos_to_idx = {p: idx for idx, p in enumerate(pos_list)}
    n = len(pos_list)
    contour_list = [contours[assignments[p]] for p in pos_list]

    print("Building distance matrix…", file=sys.stderr)
    dist_mat = np.zeros((n, n), dtype=np.float32)
    for i in range(n):
        dist_mat[i] = hausdorff_row(contour_list[i], contour_list)
    np.fill_diagonal(dist_mat, np.inf)
    row_mins = dist_mat.min(axis=1)                  # (n,) nearest-neighbour distances

    # ---- Refinement: sweep until no swap improves the minimax objective -------
    # Objective: maximise the global minimum pairwise distance (i.e. push the
    # closest pair of assigned glyphs as far apart as possible).
    #
    # For each slot, accept a swap if the new character's minimum distance to
    # all other assigned characters exceeds the current character's.  This
    # directly improves the row contribution to the global minimum.
    # Pairs are reconsidered atomically as before.
    print("Refining (minimax: maximising nearest-neighbour distance)…", file=sys.stderr)
    sweep = 0
    while True:
        sweep += 1
        global_min_before = float(row_mins.min())
        changes = 0
        processed = set()

        for i_pos in pos_list:
            if i_pos in seed_positions or i_pos in processed:
                continue

            j_pos = pair_of.get(i_pos)

            if j_pos is not None and j_pos not in seed_positions:
                # --- Paired swap ------------------------------------------------
                processed.add(i_pos)
                processed.add(j_pos)
                pi = pos_to_idx[i_pos]
                pj = pos_to_idx[j_pos]

                old_score = min(float(row_mins[pi]), float(row_mins[pj]))
                best_ci, best_pj = assignments[i_pos], assignments[j_pos]
                best_score       = old_score
                best_row_i = best_row_j = None

                for c in candidates[i_pos]:
                    p = case_partner(c)
                    if p is None or ord(p) % 256 != j_pos:
                        continue
                    if p not in cand_set.get(j_pos, set()):
                        continue

                    new_row_i     = hausdorff_row(contours[c], contour_list)
                    new_row_i[pi] = np.inf
                    new_row_i[pj] = _modified_hausdorff(contours[c], contours[p])

                    new_row_j     = hausdorff_row(contours[p], contour_list)
                    new_row_j[pj] = np.inf
                    new_row_j[pi] = new_row_i[pj]

                    score = min(float(new_row_i.min()), float(new_row_j.min()))
                    if score > best_score:
                        best_score = score
                        best_ci    = c
                        best_pj    = p
                        best_row_i = new_row_i
                        best_row_j = new_row_j

                if best_ci != assignments[i_pos]:
                    old_ci, old_pj = assignments[i_pos], assignments[j_pos]
                    dist_mat[pi] = best_row_i;  dist_mat[:, pi] = best_row_i
                    dist_mat[pj] = best_row_j;  dist_mat[:, pj] = best_row_j
                    dist_mat[pi, pi] = dist_mat[pj, pj] = np.inf
                    row_mins = dist_mat.min(axis=1)
                    contour_list[pi]   = contours[best_ci]
                    contour_list[pj]   = contours[best_pj]
                    assignments[i_pos] = best_ci
                    assignments[j_pos] = best_pj
                    changes += 1
                    print(
                        f"  {i_pos:3d}: U+{ord(old_ci):04X} {old_ci}"
                        f" → U+{ord(best_ci):04X} {best_ci}"
                        f"  +  {j_pos:3d}: U+{ord(old_pj):04X} {old_pj}"
                        f" → U+{ord(best_pj):04X} {best_pj}"
                        f"  (min {old_score:.3f} → {best_score:.3f})",
                        file=sys.stderr,
                    )

            else:
                # --- Singleton swap ---------------------------------------------
                processed.add(i_pos)
                chars = candidates[i_pos]
                if len(chars) <= 1:
                    continue
                p = pos_to_idx[i_pos]

                best_char    = assignments[i_pos]
                best_row_min = float(row_mins[p])
                best_new_row = None

                for char in chars:
                    if char == assignments[i_pos]:
                        continue
                    new_row     = hausdorff_row(contours[char], contour_list)
                    new_row[p]  = np.inf
                    new_row_min = float(new_row.min())
                    if new_row_min > best_row_min:
                        best_row_min = new_row_min
                        best_char    = char
                        best_new_row = new_row

                if best_char != assignments[i_pos]:
                    old_char    = assignments[i_pos]
                    old_row_min = float(row_mins[p])
                    dist_mat[p]     = best_new_row
                    dist_mat[:, p]  = best_new_row
                    dist_mat[p, p]  = np.inf
                    row_mins        = dist_mat.min(axis=1)
                    contour_list[p]    = contours[best_char]
                    assignments[i_pos] = best_char
                    changes += 1
                    print(
                        f"  {i_pos:3d}: U+{ord(old_char):04X} {old_char}"
                        f" → U+{ord(best_char):04X} {best_char}"
                        f"  (nn-dist {old_row_min:.3f} → {best_row_min:.3f})",
                        file=sys.stderr,
                    )

        global_min_after = float(row_mins.min())
        print(
            f"Sweep {sweep}: {changes} change(s),"
            f" global min-dist {global_min_before:.3f} → {global_min_after:.3f}",
            file=sys.stderr,
        )
        if changes == 0:
            break

    # ---- Emit results to stdout ----------------------------------------------
    result = "".join(assignments.get(i, "\ufffd") for i in range(256))
    print(result)

    print("# pos\tcp\tchar\tname", file=sys.stderr)
    for i in range(256):
        if i in assignments:
            char = assignments[i]
            cp = ord(char)
            name = unicodedata.name(char, "UNKNOWN")
            print(f"{i}\t{cp}\t{char}\t{name}", file=sys.stderr)
        else:
            print(f"{i}\t-\t-\tNO ASSIGNMENT", file=sys.stderr)


if __name__ == "__main__":
    main()
