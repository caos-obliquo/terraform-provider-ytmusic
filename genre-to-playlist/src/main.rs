use std::collections::HashMap;
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
}

// ─── Genre data ─────────────────────────────────────────────────────────

#[derive(Deserialize, Clone)]
struct GenreData {
    genre: String,
    description: Option<String>,
    bands: Vec<String>,
}

// ─── Song tracking ──────────────────────────────────────────────────────

struct SongEntry {
    video_id: VideoID<'static>,
    title: String,
    artist: String,
    band: String,
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
            let band_count = genre.bands.len();
            println!("  {} ({})", file_name, band_count);
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

    println!("━━━ Genre: {} ━━━", genre.genre);
    println!("Bands: {} | Max songs: {} | Cookie: {}",
        genre.bands.len(), cli.max_songs, cookie.as_ref().unwrap_or(&"<none>".into()));

    // Calculate per-band limit
    let per_band = cli.per_band.unwrap_or_else(|| {
        let p = cli.max_songs / genre.bands.len().max(1);
        p.max(1).min(100) // at least 1, at most 100
    });
    println!("Per-band limit: {}", per_band);

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

    // Search songs for each band
    let mut all_songs: Vec<SongEntry> = Vec::new();
    let total_bands = genre.bands.len();

    for (i, band) in genre.bands.iter().enumerate() {
        let query = format!("{} {}", band, genre.genre);
        eprint!("\rSearching {}/{}: {:<40}", i + 1, total_bands, band);

        match search_band_songs(&yt, &query, band, per_band).await {
            Ok(songs) => all_songs.extend(songs),
            Err(e) => eprintln!("\n  Warning: search failed for {band}: {e}"),
        }

        // Rate limit: sleep between searches
        if i + 1 < total_bands {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    eprintln!("\nFound {} songs total from {} bands", all_songs.len(), total_bands);

    // Sample: ensure diversity
    let sampled = sample_songs(all_songs, cli.max_songs, per_band);

    if sampled.is_empty() {
        eprintln!("Error: no songs found for this genre");
        std::process::exit(1);
    }

    println!("Sampled {} songs for playlist (max {} per band)", sampled.len(), per_band);

    if cli.dry_run {
        println!("\n── Dry run ──");
        println!("Would create playlist: \"{}\"", cli.name.as_deref().unwrap_or(&format!("Genre: {}", genre.genre)));
        println!("Would add {} songs", sampled.len());
        // Show band diversity
        let mut band_counts: HashMap<&str, usize> = HashMap::new();
        for song in &sampled {
            *band_counts.entry(&song.band).or_insert(0) += 1;
        }
        let mut sorted: Vec<_> = band_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        println!("Band diversity (top 10):");
        for (band, count) in sorted.iter().take(10) {
            println!("  {:<25} {}", band, count);
        }
        println!("Total bands represented: {}", sorted.len());
        return;
    }

    // Create playlist
    let playlist_name = cli.name.unwrap_or_else(|| format!("Genre: {}", genre.genre));
    let description = genre.description.unwrap_or_default();
    let privacy = match cli.privacy.as_str() {
        "public" => PrivacyStatus::Public,
        "unlisted" => PrivacyStatus::Unlisted,
        _ => PrivacyStatus::Private,
    };

    eprintln!("Creating playlist \"{}\"...", playlist_name);
    let playlist_id = match yt.create_playlist(CreatePlaylistQuery::new(&playlist_name, Some(&description), privacy)).await {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error creating playlist: {}", e);
            std::process::exit(1);
        }
    };
    eprintln!("Created playlist: {}", playlist_id.get_raw());

    // Add songs in batches of 100
    let batch_size = 100;
    for chunk in sampled.chunks(batch_size) {
        let video_ids: Vec<VideoID<'_>> = chunk.iter().map(|s| s.video_id.clone()).collect();
        eprint!("Adding batch of {} songs...", video_ids.len());
        match yt.add_video_items_to_playlist(playlist_id.clone(), video_ids).await {
            Ok(results) => eprintln!(" {} added", results.len()),
            Err(e) => eprintln!(" error: {}", e),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    println!("\n━━━ Done ━━━");
    println!("Playlist: \"{}\"", playlist_name);
    println!("Playlist ID: {}", playlist_id.get_raw());
    println!("Songs: {}", sampled.len());
    println!("Bands: {}", total_bands);

    // Show first 5 songs as sample
    println!("\nSample songs:");
    for (i, song) in sampled.iter().take(5).enumerate() {
        println!("  {}. {} - {} ({})", i + 1, song.artist, song.title, song.band);
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

async fn search_band_songs(
    yt: &YtMusic<BrowserToken>,
    query: &str,
    band: &str,
    limit: usize,
) -> Result<Vec<SongEntry>, String> {
    let results = yt.search_songs(query).await.map_err(|e| format!("search error: {e}"))?;
    let entries: Vec<SongEntry> = results
        .into_iter()
        .filter(|s| {
            // Filter: only include songs where the artist name contains the band name
            // (case-insensitive) to avoid genre cross-contamination
            s.artist.to_lowercase().contains(&band.to_lowercase())
                || s.title.to_lowercase().contains(&band.to_lowercase())
        })
        .take(limit)
        .map(|s| SongEntry {
            video_id: s.video_id,
            title: s.title,
            artist: s.artist,
            band: band.to_string(),
        })
        .collect();
    Ok(entries)
}

// ─── Sampling ───────────────────────────────────────────────────────────

fn sample_songs(all_songs: Vec<SongEntry>, max_songs: usize, per_band: usize) -> Vec<SongEntry> {
    // Group songs by band
    let mut by_band: HashMap<String, Vec<SongEntry>> = HashMap::new();
    for song in all_songs {
        by_band.entry(song.band.clone()).or_default().push(song);
    }

    // Shuffle each band's songs for variety, take per_band from each
    let mut rng = thread_rng();
    let mut sampled: Vec<SongEntry> = Vec::new();

    for (_band, songs) in by_band.iter_mut() {
        songs.shuffle(&mut rng);
        let take = songs.len().min(per_band);
        sampled.extend(songs.drain(..take));
    }

    // Shuffle all selected songs (interleave bands)
    sampled.shuffle(&mut rng);

    // Cap at max_songs
    sampled.truncate(max_songs);

    sampled
}
