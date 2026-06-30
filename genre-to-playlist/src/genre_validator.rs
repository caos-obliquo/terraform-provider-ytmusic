use std::time::Duration;

// ─── Genre validation via Last.fm + MusicBrainz ──────────────────────────

const LASTFM_API: &str = "https://ws.audioscrobbler.com/2.0/";
const MUSICBRAINZ_API: &str = "https://musicbrainz.org/ws/2/";
const USER_AGENT: &str = "genre-to-playlist/0.1.0 (caos-obliquo)";

/// Result of genre validation for a candidate song
#[derive(Debug, Clone)]
pub enum GenreVerdict {
    /// Genre tags match the target genre
    Accept,
    /// No positive genre tags found; optional reject_reason
    Reject(String),
    /// Could not check (no API response etc.) — fail open
    Uncertain,
}

/// Genre keyword sets for positive/negative matching.
/// Positive: any match = Accept. Negative: any match + no positive = Reject.
pub struct GenreClassifier {
    /// Substrings that indicate a match (lowercase). "sasscore", "hardcore", ...
    positive: Vec<String>,
    /// Substrings that indicate a reject (lowercase). "pop", "hip hop", ...
    negative: Vec<String>,
}

impl GenreClassifier {
    /// Build classifier from target genre name + optional overrides
    pub fn for_genre(target: &str) -> Self {
        let mut pos: Vec<String> = vec![
            target.to_lowercase(),
            "screamo".into(),
            "hardcore".into(),
            "powerviolence".into(),
            "grindcore".into(),
            "goregrind".into(),
            "gorenoise".into(),
            "mincecore".into(),
            "porngrind".into(),
            "pornogrind".into(),
            "cybergrind".into(),
            "metalcore".into(),
            "mathcore".into(),
            "emocore".into(),
            "emo".into(),
            "punk".into(),
            "crust".into(),
            "noise".into(),
            "experimental".into(),
            "chaotic".into(),
            "metal".into(),
            "heavy".into(),
            "extreme".into(),
            "death".into(),
            "brutal".into(),
            "sludge".into(),
            "drone".into(),
            "doom".into(),
            "blackened".into(),
            "underground".into(),
            "avant".into(),
            "gore".into(),
            "splatter".into(),
            "cannibal".into(),
            "putrid".into(),
            "necro".into(),
        ];
        // Add target-derived keywords
        let lower = target.to_lowercase();
        if lower.contains("core") || lower.contains("grind") {
            pos.push("core".into());
        }
        if lower.contains("grind") || lower.contains("gore") || lower.contains("noise") {
            pos.push("grind".into());
        }
        if lower.contains("gore") {
            pos.push("gore".into());
        }
        if lower.contains("death") {
            pos.push("death".into());
        }

        let neg: Vec<String> = vec![
            "pop".into(),
            "r&b".into(),
            "hip hop".into(),
            "rap".into(),
            "trap".into(),
            "country".into(),
            "edm".into(),
            "electronic".into(),
            "dance".into(),
            "reggae".into(),
            "jazz".into(),
            "blues".into(),
            "soul".into(),
            "funk".into(),
            "latin".into(),
            "k-pop".into(),
            "j-pop".into(),
            "classical".into(),
            "ambient".into(),
        ];

        Self { positive: pos, negative: neg }
    }

    fn has_positive(&self, tags: &[String]) -> bool {
        let all_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
        for tag in &all_lower {
            for pat in &self.positive {
                if tag.contains(pat) {
                    return true;
                }
            }
        }
        false
    }

    fn has_negative(&self, tags: &[String]) -> bool {
        let all_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
        for tag in &all_lower {
            for pat in &self.negative {
                if tag.contains(pat) {
                    return true;
                }
            }
        }
        false
    }

    pub fn classify(&self, tags: &[String]) -> GenreVerdict {
        if self.has_positive(tags) {
            GenreVerdict::Accept
        } else if self.has_negative(tags) {
            GenreVerdict::Reject(format!("negative genre tag(s) in {:?}", tags))
        } else {
            GenreVerdict::Uncertain
        }
    }
}

/// Validate a song's genre by querying Last.fm then MusicBrainz.
/// Returns Accept, Reject, or Uncertain.
pub async fn validate(
    client: &reqwest::Client,
    artist: &str,
    album: &str,
    target: &str,
    lastfm_key: Option<&str>,
) -> GenreVerdict {
    let classifier = GenreClassifier::for_genre(target);

    // 1. Try Last.fm album tags
    if let Some(key) = lastfm_key {
        if !key.is_empty() {
            match check_lastfm(client, key, artist, album).await {
                Some(tags) => {
                    let verdict = classifier.classify(&tags);
                    match &verdict {
                        GenreVerdict::Accept => return verdict,
                        GenreVerdict::Reject(_) => return verdict,
                        GenreVerdict::Uncertain => {} // fall through to MusicBrainz
                    }
                }
                None => {} // API error, fall through
            }
        }
    }

    // 2. Fallback: MusicBrainz (no API key needed)
    match check_musicbrainz(client, artist, album).await {
        Some(tags) => classifier.classify(&tags),
        None => GenreVerdict::Uncertain, // all APIs failed
    }
}

// ─── Last.fm ─────────────────────────────────────────────────────────────

async fn check_lastfm(
    client: &reqwest::Client,
    api_key: &str,
    artist: &str,
    album: &str,
) -> Option<Vec<String>> {
    let url = format!(
        "{}?method=album.getInfo&api_key={}&artist={}&album={}&format=json&autocorrect=1",
        LASTFM_API,
        urlencoding(api_key),
        urlencoding(artist),
        urlencoding(album),
    );

    let resp = client.get(&url).timeout(Duration::from_secs(5)).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: serde_json::Value = resp.json().await.ok()?;
    let tags = data.get("album")?.get("tags")?.get("tag")?.as_array()?;
    let out: Vec<String> = tags
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str().map(String::from)))
        .collect();
    if out.is_empty() { None } else { Some(out) }
}

// ─── MusicBrainz ─────────────────────────────────────────────────────────

async fn check_musicbrainz(
    client: &reqwest::Client,
    artist: &str,
    album: &str,
) -> Option<Vec<String>> {
    // Rate limit: 1 req/s (MusicBrainz rule)
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Step 1: Find the artist
    let artist_query = format!(
        "{}artist/?query=artist:{}&limit=5&fmt=json",
        MUSICBRAINZ_API,
        urlencoding(&artist.trim().to_lowercase()),
    );
    let resp = client
        .get(&artist_query)
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;
    let artists = data.get("artists")?.as_array()?;
    let artist_mbid = artists.first()?.get("id")?.as_str()?;

    // Step 2: Find the release (album)
    let album_query = format!(
        "{}release/?query=artistid:{}%20AND%20release:{}&limit=5&fmt=json",
        MUSICBRAINZ_API,
        urlencoding(artist_mbid),
        urlencoding(&album.trim().to_lowercase()),
    );
    tokio::time::sleep(Duration::from_millis(500)).await; // stay under rate limit
    let resp = client
        .get(&album_query)
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;
    let releases = data.get("releases")?.as_array()?;
    let release_mbid = releases.first()?.get("id")?.as_str()?;

    // Step 3: Get release with tags + genres
    let release_url = format!(
        "{}release/{}?inc=tags+genres&fmt=json",
        MUSICBRAINZ_API,
        urlencoding(release_mbid),
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    let resp = client
        .get(&release_url)
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;

    // Collect genres (moderated) + tags (user-generated)
    let mut all: Vec<String> = Vec::new();
    if let Some(genres) = data.get("genres").and_then(|g| g.as_array()) {
        for g in genres {
            if let Some(name) = g.get("name").and_then(|n| n.as_str()) {
                all.push(name.to_string());
            }
        }
    }
    if let Some(tags) = data.get("tags").and_then(|t| t.as_array()) {
        for t in tags {
            if let Some(name) = t.get("name").and_then(|n| n.as_str()) {
                all.push(name.to_string());
            }
        }
    }

    if all.is_empty() { None } else { Some(all) }
}

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Get Last.fm API key from youtui config or env var.
/// Tries: ~/.config/youtui/config.toml → [scrobbling].api_key → $LASTFM_API_KEY
pub fn get_lastfm_key() -> Option<String> {
    // 1. Try youtui config file
    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("youtui").join("config.toml");
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = content.parse::<toml::Value>() {
                if let Some(key) = config
                    .get("scrobbling")
                    .and_then(|s| s.get("api_key"))
                    .and_then(|k| k.as_str())
                {
                    if !key.is_empty() {
                        return Some(key.to_string());
                    }
                }
            }
        }
    }
    // 2. Fallback to env var
    std::env::var("LASTFM_API_KEY").ok()
}

/// Fetch a genre description from Last.fm tag page wiki.
/// Returns the first paragraph (before first newline) of the wiki summary.
pub async fn fetch_tag_description(client: &reqwest::Client, tag: &str, api_key: &str) -> Option<String> {
    let url = format!(
        "{}?method=tag.getInfo&api_key={}&tag={}&format=json",
        LASTFM_API,
        urlencoding(api_key),
        urlencoding(tag),
    );
    let resp = client.get(&url).timeout(Duration::from_secs(5)).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: serde_json::Value = resp.json().await.ok()?;
    let wiki = data.get("tag")?.get("wiki")?;
    let summary = wiki.get("summary")?.as_str()?;
    // Take just the first paragraph
    let first = summary.split("\n\n").next().unwrap_or(summary);
    let cleaned = first
        .replace("<a href=\"", "")
        .replace("\" rel=\"nofollow\">", "")
        .replace("</a>", "")
        .trim()
        .to_string();
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            _ => {
                out.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    out
}
