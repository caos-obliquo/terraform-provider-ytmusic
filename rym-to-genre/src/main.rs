use clap::Parser;
use regex::Regex;
use serde::Serialize;
use std::path::PathBuf;

// ─── CLI ─────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "rym-to-genre", about = "Convert RYM text dumps to genre-to-playlist JSON")]
struct Cli {
    /// Input file (RYM text dump)
    input: PathBuf,

    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Genre name (required)
    #[arg(short, long)]
    genre: String,

    /// Genre description
    #[arg(short, long)]
    description: Option<String>,

    /// Force: overwrite existing output file
    #[arg(short, long)]
    force: bool,

    /// Verbose: log skipped lines and stats
    #[arg(short, long)]
    verbose: bool,
}

// ─── Output format ───────────────────────────────────────────────────

#[derive(Serialize)]
struct GenreData {
    genre: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    entries: Vec<Entry>,
    /// Legacy field — new files use entries only
    #[serde(skip_serializing_if = "Option::is_none")]
    bands: Option<u32>,
}

#[derive(Serialize)]
struct Entry {
    artist: String,
    album: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    year: Option<u32>,
}

// ─── RYM Parser ──────────────────────────────────────────────────────

/// RYM text dump: entries separated by blank lines.
///
/// Each entry block:
///   Line 1: <score>\t<album title>   (or just <score> with no tab)
///   Line 2: <artist name>
///   Line 3: <album title> (<year>) [<type>]
///
/// Extra lines (4+) in the same block are notes/styles — ignored.

struct RawEntry {
    artist: String,
    album: String,
    year: Option<u32>,
}

fn parse_album_line(line: &str) -> (String, Option<u32>) {
    let year_re = Regex::new(r"\((\d{4})\)").unwrap();
    let year = year_re.captures(line).and_then(|c| c[1].parse::<u32>().ok());
    // Strip bracketed content: [EP], [Compilation], etc.
    let cleaned = Regex::new(r"\s*\[.*?\]").unwrap().replace_all(line, "");
    // Strip year parens
    let cleaned = Regex::new(r"\s*\(\d{4}\)").unwrap().replace_all(&cleaned, "");
    (cleaned.trim().to_string(), year)
}

/// Check if a line starts a valid RYM entry.
/// RYM uses two formats:
///   1. <score>\t<title> — tab-separated score prefix
///   2. <title> alone — no score, just the album title
/// Both are valid. The year-in-parens check on line 3 filters notes.
fn is_entry_first_line(_line: &str) -> bool {
    true
}

/// Check if a line contains (YYYY)
fn has_year_in_parens(line: &str) -> bool {
    let bytes = line.as_bytes();
    // Scan for "(4digits)"
    for i in 0..bytes.len().saturating_sub(5) {
        if bytes[i] == b'('
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
            && bytes[i + 4].is_ascii_digit()
            && bytes[i + 5] == b')'
        {
            return true;
        }
    }
    false
}

/// Try to extract (artist, album_line) from 3 lines.
fn try_extract_entry(lines: &[&str]) -> Option<(String, String)> {
    if lines.len() < 3 {
        return None;
    }
    let first = lines[0].trim();
    let second = lines[1].trim();
    let third = lines[2].trim();

    if !is_entry_first_line(first) {
        return None;
    }
    if !has_year_in_parens(third) {
        return None;
    }
    let artist = second.to_string();
    if artist.len() < 2 {
        return None;
    }
    Some((artist, third.to_string()))
}

fn parse_rym(input: &str) -> (Vec<RawEntry>, usize, usize) {
    // Split into groups by blank lines
    let mut groups: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for line in input.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !cur.is_empty() {
                groups.push(cur.clone());
                cur.clear();
            }
        } else {
            cur.push(t);
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }

    let total = groups.len();
    let mut entries = Vec::new();
    let mut skipped = 0u32;

    for group in &groups {
        if group.len() < 3 {
            skipped += 1;
            continue;
        }

        // Scan for first valid 3-line entry anywhere in the group.
        // In well-formed data it's lines [0..3].
        // When notes prepend (no blank line separator), entry may start at offset 1.
        let mut found = false;
        for offset in 0..=group.len().saturating_sub(3) {
            if let Some((artist, album_line)) = try_extract_entry(&group[offset..]) {
                let (album, year) = parse_album_line(&album_line);
                if album.len() >= 1 {
                    entries.push(RawEntry { artist, album, year });
                    found = true;
                    break;
                }
            }
        }
        if !found {
            skipped += 1;
        }
    }

    (entries, total, skipped as usize)
}

// ─── Main ────────────────────────────────────────────────────────────

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    // Read input
    let input_raw =
        std::fs::read_to_string(&cli.input).map_err(|e| format!("read {}: {}", cli.input.display(), e))?;

    // Parse
    let (raw_entries, total_groups, skipped) = parse_rym(&input_raw);
    if raw_entries.is_empty() {
        return Err(format!(
            "no entries found ({} groups total, {} skipped)",
            total_groups, skipped
        ));
    }

    if cli.verbose {
        eprintln!(
            "Groups: {} total | Skipped: {} | Parsed: {} raw entries",
            total_groups, skipped, raw_entries.len()
        );
    }

    // Dedup (same artist + album combo)
    let mut seen = std::collections::HashSet::new();
    let entries: Vec<Entry> = raw_entries
        .into_iter()
        .filter(|e| seen.insert((e.artist.to_lowercase(), e.album.to_lowercase())))
        .map(|e| Entry { artist: e.artist, album: e.album, year: e.year })
        .collect();

    if cli.verbose {
        eprintln!("Unique entries after dedup: {}", entries.len());
    }

    // Build output
    let data = GenreData { genre: cli.genre, description: cli.description, entries, bands: None };

    let json = serde_json::to_string_pretty(&data).map_err(|e| format!("serialize: {e}"))?;

    // Write
    if let Some(out) = &cli.output {
        if out.exists() && !cli.force {
            return Err(format!("output file {} exists; use --force to overwrite", out.display()));
        }
        std::fs::write(out, &json).map_err(|e| format!("write {}: {}", out.display(), e))?;
        eprintln!("Wrote {} unique entries to {}", data.entries.len(), out.display());
    } else {
        println!("{}", json);
    }

    Ok(())
}
