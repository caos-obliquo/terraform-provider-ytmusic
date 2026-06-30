use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use std::fs;

use clap::Parser;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::common::{VideoID, YoutubeID};
use ytmapi_rs::parse::PlaylistItem;
use ytmapi_rs::query::playlist::PrivacyStatus;
use ytmapi_rs::query::playlist::{AddPlaylistItemsQuery, DuplicateHandlingMode};
use ytmapi_rs::query::CreatePlaylistQuery;
use ytmapi_rs::query::GetPlaylistTracksQuery;
use ytmapi_rs::YtMusic;

mod genre_validator;

const GEN_HEADER: &str = "\n——\nGenerated via github.com/caos-obliquo/terraform-provider-ytmusic";

// Hard blocklist — artists that are NEVER valid for any genre pipeline.
// Prevents pop/mainstream leaks when search returns top hits.
const POP_BLOCKLIST: &[&str] = &[
    "katy perry", "drake", "bruno mars", "rick astley", "shawn mendes",
    "one direction", "gigi perez", "dominic fike", "arjan dhillon",
    "taylor swift", "britney spears", "justin bieber", "ed sheeran",
    "billie eilish", "ariana grande", "selena gomez", "dua lipa",
    "harry styles", "the weeknd", "post malone", "eminem",
    "cardi b", "nicki minaj", "lady gaga", "rihanna", "beyonce",
    "elton john", "madonna", "michael jackson", "prince",
    "maroon 5", "coldplay", "imagine dragons", "twenty one pilots",
    "panic! at the disco", "fall out boy", "chainsmokers",
    "halsey", "lizzo", "miley cyrus", "demi lovato",
    "charlie puth", "sam smith", "adele", "shakira",
    "pink", "jennifer lopez", "usher", "akon", "pitbull",
    "flo rida", "will.i.am", "black eyed peas", "fergie",
    "sia", "tove lo", "lorde", "ellie goulding",
    "calvin harris", "david guetta", "avicii", "kygo",
    "zara larsson", "anne-marie", "rita ora",
    "doja cat", "megan thee stallion", "saweetie",
    "olivia rodrigo", "lil nas x", "jack harlow",
    "bts", "blackpink", "twice",
];

// ─── CLI ────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "genre-to-playlist", about = "Generate balanced YT Music playlists from genre band lists")]
struct Cli {
    /// Genre name (matches a file in genres/ dir, e.g. "black_metal")
    #[arg(short, long)]
    genre: Option<String>,

    /// Path to YT Music cookies file (or YTMAPI_COOKIE env)
    #[arg(short, long)]
    cookie: Option<String>,

    /// Maximum total songs in playlist (YT Music limit: 5000)
    #[arg(short = 'm', long, default_value = "5000")]
    max_songs: usize,

    /// Max songs per band (default: auto-calc from max_songs / band count)
    #[arg(short = 'p', long)]
    per_band: Option<usize>,

    /// List available genres and exit
    #[arg(short = 'l', long)]
    list_genres: bool,

    /// Search and sample but do not create playlist
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Directory containing genre JSON files
    #[arg(long, default_value = "genres")]
    genres_dir: String,

    /// Privacy status for created playlist
    #[arg(long, default_value = "private")]
    privacy: String,

    /// Playlist name (default: "Genre: <name>")
    #[arg(short = 'N', long)]
    name: Option<String>,

    /// Existing playlist ID to populate (skip create)
    #[arg(short = 'P', long)]
    playlist_id: Option<String>,

    /// Prune non-genre tracks from the playlist (removes non-matching)
    #[arg(long)]
    prune: bool,

    /// Auto-create next part when playlist nears YT Music's 5000 cap
    #[arg(long)]
    auto_split: bool,

    /// Skip search phase, load previously cached sampled songs
    #[arg(long)]
    use_cache: bool,
}

// ─── Genre data ─────────────────────────────────────────────────────────

/// A single curated entry with specific album search
#[derive(Deserialize, Clone)]
struct GenreEntry {
    artist: String,
    album: String,
    year: Option<u16>,
}

/// Supports two formats:
///   - `bands: ["Band1", ...]` — old format, search "{band} {genre}"
///   - `entries: [{artist, album, year?}, ...]` — new format, search "{artist} {album}"
#[derive(Deserialize, Clone)]
struct GenreData {
    genre: String,
    description: Option<String>,
    #[serde(default)]
    bands: Vec<String>,
    entries: Option<Vec<GenreEntry>>,
}

// ─── Song tracking ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct CachedEntry {
    video_id: String,
    title: String,
    artist: String,
    source: String,
}

impl From<CachedEntry> for SongEntry {
    fn from(c: CachedEntry) -> Self {
        Self {
            video_id: VideoID::from_raw(c.video_id),
            title: c.title,
            artist: c.artist,
            source: c.source,
        }
    }
}

struct SongEntry {
    video_id: VideoID<'static>,
    title: String,
    artist: String,
    source: String, // band name or artist name for display
}

// ─── Item enum for unified iteration ────────────────────────────────────

enum GenreItem {
    Band(String),
    Entry(GenreEntry),
}

// ─── Main ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Resolve cookie path
    let cookie = cli.cookie.or_else(|| std::env::var("YTMAPI_COOKIE").ok());
    if !cli.dry_run && !cli.list_genres {
        if cookie.is_none() {
            eprintln!("Error: --cookie <file> or YTMAPI_COOKIE env required");
            std::process::exit(1);
        }
        if !std::path::Path::new(cookie.as_ref().unwrap()).exists() {
            eprintln!("Error: cookie file not found: {}", cookie.as_ref().unwrap());
            std::process::exit(1);
        }
    }

    // Discover genres
    let genres_dir = PathBuf::from(&cli.genres_dir);
    let genre_files: Vec<(String, GenreData)> = match load_all_genres(&genres_dir) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("Error loading genres: {}", e);
            std::process::exit(1);
        }
    };

    // List genres and exit
    if cli.list_genres {
        println!("Available genres:");
        for (file_name, genre) in &genre_files {
            let count = genre.entries.as_ref().map_or(genre.bands.len(), |e| e.len());
            let mode = if genre.entries.is_some() { "entries" } else { "bands" };
            println!("  {} ({} {})", file_name, count, mode);
        }
        return;
    }

    // Resolve genre
    let genre_name = match &cli.genre {
        Some(g) => g,
        None => {
            eprintln!("Error: --genre required. Use --list-genres to see available genres.");
            std::process::exit(1);
        }
    };

    let genre = match genre_files.iter().find(|(name, _)| name == genre_name) {
        Some((_, g)) => g.clone(),
        None => {
            eprintln!("Error: unknown genre '{genre_name}'. Use --list-genres to see available genres.");
            std::process::exit(1);
        }
    };

    // Build unified item list (bands or entries)
    let items: Vec<GenreItem> = if let Some(entries) = &genre.entries {
        entries.iter().map(|e| GenreItem::Entry(e.clone())).collect()
    } else {
        genre.bands.iter().map(|b| GenreItem::Band(b.clone())).collect()
    };
    let total_items = items.len();

    println!("━━━ Genre: {} ━━━", genre.genre);
    println!("Items: {} | Max songs: {} | Cookie: {}",
        total_items, cli.max_songs, cookie.as_ref().unwrap_or(&"<none>".into()));

    // Calculate per-item limit
    let per_item = cli.per_band.unwrap_or_else(|| {
        let p = cli.max_songs / total_items.max(1);
        p.max(1).min(100) // at least 1, at most 100
    });
    println!("Per-item limit: {}", per_item);

    // Build artist whitelist from genre entries (for validation layer)
    let valid_artists: HashSet<String> = if let Some(entries) = &genre.entries {
        entries.iter().map(|e| e.artist.to_lowercase()).collect()
    } else {
        genre.bands.iter().map(|b| b.to_lowercase()).collect()
    };
    eprintln!("Valid artists in genre: {}", valid_artists.len());

    // Build YT Music client
    let cookie_path = cookie.as_ref().unwrap();
    eprintln!("Authenticating...");
    let yt = match YtMusic::<BrowserToken>::from_cookie_file(cookie_path).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Auth error: {}", e);
            std::process::exit(1);
        }
    };
    eprintln!("Authenticated OK");

    // HTTP client + Last.fm key for dynamic genre validation
    let http_client = reqwest::Client::builder()
        .user_agent("genre-to-playlist/0.1.0")
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build reqwest client");
    let lastfm_key = genre_validator::get_lastfm_key();
    if lastfm_key.is_some() {
        eprintln!("Last.fm genre validation: enabled (from youtui config)");
    } else {
        eprintln!("Last.fm genre validation: disabled (set LASTFM_API_KEY or add [scrobbling].api_key to ~/.config/youtui/config.toml)");
    }
    eprintln!("MusicBrainz genre validation: enabled (fallback)");
    let mut genre_cache: HashMap<(String, String), genre_validator::GenreVerdict> = HashMap::new();
    let mut accepted_count = 0usize;
    let mut rejected_count = 0usize;
    let mut uncertain_count = 0usize;

    // Search songs for each item (or load from cache)
    let mut all_songs: Vec<SongEntry> = Vec::new();
    let cache_path = format!("{}_songs_cache.json", genre.genre);

    if cli.use_cache {
        match fs::read_to_string(&cache_path) {
            Ok(json) => {
                let cached: Vec<CachedEntry> = match serde_json::from_str(&json) {
                    Ok(c) => c,
                    Err(e) => { eprintln!("Cache parse error: {e}, re-searching"); Vec::new() }
                };
                if !cached.is_empty() {
                    all_songs = cached.into_iter().map(|c| c.into()).collect();
                    eprintln!("Loaded {} songs from cache ({}), skipping search", all_songs.len(), cache_path);
                }
            }
            Err(_) => eprintln!("No cache file found ({}), searching fresh", cache_path),
        }
    }

    if all_songs.is_empty() {
    let mut rate_limit_errors = 0u32;
    for (i, item) in items.iter().enumerate() {
        let (display, query, entry_opt) = match item {
            GenreItem::Band(band) => {
                (band.clone(), format!("{} {}", band, genre.genre), None)
            }
            GenreItem::Entry(entry) => {
                let q = if let Some(year) = entry.year {
                    format!("{} {} {}", entry.artist, entry.album, year)
                } else {
                    format!("{} {}", entry.artist, entry.album)
                };
                (format!("{} - {}", entry.artist, entry.album), q, Some(entry))
            }
        };
        // Step 1: Search YT Music (strict album match)
        let mut matched = match search_item_songs(&yt, &query, &display, entry_opt, per_item, &valid_artists, true).await {
            Ok(songs) => {
                rate_limit_errors = 0; // reset on success
                songs
            }
            Err(e) => {
                let is_rate_limit = e.contains("invalid json") || e.contains("column: 1");
                if is_rate_limit {
                    rate_limit_errors += 1;
                    let sleep_secs = 30u64 * rate_limit_errors as u64;
                    eprintln!("[{}/{}] ⏱ {} — rate limited ({e}), sleeping {sleep_secs}s", i + 1, total_items, display);
                    tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
                    // Retry once after backoff
                    rate_limit_errors = 0;
                    match search_item_songs(&yt, &query, &display, entry_opt, per_item, &valid_artists, true).await {
                        Ok(s) => s,
                        Err(e2) => {
                            eprintln!("[{}/{}] ✗ {} — search error after retry: {e2}", i + 1, total_items, display);
                            Vec::new()
                        }
                    }
                } else {
                    eprintln!("[{}/{}] ✗ {} — search error: {e}", i + 1, total_items, display);
                    Vec::new()
                }
            }
        };

        // Step 1b: If album search found nothing, try artist-only search
        let was_artist_fallback = if matched.is_empty() {
            if let Some(entry) = entry_opt {
                let artist_query = entry.artist.clone();
                match search_item_songs(&yt, &artist_query, &display, Some(entry), per_item, &valid_artists, false).await {
                    Ok(songs) if !songs.is_empty() => {
                        eprintln!("[{}/{}] ↵ {} — album not found, trying artist search", i + 1, total_items, display);
                        matched = songs;
                        true
                    }
                    _ => false,
                }
            } else {
                false
            }
        } else {
            false
        };

        if matched.is_empty() {
            eprintln!("[{}/{}] ✗ {} — not found on YT Music", i + 1, total_items, display);
            if i + 1 < total_items {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            continue;
        }

        // Step 2: Dynamic genre validation (per entry, cached)
        if let Some(entry) = entry_opt {
            let cache_key = (entry.artist.to_lowercase(), entry.album.to_lowercase());
            let verdict = if let Some(v) = genre_cache.get(&cache_key) {
                v.clone()
            } else {
                let v = genre_validator::validate(
                    &http_client,
                    &entry.artist,
                    &entry.album,
                    &genre.genre,
                    lastfm_key.as_deref(),
                ).await;
                genre_cache.insert(cache_key.clone(), v);
                genre_cache.get(&cache_key).unwrap().clone()
            };

            let total_for_entry = matched.len();
            match &verdict {
                genre_validator::GenreVerdict::Accept => {
                    all_songs.extend(matched);
                    // If we found via album search (not artist fallback), also do broad search
                    if !was_artist_fallback {
                        let broad_query = entry.artist.clone();
                        let broad_limit = per_item * 3;
                        let broad_count = if let Ok(broad_results) = search_item_songs(
                            &yt, &broad_query, &display, Some(entry), broad_limit,
                            &valid_artists, false,
                        ).await {
                            let n = broad_results.len();
                            all_songs.extend(broad_results);
                            n
                        } else { 0 };
                        accepted_count += 1;
                        eprintln!("[{}/{}] ✓ {} — {} album + {} broad = {} songs",
                            i + 1, total_items, display, total_for_entry, broad_count, total_for_entry + broad_count);
                    } else {
                        accepted_count += 1;
                        eprintln!("[{}/{}] ✓ {} — {} songs (from artist fallback)",
                            i + 1, total_items, display, total_for_entry);
                    }
                }
                genre_validator::GenreVerdict::Reject(reason) => {
                    rejected_count += 1;
                    eprintln!("[{}/{}] ✗ {} — {}", i + 1, total_items, display, reason);
                }
                genre_validator::GenreVerdict::Uncertain => {
                    all_songs.extend(matched);
                    uncertain_count += 1;
                    let label = if was_artist_fallback { " (artist fallback)" } else { "" };
                    eprintln!("[{}/{}] ? {} — {} songs (uncertain, added anyway{})",
                        i + 1, total_items, display, total_for_entry, label);
                }
            }
        } else {
            // Band mode: no dynamic validation, accept all
            let n = matched.len();
            all_songs.extend(matched);
            accepted_count += 1;
            eprintln!("[{}/{}] ✓ {} — {} songs", i + 1, total_items, display, n);
        }

        // Rate limit: sleep between searches
        if i + 1 < total_items {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    } // end if all_songs.is_empty()

    eprintln!("\nGenre validation: {} accepted, {} rejected, {} uncertain", accepted_count, rejected_count, uncertain_count);
    eprintln!("Found {} songs total from {} items", all_songs.len(), total_items);

    // Sample: ensure diversity
    let sampled = sample_songs(all_songs, cli.max_songs, per_item, cli.auto_split);

    if sampled.is_empty() {
        eprintln!("Error: no songs found for this genre");
        std::process::exit(1);
    }

    println!("Sampled {} songs for playlist (max {} per item)", sampled.len(), per_item);

    // Cache sampled songs for reuse (--use-cache on retry)
    {
        let cached: Vec<CachedEntry> = sampled.iter().map(|s| CachedEntry {
            video_id: s.video_id.get_raw().to_string(),
            title: s.title.clone(),
            artist: s.artist.clone(),
            source: s.source.clone(),
        }).collect();
        if let Ok(json) = serde_json::to_string_pretty(&cached) {
            let _ = fs::write(&cache_path, &json);
            eprintln!("Saved {} songs to cache ({})", cached.len(), cache_path);
        }
    }

    if cli.dry_run {
        println!("\n── Dry run ──");
        println!("Would create playlist: \"{}\"", cli.name.as_deref().unwrap_or(&format!("Genre: {}", genre.genre)));
        println!("Would add {} songs", sampled.len());
        // Show source diversity
        let mut source_counts: HashMap<&str, usize> = HashMap::new();
        for song in &sampled {
            *source_counts.entry(&song.source).or_insert(0) += 1;
        }
        let mut sorted: Vec<_> = source_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        println!("Item diversity (top 10):");
        for (source, count) in sorted.iter().take(10) {
            println!("  {:<30} {}", source, count);
        }
        println!("Total items represented: {}", sorted.len());
        return;
    }

    // Create or use existing playlist
    let mut playlist_id = if let Some(existing_id) = &cli.playlist_id {
        eprintln!("Using existing playlist: {}", existing_id);
        ytmapi_rs::common::PlaylistID::from_raw(existing_id)
    } else {
        let playlist_name = cli.name.clone().unwrap_or_else(|| format!("Genre: {}", genre.genre));
        let desc = genre.description.clone().unwrap_or_default();
        // Try to fetch description from Last.fm tag page if none in genre file
        let description = if desc.is_empty() {
            if let Some(ref key) = lastfm_key {
                match genre_validator::fetch_tag_description(&http_client, &genre.genre, key).await {
                    Some(tag_desc) => {
                        eprintln!("  Description from Last.fm: {}", &tag_desc[..tag_desc.len().min(80)]);
                        format!("{}{}", tag_desc, GEN_HEADER)
                    }
                    None => GEN_HEADER.to_string(),
                }
            } else {
                GEN_HEADER.to_string()
            }
        } else {
            format!("{}{}", desc, GEN_HEADER)
        };
        let privacy = match cli.privacy.as_str() {
            "public" => PrivacyStatus::Public,
            "unlisted" => PrivacyStatus::Unlisted,
            _ => PrivacyStatus::Private,
        };
        eprintln!("Creating playlist \"{}\"...", playlist_name);
        match yt.create_playlist(CreatePlaylistQuery::new(&playlist_name, Some(&description), privacy)).await {
            Ok(id) => {
                eprintln!("Created playlist: {}", id.get_raw());
                id
            }
            Err(e) => {
                eprintln!("Error creating playlist: {}", e);
                std::process::exit(1);
            }
        }
    };

    // Deduplicate against existing playlist tracks (handles pagination)
    eprintln!("Fetching existing playlist tracks (streaming all pages)...");
    let raw_id = playlist_id.get_raw().to_string();
    // Browse endpoint needs VL prefix
    let browse_id = if raw_id.starts_with("VL") { raw_id.clone() } else { format!("VL{}", raw_id) };
    let pid = ytmapi_rs::common::PlaylistID::from_raw(&browse_id);
    let existing_ids: HashSet<String> = match fetch_all_playlist_tracks(&yt, pid).await {
        Ok(tracks) => {
            let ids: HashSet<String> = tracks.iter().filter_map(playlist_item_video_id).collect();
            eprintln!("  {} tracks already in playlist", ids.len());
            ids
        }
        Err(e) => {
            eprintln!("  Warning: could not fetch existing tracks: {e} (proceeding without dedup)");
            HashSet::new()
        }
    };
    let total_sampled = sampled.len();
    let new_songs: Vec<SongEntry> = sampled
        .into_iter()
        .filter(|s| !existing_ids.contains(s.video_id.get_raw()))
        .collect();
    let skipped = total_sampled.saturating_sub(new_songs.len());
    if skipped > 0 {
        eprintln!("  Skipping {} songs already in playlist", skipped);
    }
    if new_songs.is_empty() {
        eprintln!("All songs already in playlist. Nothing to add.");
        return;
    }
    let sampled = new_songs;

    // Base playlist name for auto-split parts
    let base_title = cli.name.as_deref().unwrap_or(&format!("Genre: {}", genre.genre)).to_string();
    let privacy_status = match cli.privacy.as_str() {
        "public" => PrivacyStatus::Public,
        "unlisted" => PrivacyStatus::Unlisted,
        _ => PrivacyStatus::Private,
    };

    // Add songs in batches with DEDUPE_OPTION_SKIP (skip duplicates silently
    // instead of failing the entire batch) and longer retry backoff.
    // Auto-split only on 4900 cap, never on failure — root cause of tiny parts.
    const MAX_PER_PART: u32 = 5000;
    let batch_size = 50;
    let mut added_total = 0u32;
    let mut failed_total = 0u32;
    let mut part = 1u32;
    let mut part_added = 0u32;  // tracks songs in current part for split logic
    for chunk in sampled.chunks(batch_size) {
        let video_ids: Vec<VideoID<'_>> = chunk.iter().map(|s| s.video_id.clone()).collect();

        // Auto-split: create new part BEFORE sending batch if current part is nearly full
        if cli.auto_split && part_added >= MAX_PER_PART && !video_ids.is_empty() {
            part += 1;
            let part_name = format!("{} (Pt. {})", base_title, part);
            let desc = format!("Continuation of {}{}", base_title, GEN_HEADER);
            eprintln!("  Part {} at {} songs, creating next part: \"{}\"...", part - 1, part_added, part_name);
            match yt.create_playlist(CreatePlaylistQuery::new(&part_name, Some(&desc), privacy_status.clone())).await {
                Ok(new_id) => {
                    eprintln!("  Created: {}", new_id.get_raw());
                    playlist_id = new_id;
                    part_added = 0;
                }
                Err(e) => {
                    eprintln!("  Failed to create next part: {} (stopping add)", e);
                    break;
                }
            }
        }

        let mut last_err = String::new();
        let mut success = false;
        // STATUS_FAILED → 5s/15s/45s backoff (rate limiting)
        // HTTP errors → instant retry
        for attempt in 1..=3 {
            eprintln!(
                "Adding batch of {} songs... (attempt {}/3)",
                video_ids.len(),
                attempt
            );
            let query = AddPlaylistItemsQuery::new_from_videos(
                playlist_id.clone(),
                video_ids.clone(),
                DuplicateHandlingMode::Unhandled,
            );
            match yt.query(query).await {
                Ok(results) => {
                    eprintln!("  {} added (duplicates silently skipped)", results.len());
                    added_total += results.len() as u32;
                    part_added += results.len() as u32;
                    success = true;
                    break;
                }
                Err(e) => {
                    last_err = format!("{}", e);
                    let delay = if e.to_string().contains("STATUS_FAILED") {
                        match attempt { 1 => 5, 2 => 15, _ => 45 }
                    } else {
                        match attempt { 1 => 1, 2 => 3, _ => 9 }
                    };
                    eprintln!("  failed: {} (retry in {}s)", last_err, delay);
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                }
            }
        }
        if !success {
            // Never auto-split on failure. Just log and move on.
            eprintln!("  Failed after 3 attempts: {}\n  {} songs skipped", last_err, video_ids.len());
            failed_total += video_ids.len() as u32;
        }
        // Throttle: ramp delay as more songs are added
        let delay = if added_total > 1000 { 1500 } else if added_total > 500 { 1000 } else { 750 };
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    eprintln!("Add phase done: {} added, {} failed", added_total, failed_total);

    println!("\n━━━ Done ━━━");
    println!("Playlist ID: {}", playlist_id.get_raw());
    println!("Songs: {}", sampled.len());
    println!("Items: {}", total_items);

    // Show first 5 songs as sample
    println!("\nSample songs:");
    for (i, song) in sampled.iter().take(5).enumerate() {
        println!("  {}. {} - {} ({})", i + 1, song.artist, song.title, song.source);
    }
}

// ─── Genre loading ──────────────────────────────────────────────────────

fn load_all_genres(dir: &PathBuf) -> Result<Vec<(String, GenreData)>, String> {
    if !dir.exists() {
        return Err(format!("genres dir not found: {}", dir.display()));
    }
    let mut genres = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read dir: {e}"))? {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let data = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let genre: GenreData = serde_json::from_str(&data).map_err(|e| format!("parse {}: {e}", path.display()))?;
        genres.push((file_name, genre));
    }
    if genres.is_empty() {
        return Err("no genre JSON files found".into());
    }
    Ok(genres)
}

// ─── Search ─────────────────────────────────────────────────────────────

async fn search_item_songs(
    yt: &YtMusic<BrowserToken>,
    query: &str,
    display_name: &str,
    entry: Option<&GenreEntry>,
    limit: usize,
    valid_artists: &HashSet<String>,
    album_match_required: bool,
) -> Result<Vec<SongEntry>, String> {
    let results = yt.search_songs(query).await.map_err(|e| format!("search error: {e}"))?;
    let matched: Vec<SongEntry> = results
        .into_iter()
        .filter(|s| {
            let artist = s.artist.to_lowercase();

            // LAYER 3: Pop blocklist — hard reject known mainstream artists
            if POP_BLOCKLIST.iter().any(|pop| artist.contains(pop)) {
                return false;
            }

            if let Some(entry) = entry {
                // LAYER 2: Artist whitelist — must match a known genre artist
                let valid_artist = valid_artists.iter().any(|va| {
                    artist.contains(va) || va.contains(&artist)
                });
                if !valid_artist {
                    return false;
                }

                if album_match_required {
                    // LAYER 1: Strict album match (for unverified entries — prevents pop leaks)
                    s.album.as_ref().map_or(false, |a|
                        a.name.to_lowercase().contains(&entry.album.to_lowercase())
                    )
                } else {
                    // Broad match (for genre-verified entries — accept any song by artist)
                    true
                }
            } else {
                // Band-based: artist or title contains the band name
                let band_lower = display_name.to_lowercase();
                s.artist.to_lowercase().contains(&band_lower)
                    || s.title.to_lowercase().contains(&band_lower)
            }
        })
        .take(limit)
        .map(|s| SongEntry {
            video_id: s.video_id,
            title: s.title,
            artist: s.artist,
            source: display_name.to_string(),
        })
        .collect();
    Ok(matched)
}

// ─── Playlist track fetching (with pagination) ──────────────────────

/// Fetch ALL tracks from a playlist, handling YT Music continuation/pagination.
async fn fetch_all_playlist_tracks(
    yt: &YtMusic<BrowserToken>,
    pid: ytmapi_rs::common::PlaylistID<'_>,
) -> Result<Vec<PlaylistItem>, String> {
    use futures::TryStreamExt;
    let query = GetPlaylistTracksQuery::new(pid);
    let pages: Vec<Vec<PlaylistItem>> = yt
        .stream(&query)
        .try_collect()
        .await
        .map_err(|e| format!("fetch playlist tracks: {e}"))?;
    let num_pages = pages.len();
    let all: Vec<PlaylistItem> = pages.into_iter().flatten().collect();
    eprintln!("  Fetched {} total items across {} pages", all.len(), num_pages);
    Ok(all)
}

/// Extract video_id string from a PlaylistItem variant.
fn playlist_item_video_id(item: &PlaylistItem) -> Option<String> {
    match item {
        PlaylistItem::Song(s) => Some(s.video_id.get_raw().to_string()),
        PlaylistItem::Video(v) => Some(v.video_id.get_raw().to_string()),
        PlaylistItem::UploadSong(u) => Some(u.video_id.get_raw().to_string()),
        PlaylistItem::Episode(_) => None,
    }
}

// ─── Sampling ───────────────────────────────────────────────────────────

fn sample_songs(all_songs: Vec<SongEntry>, max_songs: usize, per_item: usize, auto_split: bool) -> Vec<SongEntry> {
    // Group songs by source (band or artist name)
    let mut by_source: HashMap<String, Vec<SongEntry>> = HashMap::new();
    for song in all_songs {
        by_source.entry(song.source.clone()).or_default().push(song);
    }

    // Shuffle each source's songs for variety, take per_item from each
    let mut rng = thread_rng();
    let mut sampled: Vec<SongEntry> = Vec::new();

    for (_src, songs) in by_source.iter_mut() {
        songs.shuffle(&mut rng);
        let take = songs.len().min(per_item);
        sampled.extend(songs.drain(..take));
    }

    // Shuffle all selected songs (interleave sources)
    sampled.shuffle(&mut rng);

    // Cap at max_songs (unless auto-split: let parts handle the limits)
    if !auto_split {
        sampled.truncate(max_songs);
    }

    sampled
}
