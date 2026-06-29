use std::io::{self, Read};
use ytmapi_rs::{
    YtMusic,
    common::{PlaylistID, VideoID, SetVideoID, YoutubeID},
    query::{
        CreatePlaylistQuery,
        EditPlaylistQuery,
        playlist::PrivacyStatus,
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
    match yt.add_video_items_to_playlist(pid, vids).await {
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
