use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use std::fs;

use clap::Parser;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::Deserialize;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::common::{VideoID, YoutubeID};
use ytmapi_rs::query::playlist::PrivacyStatus;
use ytmapi_rs::query::CreatePlaylistQuery;
use ytmapi_rs::YtMusic;

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

    // Search songs for each item
    let mut all_songs: Vec<SongEntry> = Vec::new();

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
        eprint!("\rSearching {}/{}: {:<50}", i + 1, total_items, display);

        match search_item_songs(&yt, &query, &display, entry_opt, per_item, &valid_artists).await {
            Ok(songs) => all_songs.extend(songs),
            Err(e) => eprintln!("\n  Warning: search failed for {}: {e}", display),
        }

        // Rate limit: sleep between searches
        if i + 1 < total_items {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    eprintln!("\nFound {} songs total from {} items", all_songs.len(), total_items);

    // Sample: ensure diversity
    let sampled = sample_songs(all_songs, cli.max_songs, per_item);

    if sampled.is_empty() {
        eprintln!("Error: no songs found for this genre");
        std::process::exit(1);
    }

    println!("Sampled {} songs for playlist (max {} per item)", sampled.len(), per_item);

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
    let playlist_id = if let Some(existing_id) = &cli.playlist_id {
        eprintln!("Using existing playlist: {}", existing_id);
        ytmapi_rs::common::PlaylistID::from_raw(existing_id)
    } else {
        let playlist_name = cli.name.unwrap_or_else(|| format!("Genre: {}", genre.genre));
        let desc = genre.description.unwrap_or_default();
        let description = format!("{}{}", desc, GEN_HEADER);
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

    // Deduplicate against existing playlist tracks
    eprintln!("Fetching existing playlist tracks...");
    let raw_id = playlist_id.get_raw().to_string();
    // Browse endpoint needs VL prefix
    let browse_id = if raw_id.starts_with("VL") { raw_id.clone() } else { format!("VL{}", raw_id) };
    let pid = ytmapi_rs::common::PlaylistID::from_raw(&browse_id);
    let existing_ids: HashSet<String> = match yt.get_playlist_tracks(pid).await {
        Ok(tracks) => {
            use ytmapi_rs::parse::PlaylistItem;
            let ids: HashSet<String> = tracks.iter().filter_map(|t| {
                match t {
                    PlaylistItem::Song(s) => Some(s.video_id.get_raw().to_string()),
                    PlaylistItem::Video(v) => Some(v.video_id.get_raw().to_string()),
                    PlaylistItem::UploadSong(u) => Some(u.video_id.get_raw().to_string()),
                    PlaylistItem::Episode(_) => None,
                }
            }).collect();
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

    // Add songs in batches with retry + backoff
    let batch_size = 50;
    let mut added_total = 0u32;
    let mut failed_total = 0u32;
    for chunk in sampled.chunks(batch_size) {
        let video_ids: Vec<VideoID<'_>> = chunk.iter().map(|s| s.video_id.clone()).collect();
        let mut last_err = String::new();
        let mut success = false;
        for attempt in 1..=3 {
            eprintln!(
                "Adding batch of {} songs... (attempt {}/3)",
                video_ids.len(),
                attempt
            );
            match yt.add_video_items_to_playlist(playlist_id.clone(), video_ids.clone()).await {
                Ok(results) => {
                    eprintln!("  {} added", results.len());
                    added_total += results.len() as u32;
                    success = true;
                    break;
                }
                Err(e) => {
                    last_err = format!("{}", e);
                    eprintln!("  failed: {} (retry in {}s)", last_err, match attempt { 1 => 1, 2 => 3, _ => 9 });
                    tokio::time::sleep(Duration::from_secs(match attempt { 1 => 1, 2 => 3, _ => 9 })).await;
                }
            }
        }
        if !success {
            eprintln!("  STATUS_FAILED after 3 attempts: {}\n  {} songs skipped", last_err, video_ids.len());
            failed_total += video_ids.len() as u32;
        }
        // Throttle: ramp delay as more songs are added
        let delay = if added_total > 1000 { 1000 } else if added_total > 500 { 750 } else { 500 };
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
) -> Result<Vec<SongEntry>, String> {
    // For entries mode: only accept album matches. If album search fails or
    // returns no matches, skip the entry entirely. No artist-only fallback,
    // no artist-name filter — that's how Katy Perry leaks into sasscore.
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

                // LAYER 1: Album must match (case-insensitive)
                s.album.as_ref().map_or(false, |a|
                    a.name.to_lowercase().contains(&entry.album.to_lowercase())
                )
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

// ─── Sampling ───────────────────────────────────────────────────────────

fn sample_songs(all_songs: Vec<SongEntry>, max_songs: usize, per_item: usize) -> Vec<SongEntry> {
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

    // Cap at max_songs
    sampled.truncate(max_songs);

    sampled
}
