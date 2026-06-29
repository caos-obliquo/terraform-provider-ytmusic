#!/usr/bin/env python3
"""Extract artist/album/year triples from a.txt (RYM list dump)."""

import re, json, sys

lines = open("a.txt").read().splitlines()

entries = []
seen = set()  # dedup by (artist, album)

# Junk patterns to skip when looking for artist/album lines
JUNK_RE = re.compile(
    r"^(Style:|Note:|☆|♫|🌓|⭐|■|★|Also Under:|Recommended by|"
    r"Profile photo|suggested by:|silly$|:3$|"
    r"♫|♪|•|→|⤷)"
)

YEAR_RE = re.compile(r"^(.+?)\s*\((\d{4})\)(?:\s*\[([^\]]*)\])?\s*$")

i = 0
while i < len(lines):
    line = lines[i].strip()
    
    # Try to match "Album (Year)" or "Album (Year) [Type]"
    m = YEAR_RE.match(line)
    if m:
        album_line = m.group(1).strip()
        year = m.group(2)
        
        # Look backwards for artist (skip empty/junk lines)
        artist = None
        j = i - 1
        while j >= 0:
            prev = lines[j].strip()
            if prev == "":
                j -= 1
                continue
            if JUNK_RE.match(prev) or YEAR_RE.match(prev):
                break
            # Found a potential artist line
            # It should not be a standalone number
            if re.match(r"^\d+$", prev):
                j -= 1
                continue
            artist = prev
            break
        
        if artist:
            key = (artist.lower(), album_line.lower())
            if key not in seen:
                seen.add(key)
                entries.append({
                    "artist": artist,
                    "album": album_line,
                    "year": int(year)
                })
    
    i += 1

print(f"Extracted {len(entries)} unique entries", file=sys.stderr)

# Save as genre JSON
# Extract unique artist names
artists = sorted(set(e["artist"] for e in entries), key=str.casefold)

out = {
    "genre": "Sasscore",
    "description": "Sasscore is a chaotic, dance-infused fusion of screamo, noise rock, and post-hardcore characterized by spastic rhythms, synths, snotty vocals, and ironic/sexual lyrical themes. Originating in the early 2000s underground.",
    "entries": entries
}
with open("genre-to-playlist/genres/sasscore.json", "w") as f:
    json.dump(out, f, indent=2, ensure_ascii=False)

print(f"Saved to genre-to-playlist/genres/sasscore.json ({len(artists)} artists)", file=sys.stderr)
