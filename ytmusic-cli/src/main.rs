use std::io::{self, Read, Write};
use std::collections::BTreeMap;
use ytmapi_rs::{
    YtMusic,
    common::{PlaylistID, SetVideoID, VideoID, YoutubeID},
    query::{
        CreatePlaylistQuery,
        EditPlaylistQuery,
        GetPlaylistTracksQuery,
        playlist::{AddPlaylistItemsQuery, DuplicateHandlingMode, PrivacyStatus},
    },
};
use serde::{Deserialize, Serialize};

// ─── Machine protocol: JSON stdin/stdout ───

#[derive(Deserialize)]
struct Request {
    action: String,
    cookie_file: Option<String>,
    payload: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Response {
    success: bool,
    data: Option<serde_json::Value>,
    error: Option<String>,
}

impl Response {
    fn ok(data: serde_json::Value) -> Self {
        Self { success: true, data: Some(data), error: None }
    }
    fn err(msg: impl Into<String>) -> Self {
        Self { success: false, data: None, error: Some(msg.into()) }
    }
}

#[tokio::main]
async fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).ok();

    // Support both stdin JSON and CLI args for flexibility
    let req: Request = if !input.trim().is_empty() {
        serde_json::from_str(&input).unwrap_or_else(|e| {
            eprintln!("{{\"error\":\"invalid JSON input: {}\"}}", e);
            std::process::exit(1);
        })
    } else {
        let args: Vec<String> = std::env::args().collect();
        if args.len() < 2 {
            eprintln!("Usage: ytmusic-cli <action> [--cookie <file>] [payload JSON]");
            eprintln!("Or pipe JSON: echo '{{\"action\":\"...\"}}' | ytmusic-cli");
            std::process::exit(1);
        }
        let mut cookie_file = std::env::var("YTMAPI_COOKIE").ok();
        let mut payload: Option<serde_json::Value> = None;
        let mut action = String::new();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--cookie" => { i += 1; cookie_file = Some(args.get(i).cloned().unwrap_or_default()); }
                _ if action.is_empty() => action = args[i].clone(),
                _ => {
                    payload = serde_json::from_str(&args[i]).ok();
                    break;
                }
            }
            i += 1;
        }
        Request { action, cookie_file, payload }
    };

    let result = match req.action.as_str() {
        "auth-check" => cmd_auth_check(req.cookie_file.as_deref()).await,
        "playlist-list" => cmd_playlist_list(req.cookie_file.as_deref()).await,
        "playlist-get" => cmd_playlist_get(req.cookie_file.as_deref(), req.payload).await,
        "playlist-create" => cmd_playlist_create(req.cookie_file.as_deref(), req.payload).await,
        "playlist-delete" => cmd_playlist_delete(req.cookie_file.as_deref(), req.payload).await,
        "playlist-edit" => cmd_playlist_edit(req.cookie_file.as_deref(), req.payload).await,
        "playlist-add-items" => cmd_playlist_add_items(req.cookie_file.as_deref(), req.payload).await,
        "playlist-remove-items" => cmd_playlist_remove_items(req.cookie_file.as_deref(), req.payload).await,
        "playlist-tracks" => cmd_playlist_tracks(req.cookie_file.as_deref(), req.payload).await,
        "playlist-remove-artist" => cmd_playlist_remove_artist(req.cookie_file.as_deref(), req.payload).await,
        "playlist-clean" => cmd_playlist_clean(req.cookie_file.as_deref(), req.payload).await,
        "debug-browse" => cmd_debug_browse(req.cookie_file.as_deref(), req.payload).await,
        "raw-browse" => cmd_raw_browse(req.cookie_file.as_deref(), req.payload).await,
        "search" => cmd_search(req.cookie_file.as_deref(), req.payload).await,
        _ => Response::err(format!("unknown action: {}", req.action)),
    };

    println!("{}", serde_json::to_string(&result).unwrap());
}

// ── Auth ────────────────────────────────────────────────────────────────

async fn cmd_auth_check(cookie: Option<&str>) -> Response {
    match build_client(cookie).await {
        Ok(_) => Response::ok(serde_json::json!({"status": "authenticated"})),
        Err(e) => Response::err(e),
    }
}

// ── Playlist CRUD ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct PlaylistSummary {
    id: String,
    title: String,
    description: Option<String>,
    track_count: Option<u32>,
}

async fn cmd_playlist_list(cookie: Option<&str>) -> Response {
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    match yt.get_library_playlists().await {
        Ok(playlists) => {
            let list: Vec<PlaylistSummary> = playlists.iter().map(|p| PlaylistSummary {
                id: p.playlist_id.get_raw().to_string(),
                title: p.title.clone(),
                description: None,
                track_count: None,
            }).collect();
            Response::ok(serde_json::to_value(list).unwrap())
        }
        Err(e) => Response::err(format!("playlist list error: {}", e)),
    }
}

#[derive(Deserialize)]
struct PlaylistGetPayload {
    id: String,
}

async fn cmd_playlist_get(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistGetPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    // get_playlist_details expects browseId with VL prefix
    let browse_id = if p.id.starts_with("VL") {
        p.id.clone()
    } else {
        format!("VL{}", p.id)
    };
    let pid = PlaylistID::from_raw(&browse_id);
    match yt.get_playlist_details(pid).await {
        Ok(details) => Response::ok(serde_json::to_value(&details).unwrap_or_default()),
        Err(e) => Response::err(format!("playlist get error: {}", e)),
    }
}

#[derive(Deserialize)]
struct PlaylistCreatePayload {
    title: String,
    description: Option<String>,
    privacy: Option<String>,
}

async fn cmd_playlist_create(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistCreatePayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'title' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let privacy = match p.privacy.as_deref() {
        Some("public") => PrivacyStatus::Public,
        Some("unlisted") => PrivacyStatus::Unlisted,
        _ => PrivacyStatus::Private,
    };
    match yt.create_playlist(CreatePlaylistQuery::new(&p.title, p.description.as_deref(), privacy)).await {
        Ok(id) => Response::ok(serde_json::json!({"id": id.get_raw()})),
        Err(e) => Response::err(format!("playlist create error: {}", e)),
    }
}

#[derive(Deserialize)]
struct PlaylistDeletePayload {
    id: String,
}

async fn cmd_playlist_delete(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistDeletePayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let pid = PlaylistID::from_raw(&p.id);
    match yt.delete_playlist(pid).await {
        Ok(_) => Response::ok(serde_json::json!({"deleted": true})),
        Err(e) => Response::err(format!("playlist delete error: {}", e)),
    }
}

#[derive(Deserialize)]
struct PlaylistEditPayload {
    id: String,
    title: Option<String>,
    description: Option<String>,
    privacy: Option<String>,
}

async fn cmd_playlist_edit(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistEditPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let pid = PlaylistID::from_raw(&p.id);
    let mut query = if let Some(ref t) = p.title {
        EditPlaylistQuery::new_title(&pid, t)
    } else {
        EditPlaylistQuery::new_title(&pid, "")
    };
    if let Some(d) = &p.description { query = query.with_new_description(d); }
    if let Some(pr) = &p.privacy {
        query = query.with_new_privacy_status(match pr.as_str() {
            "public" => PrivacyStatus::Public,
            "unlisted" => PrivacyStatus::Unlisted,
            _ => PrivacyStatus::Private,
        });
    }
    match yt.edit_playlist(query).await {
        Ok(_) => Response::ok(serde_json::json!({"edited": true})),
        Err(e) => Response::err(format!("playlist edit error: {}", e)),
    }
}

#[derive(Deserialize)]
struct PlaylistAddItemsPayload {
    id: String,
    video_ids: Vec<String>,
}

async fn cmd_playlist_add_items(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistAddItemsPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' and 'video_ids' fields"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let pid = PlaylistID::from_raw(&p.id);
    let vids: Vec<VideoID<'_>> = p.video_ids.iter().map(|v| VideoID::from_raw(v.clone())).collect();
    // Use Unhandled (DEDUPE_OPTION_SKIP) — duplicate songs silently skipped instead of failing the whole batch
    let query = AddPlaylistItemsQuery::new_from_videos(pid, vids, DuplicateHandlingMode::Unhandled);
    match yt.query(query).await {
        Ok(results) => Response::ok(serde_json::json!({"added": results.len()})),
        Err(e) => Response::err(format!("add items error: {}", e)),
    }
}

#[derive(Deserialize)]
struct PlaylistRemoveItemsPayload {
    id: String,
    set_video_ids: Vec<String>,
}

async fn cmd_playlist_remove_items(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistRemoveItemsPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' and 'set_video_ids' fields"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let pid = PlaylistID::from_raw(&p.id);
    let set_ids: Vec<SetVideoID<'_>> = p.set_video_ids.iter().map(|v| SetVideoID::from_raw(v.clone())).collect();
    match yt.remove_playlist_items(pid, set_ids).await {
        Ok(_) => Response::ok(serde_json::json!({"removed": true})),
        Err(e) => Response::err(format!("remove items error: {}", e)),
    }
}

// ── Shared: fetch playlist tracks via raw JSON (bypasses ytmapi-rs path bug) ──

async fn fetch_playlist_tracks(yt: &YtMusic<ytmapi_rs::auth::BrowserToken>, playlist_id: &str) -> Result<Vec<serde_json::Value>, String> {
    let browse_id = if playlist_id.starts_with("VL") { playlist_id.to_string() } else { format!("VL{}", playlist_id) };
    let pid = PlaylistID::from_raw(&browse_id);
    let query = GetPlaylistTracksQuery::new(pid);
    let raw_json = yt.raw_json_query::<GetPlaylistTracksQuery<'_>>(&query).await.map_err(|e| format!("browse error: {e}"))?;
    let val: serde_json::Value = serde_json::from_str(&raw_json).map_err(|e| format!("JSON parse error: {e}"))?;

    fn flex_text(item: &serde_json::Value, idx: usize) -> String {
        item.pointer(&format!("/flexColumns/{idx}/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text"))
            .and_then(|t| t.as_str())
            .unwrap_or("?")
            .to_string()
    }

    let base_path = "/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents/0/musicPlaylistShelfRenderer/contents";
    let items = val.pointer(base_path)
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();

    let tracks: Vec<serde_json::Value> = items.into_iter().filter_map(|item| {
        // Skip continuation item renderers
        if item.get("continuationItemRenderer").is_some() { return None; }
        let renderer = item.get("musicResponsiveListItemRenderer")?;
        let title = flex_text(renderer, 0);
        let artist = flex_text(renderer, 1);
        let album = flex_text(renderer, 2);
        let video_id = renderer.pointer("/playlistItemData/videoId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Some(serde_json::json!({
            "title": title,
            "artist": artist,
            "videoId": video_id,
            "album": album,
        }))
    }).collect();
    Ok(tracks)
}

// ── Playlist Tracks ─────────────────────────────────────────────────────

async fn cmd_playlist_tracks(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistGetPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    match fetch_playlist_tracks(&yt, &p.id).await {
        Ok(tracks) => Response::ok(serde_json::json!({"tracks": tracks, "count": tracks.len()})),
        Err(e) => Response::err(format!("playlist tracks error: {e}")),
    }
}

#[derive(Deserialize)]
struct PlaylistRemoveArtistPayload {
    id: String,
    artist: String,
}

async fn cmd_playlist_remove_artist(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistRemoveArtistPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' and 'artist' fields"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let tracks = match fetch_playlist_tracks(&yt, &p.id).await {
        Ok(t) => t,
        Err(e) => return Response::err(format!("fetch tracks error: {}", e)),
    };
    let artist_lower = p.artist.to_lowercase();
    let to_remove: Vec<&serde_json::Value> = tracks.iter().filter(|t| {
        t.get("artist").and_then(|a| a.as_str()).map_or(false, |a| a.to_lowercase().contains(&artist_lower))
    }).collect();

    if to_remove.is_empty() {
        return Response::ok(serde_json::json!({"removed": 0, "message": "no tracks found for artist"}));
    }

    let total = to_remove.len();
    let mut removed = 0;
    for chunk in to_remove.chunks(100) {
        let ids: Vec<SetVideoID<'_>> = chunk.iter().filter_map(|t| {
            t.get("videoId").and_then(|v| v.as_str()).map(|v| SetVideoID::from_raw(v.to_string()))
        }).collect();
        match yt.remove_playlist_items(PlaylistID::from_raw(&p.id), ids).await {
            Ok(_) => removed += chunk.len(),
            Err(e) => eprintln!("  remove batch error: {}", e),
        }
    }
    Response::ok(serde_json::json!({"removed": removed, "total": total}))
}

// ── Playlist Clean (interactive) ──────────────────────────────────────

async fn cmd_playlist_clean(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistGetPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };

    let tracks = match fetch_playlist_tracks(&yt, &p.id).await {
        Ok(t) => t,
        Err(e) => return Response::err(format!("fetch tracks error: {}", e)),
    };
    eprintln!("Fetched {} tracks", tracks.len());

    // Group by artist name
    let mut artist_tracks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in &tracks {
        let name = t.get("artist").and_then(|a| a.as_str()).unwrap_or("?").to_string();
        let vid = t.get("videoId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        artist_tracks.entry(name).or_default().push(vid);
    }

    // Sort by count desc
    let mut sorted: Vec<(String, Vec<String>)> = artist_tracks.into_iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let mut to_remove: Vec<String> = Vec::new();
    let mut stdin_buf = String::new();
    for (i, (artist, tracks)) in sorted.iter().enumerate() {
        println!("[{}/{}] {} ({} tracks)", i + 1, sorted.len(), artist, tracks.len());
        print!("  Remove? [y/n/d=done/q=quit]: ");
        io::stdout().flush().ok();
        stdin_buf.clear();
        match io::stdin().read_line(&mut stdin_buf) {
            Ok(_) => {
                let trimmed = stdin_buf.trim().to_lowercase();
                match trimmed.as_str() {
                    "y" | "yes" => {
                        to_remove.push(artist.clone());
                        println!("  → marked for removal");
                    }
                    "d" | "done" => {
                        println!("  → stopping review");
                        break;
                    }
                    "q" | "quit" => {
                        println!("  → quitting");
                        return Response::ok(serde_json::json!({"removed": 0, "marked": to_remove.len(), "message": "cancelled"}));
                    }
                    _ => println!("  → kept"),
                }
            }
            Err(e) => {
                eprintln!("stdin error: {e}");
                break;
            }
        }
    }

    if to_remove.is_empty() {
        return Response::ok(serde_json::json!({"removed": 0, "message": "no artists marked"}));
    }

    // Remove all marked artists in batch
    eprintln!("\nRemoving {} artist(s)...", to_remove.len());
    let mut total_removed = 0;
    for artist in &to_remove {
        let artist_lower = artist.to_lowercase();
        let artist_vids: Vec<SetVideoID<'static>> = tracks.iter().filter_map(|t| {
            let name = t.get("artist").and_then(|a| a.as_str()).unwrap_or("");
            if name.to_lowercase().contains(&artist_lower) {
                t.get("videoId").and_then(|v| v.as_str()).map(|v| SetVideoID::from_raw(v.to_string()))
            } else { None }
        }).collect();

        if artist_vids.is_empty() { continue; }

        for chunk in artist_vids.chunks(100) {
            let ids: Vec<SetVideoID<'_>> = chunk.iter().map(|s| SetVideoID::from_raw(s.get_raw().to_string())).collect();
            match yt.remove_playlist_items(PlaylistID::from_raw(&p.id), ids).await {
                Ok(_) => total_removed += chunk.len(),
                Err(e) => eprintln!("  batch error for {artist}: {e}"),
            }
        }
        eprintln!("  Removed {} tracks by {}", artist_vids.len(), artist);
    }

    Response::ok(serde_json::json!({"removed": total_removed, "artists_cleaned": to_remove.len()}))
}

// ── Debug ───────────────────────────────────────────────────────────────

async fn cmd_raw_browse(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistGetPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let browse_id = if p.id.starts_with("VL") { p.id.clone() } else { format!("VL{}", p.id) };
    let pid = PlaylistID::from_raw(&browse_id);
    let query = GetPlaylistTracksQuery::new(pid);
    match yt.raw_json_query::<GetPlaylistTracksQuery<'_>>(&query).await {
        Ok(raw) => Response::ok(serde_json::json!({"raw": raw})),
        Err(e) => Response::err(format!("browse error: {e}")),
    }
}

async fn cmd_debug_browse(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: PlaylistGetPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'id' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let browse_id = if p.id.starts_with("VL") { p.id.clone() } else { format!("VL{}", p.id) };
    let pid = ytmapi_rs::common::PlaylistID::from_raw(&browse_id);
    let query = ytmapi_rs::query::GetPlaylistTracksQuery::new(pid);
    let raw = yt.raw_json_query::<ytmapi_rs::query::GetPlaylistTracksQuery<'_>>(&query).await;
    match raw {
        Ok(raw) => {
            // Parse and show top-level keys + relevant sub-paths
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(v) => {
                    let mut info = serde_json::json!({
                        "top_keys": v.as_object().map(|o| o.keys().collect::<Vec<_>>()),
                    });
                    // Try two-column path
                    if let Some(tc) = v.pointer("/contents/twoColumnBrowseResultsRenderer") {
                        info["twoColumn"] = serde_json::json!({
                            "keys": tc.as_object().map(|o| o.keys().collect::<Vec<_>>()),
                        });
                        // Check secondaryContents
                        if let Some(sc) = tc.pointer("/secondaryContents/sectionListRenderer/contents") {
                            info["has_secondaryContents"] = serde_json::json!(true);
                            if let Some(arr) = sc.as_array() {
                                info["secondaryContents_len"] = serde_json::json!(arr.len());
                                if let Some(first) = arr.first() {
                                    info["secondary_first_keys"] = serde_json::json!(
                                        first.as_object().map(|o| o.keys().collect::<Vec<_>>())
                                    );
                                }
                            }
                        }
                        // Check tabs
                        if let Some(tabs) = tc.pointer("/tabs") {
                            if let Some(arr) = tabs.as_array() {
                                info["tabs_len"] = serde_json::json!(arr.len());
                                if let Some(first_tab) = arr.first() {
                                    if let Some(content) = first_tab.pointer("/tabRenderer/content") {
                                        info["tab_content_keys"] = serde_json::json!(
                                            content.as_object().map(|o| o.keys().collect::<Vec<_>>())
                                        );
                                        if let Some(sl) = content.pointer("/sectionListRenderer/contents/0") {
                                            info["section_first_keys"] = serde_json::json!(
                                                sl.as_object().map(|o| o.keys().collect::<Vec<_>>())
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Try single-column path
                    if let Some(sc) = v.pointer("/contents/singleColumnBrowseResultsRenderer") {
                        info["singleColumn"] = serde_json::json!({
                            "keys": sc.as_object().map(|o| o.keys().collect::<Vec<_>>()),
                        });
                        if let Some(tabs) = sc.pointer("/tabs") {
                            if let Some(arr) = tabs.as_array() {
                                info["single_tabs_len"] = serde_json::json!(arr.len());
                                if let Some(first_tab) = arr.first() {
                                    if let Some(content) = first_tab.pointer("/tabRenderer/content") {
                                        info["single_tab_content_keys"] = serde_json::json!(
                                            content.as_object().map(|o| o.keys().collect::<Vec<_>>())
                                        );
                                        if let Some(sl) = content.pointer("/sectionListRenderer/contents/0") {
                                            info["single_section_first_keys"] = serde_json::json!(
                                                sl.as_object().map(|o| o.keys().collect::<Vec<_>>())
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Response::ok(info)
                }
                Err(e) => Response::err(format!("parse error: {e}")),
            }
        }
        Err(e) => Response::err(format!("browse error: {e}")),
    }
}

// ── Search ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearchPayload {
    query: String,
    #[serde(rename = "type")]
    search_type: Option<String>,
}

async fn cmd_search(cookie: Option<&str>, payload: Option<serde_json::Value>) -> Response {
    let p: SearchPayload = match payload.and_then(|v| serde_json::from_value(v).ok()) {
        Some(p) => p,
        None => return Response::err("payload requires 'query' field"),
    };
    let yt = match build_client(cookie).await { Ok(c) => c, Err(e) => return Response::err(e) };
    let results = match p.search_type.as_deref() {
        Some("artists") => {
            match yt.search_artists(&p.query).await {
                Ok(items) => Response::ok(serde_json::to_value(&items).unwrap_or_default()),
                Err(e) => Response::err(format!("search error: {}", e)),
            }
        }
        Some("albums") => {
            match yt.search_albums(&p.query).await {
                Ok(items) => Response::ok(serde_json::to_value(&items).unwrap_or_default()),
                Err(e) => Response::err(format!("search error: {}", e)),
            }
        }
        Some("playlists") => {
            match yt.search_playlists(&p.query).await {
                Ok(items) => Response::ok(serde_json::to_value(&items).unwrap_or_default()),
                Err(e) => Response::err(format!("search error: {}", e)),
            }
        }
        _ => {
            match yt.search_songs(&p.query).await {
                Ok(items) => Response::ok(serde_json::to_value(&items).unwrap_or_default()),
                Err(e) => Response::err(format!("search error: {}", e)),
            }
        }
    };
    return results;
}

// ── Helpers ─────────────────────────────────────────────────────────────

async fn build_client(cookie: Option<&str>) -> Result<YtMusic<ytmapi_rs::auth::BrowserToken>, String> {
    let path = cookie.ok_or_else(|| "--cookie <file> or YTMAPI_COOKIE env required".to_string())?;
    YtMusic::from_cookie_file(&path).await.map_err(|e| format!("auth error: {}", e))
}
