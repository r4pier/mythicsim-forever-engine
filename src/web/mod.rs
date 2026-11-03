//! `patchsim serve`: a small local web page for building a sim request, editing patch
//! overrides and comparing the baseline with the patched simulation.
//!
//! It is a plain HTTP/1.1 server on 127.0.0.1 built on the standard library only. The page is
//! compiled into the binary. Everything that simulates goes through this same executable's
//! `patchsim run`, `patchsim presets` and `patchsim spell` commands in child processes,
//! because overrides install process-wide and baseline and patched need separate processes.
//! The catalogs the page needs to fill its pickers (items, enchants, consumables, the request
//! schema) are read from the repository's `data/` directory.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const INDEX_HTML: &str = include_str!("index.html");
/// Talent trees by class, written by tools/web_talents.py from the engine's own layouts.
const TALENTS_JSON: &str = include_str!("talents.json");
/// Icon names for talents, buffs and consumables, written by tools/web_icons.py from Wowhead.
const ICONS_JSON: &str = include_str!("icons.json");
/// Request bodies above this are refused: a full request with a long rotation is far smaller.
const MAX_BODY: usize = 8 * 1024 * 1024;

struct Server {
    root: PathBuf,
    exe: PathBuf,
    catalog: OnceLock<Value>,
    items: OnceLock<Vec<Value>>,
}

/// Runs the server until the process is stopped. `args` are the words after `serve`.
pub fn serve(args: &[String]) -> Result<(), String> {
    let mut port: u16 = 8090;
    let mut open = true;
    let mut root: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--port" => {
                index += 1;
                port = args
                    .get(index)
                    .and_then(|value| value.parse().ok())
                    .ok_or("--port requires a number")?;
            }
            "--root" => {
                index += 1;
                root = Some(PathBuf::from(
                    args.get(index).ok_or("--root requires a path")?,
                ));
            }
            "--no-open" => open = false,
            other => return Err(format!("unsupported serve argument {other}")),
        }
        index += 1;
    }
    let root = root.unwrap_or_else(find_root);
    if !root.join("data").join("items.jsonl").is_file() {
        return Err(format!(
            "{} does not look like the engine repository (no data/items.jsonl); run from the repository or pass --root",
            root.display()
        ));
    }
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|err| format!("cannot listen on 127.0.0.1:{port}: {err}"))?;
    let url = format!("http://127.0.0.1:{port}/");
    println!("patchsim web interface: {url}");
    println!("Press Ctrl+C to stop.");
    if open {
        open_browser(&url);
    }
    let server = Arc::new(Server {
        root,
        exe,
        catalog: OnceLock::new(),
        items: OnceLock::new(),
    });
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = Arc::clone(&server);
        std::thread::spawn(move || {
            if let Err(err) = handle(&server, stream) {
                eprintln!("request failed: {err}");
            }
        });
    }
    Ok(())
}

/// The repository: the working directory when it holds `data/`, else the one this was built in.
fn find_root() -> PathBuf {
    if let Ok(dir) = std::env::current_dir() {
        if dir.join("data").join("items.jsonl").is_file() {
            return dir;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn open_browser(url: &str) {
    let result = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
    if result.is_err() {
        println!("Open {url} in your browser.");
    }
}

struct HttpRequest {
    method: String,
    path: String,
    query: HashMap<String, String>,
    body: Vec<u8>,
}

fn handle(server: &Server, stream: TcpStream) -> Result<(), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|err| err.to_string())?);
    let request = match read_request(&mut reader)? {
        Some(request) => request,
        None => return Ok(()),
    };
    let (status, content_type, body) = match route(server, &request) {
        Ok((content_type, body)) => (200, content_type, body),
        Err((status, message)) => (
            status,
            "application/json",
            json!({ "error": message }).to_string().into_bytes(),
        ),
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    };
    let mut stream = stream;
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&body))
        .map_err(|err| err.to_string())
}

fn read_request(reader: &mut impl BufRead) -> Result<Option<HttpRequest>, String> {
    let mut line = String::new();
    if reader.read_line(&mut line).map_err(|err| err.to_string())? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader
            .read_line(&mut header)
            .map_err(|err| err.to_string())?
            == 0
        {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().map_err(|_| "bad Content-Length")?;
            }
        }
    }
    if length > MAX_BODY {
        return Err("request body too large".into());
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|err| err.to_string())?;
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), parse_query(query)),
        None => (target, HashMap::new()),
    };
    Ok(Some(HttpRequest {
        method,
        path,
        query,
        body,
    }))
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key), decode(value))
        })
        .collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() && text.is_char_boundary(index + 3) => {
                match u8::from_str_radix(&text[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

type Routed = Result<(&'static str, Vec<u8>), (u16, String)>;

fn route(server: &Server, request: &HttpRequest) -> Routed {
    let json_ok = |value: Value| Ok(("application/json", value.to_string().into_bytes()));
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => {
            Ok(("text/html; charset=utf-8", INDEX_HTML.as_bytes().to_vec()))
        }
        ("GET", "/api/catalog") => json_ok(server.catalog()?.clone()),
        ("GET", "/api/presets") => json_ok(server.presets()?),
        ("GET", "/api/preset") => {
            let name = request
                .query
                .get("name")
                .ok_or((400, "name is required".into()))?;
            json_ok(server.preset(name)?)
        }
        ("GET", "/api/items") => {
            let slot = request.query.get("slot").map(String::as_str).unwrap_or("");
            let text = request.query.get("q").map(String::as_str).unwrap_or("");
            json_ok(server.search_items(slot, text)?)
        }
        ("GET", "/api/spells") => {
            let text = request
                .query
                .get("q")
                .ok_or((400, "q is required".into()))?;
            json_ok(server.spells(text)?)
        }
        ("POST", "/api/run") => {
            let body: Value = serde_json::from_slice(&request.body)
                .map_err(|err| (400, format!("body is not JSON: {err}")))?;
            json_ok(server.run(&body)?)
        }
        _ => Err((
            404,
            format!("no route for {} {}", request.method, request.path),
        )),
    }
}

fn internal(err: impl std::fmt::Display) -> (u16, String) {
    (500, err.to_string())
}

fn read_jsonl(path: &Path) -> Result<Vec<Value>, (u16, String)> {
    let text =
        fs::read_to_string(path).map_err(|err| internal(format!("{}: {err}", path.display())))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(internal))
        .collect()
}

/// The proto messages the page renders as editable groups.
const SCHEMA_MESSAGES: &[&str] = &[
    "proto.RaidBuffs",
    "proto.PartyBuffs",
    "proto.IndividualBuffs",
    "proto.Debuffs",
    "proto.ConsumesSpec",
    "proto.Encounter",
    "proto.Target",
    "proto.SimOptions",
];

impl Server {
    /// Runs this executable with `args` and returns its standard output, or its error text.
    fn patchsim(&self, args: &[&str]) -> Result<Vec<u8>, (u16, String)> {
        let output = Command::new(&self.exe)
            .args(args)
            .current_dir(&self.root)
            .output()
            .map_err(internal)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                format!("patchsim {} failed", args.first().unwrap_or(&""))
            } else {
                stderr
            };
            return Err((400, message));
        }
        Ok(output.stdout)
    }

    fn patchsim_json(&self, args: &[&str]) -> Result<Value, (u16, String)> {
        let stdout = self.patchsim(args)?;
        serde_json::from_slice(&stdout).map_err(|err| internal(format!("patchsim output: {err}")))
    }

    fn catalog(&self) -> Result<&Value, (u16, String)> {
        if let Some(catalog) = self.catalog.get() {
            return Ok(catalog);
        }
        let data = self.root.join("data");
        let schema: Value =
            serde_json::from_slice(&fs::read(data.join("proto-schema.json")).map_err(internal)?)
                .map_err(internal)?;
        let mut messages = serde_json::Map::new();
        for name in SCHEMA_MESSAGES {
            if let Some(message) = schema["messages"].get(*name) {
                messages.insert(name.to_string(), message.clone());
            }
        }
        let consumables: Vec<Value> = read_jsonl(&data.join("consumables.jsonl"))?
            .into_iter()
            .map(|row| json!({ "id": row["id"], "name": row["name"], "type": row["type"] }))
            .collect();
        let enchants: Vec<Value> = read_jsonl(&data.join("enchants.jsonl"))?
            .into_iter()
            .map(|row| {
                json!({ "id": row["effect_id"], "name": row["name"], "type": row["type"],
                        "enchant_type": row.get("enchant_type").cloned().unwrap_or(Value::Null) })
            })
            .collect();
        let catalog = json!({
            "messages": messages,
            "enums": {
                "proto.TristateEffect": schema["enums"]["proto.TristateEffect"],
                "proto.Race": schema["enums"]["proto.Race"],
                "proto.MobType": schema["enums"]["proto.MobType"],
            },
            "consumables": consumables,
            "enchants": enchants,
            "races": self.races(),
            "talents": serde_json::from_str::<Value>(TALENTS_JSON).map_err(internal)?,
            "icons": serde_json::from_str::<Value>(ICONS_JSON).map_err(internal)?,
        });
        Ok(self.catalog.get_or_init(|| catalog))
    }

    /// Races each preset name ships a builder starter for, read from the starters' requests.
    fn races(&self) -> Value {
        let dir = self
            .root
            .join("validation")
            .join("2026-10-04-builder-starters");
        let mut races: HashMap<String, Vec<String>> = HashMap::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(spec) = name.split("--").next() else {
                    continue;
                };
                let Ok(bytes) = fs::read(entry.path()) else {
                    continue;
                };
                let Ok(request) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                if let Some(race) = request["raid"]["parties"][0]["players"][0]["race"].as_str() {
                    let list = races.entry(spec.to_string()).or_default();
                    if !list.iter().any(|known| known == race) {
                        list.push(race.to_string());
                    }
                }
            }
        }
        for list in races.values_mut() {
            list.sort();
        }
        json!(races)
    }

    /// `patchsim presets`, each with the class, race and talents its request names.
    fn presets(&self) -> Result<Value, (u16, String)> {
        let mut presets = self.patchsim_json(&["presets", "--json"])?;
        if let Some(list) = presets.as_array_mut() {
            for preset in list.iter_mut() {
                let Some(path) = preset["path"].as_str() else {
                    continue;
                };
                let Ok(bytes) = fs::read(self.root.join(path)) else {
                    continue;
                };
                let Ok(request) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                let player = &request["raid"]["parties"][0]["players"][0];
                preset["class"] = player["class"].clone();
                preset["race"] = player["race"].clone();
                preset["talents"] = player["talentsString"].clone();
            }
        }
        Ok(presets)
    }

    fn preset(&self, name: &str) -> Result<Value, (u16, String)> {
        let presets = self.presets()?;
        let entry = presets
            .as_array()
            .and_then(|list| list.iter().find(|preset| preset["name"] == name))
            .ok_or_else(|| (404, format!("no preset named {name}")))?;
        let path = entry["path"]
            .as_str()
            .ok_or((500, "preset has no path".to_string()))?;
        let path = self.root.join(path);
        let request: Value =
            serde_json::from_slice(&fs::read(&path).map_err(internal)?).map_err(internal)?;
        Ok(json!({ "name": name, "request": request }))
    }

    fn items(&self) -> Result<&Vec<Value>, (u16, String)> {
        if let Some(items) = self.items.get() {
            return Ok(items);
        }
        let rows = read_jsonl(&self.root.join("data").join("items.jsonl"))?;
        let items = rows
            .into_iter()
            .map(|row| {
                let ilvl = row["scaling_options"]["0"]["ilvl"].clone();
                json!({
                    "id": row["id"], "name": row["name"], "type": row["type"],
                    "hand_type": row.get("hand_type").cloned().unwrap_or(Value::Null),
                    "weapon_type": row.get("weapon_type").cloned().unwrap_or(Value::Null),
                    "ranged_weapon_type": row.get("ranged_weapon_type").cloned().unwrap_or(Value::Null),
                    "armor_type": row.get("armor_type").cloned().unwrap_or(Value::Null),
                    "quality": row.get("quality").cloned().unwrap_or(Value::Null),
                    "ilvl": ilvl,
                })
            })
            .collect();
        Ok(self.items.get_or_init(|| items))
    }

    /// Items by id list (`ids=1,2`) or by slot type and name text, at most 60.
    fn search_items(&self, slot: &str, text: &str) -> Result<Value, (u16, String)> {
        let items = self.items()?;
        let ids: Vec<i64> = text
            .strip_prefix("ids:")
            .map(|list| {
                list.split(',')
                    .filter_map(|id| id.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        if !ids.is_empty() {
            let found: Vec<&Value> = items
                .iter()
                .filter(|item| item["id"].as_i64().is_some_and(|id| ids.contains(&id)))
                .collect();
            return Ok(json!(found));
        }
        let needle = text.to_lowercase();
        let by_id: Option<i64> = text.trim().parse().ok();
        let found: Vec<&Value> = items
            .iter()
            .filter(|item| slot.is_empty() || item["type"] == slot)
            .filter(|item| match by_id {
                Some(id) => item["id"].as_i64() == Some(id),
                None => item["name"]
                    .as_str()
                    .is_some_and(|name| name.to_lowercase().contains(&needle)),
            })
            .take(60)
            .collect();
        Ok(json!(found))
    }

    fn spells(&self, text: &str) -> Result<Value, (u16, String)> {
        if text.trim().is_empty() {
            return Ok(json!([]));
        }
        self.patchsim_json(&["spell", text, "--json"])
    }

    /// Runs baseline and patched for `{request, overrides}` through `patchsim run`.
    fn run(&self, body: &Value) -> Result<Value, (u16, String)> {
        let request = body
            .get("request")
            .filter(|request| request.is_object())
            .ok_or((400, "request is required".to_string()))?;
        let overrides = body
            .get("overrides")
            .cloned()
            .unwrap_or_else(|| json!({ "changes": [] }));
        if !overrides.is_object() {
            return Err((400, "overrides must be a JSON object".to_string()));
        }
        let dir = std::env::temp_dir().join("patchsim-web");
        fs::create_dir_all(&dir).map_err(internal)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0);
        let tag = format!("{}-{stamp}", std::process::id());
        let request_path = dir.join(format!("request-{tag}.json"));
        let overrides_path = dir.join(format!("overrides-{tag}.json"));
        fs::write(&request_path, request.to_string()).map_err(internal)?;
        fs::write(&overrides_path, overrides.to_string()).map_err(internal)?;
        // patchsim sets the iterations and seed itself (10000 and 42 unless told), so pass the
        // page's values through.
        let options = &request["simOptions"];
        let iterations = options["iterations"].as_u64().unwrap_or(10_000).to_string();
        let seed = match &options["randomSeed"] {
            Value::String(seed) if !seed.trim().is_empty() => seed.trim().to_string(),
            Value::Number(seed) => seed.to_string(),
            _ => "42".to_string(),
        };
        let result = self.patchsim_json(&[
            "run",
            "--request",
            &request_path.to_string_lossy(),
            "--overrides",
            &overrides_path.to_string_lossy(),
            "--iterations",
            &iterations,
            "--seed",
            &seed,
            "--json",
            "-",
        ]);
        let _ = fs::remove_file(&request_path);
        let _ = fs::remove_file(&overrides_path);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_query_text() {
        assert_eq!(decode("frost+bolt%20r1"), "frost bolt r1");
        assert_eq!(decode("100%"), "100%");
        let query = parse_query("slot=ItemTypeHead&q=lion%27s");
        assert_eq!(query["slot"], "ItemTypeHead");
        assert_eq!(query["q"], "lion's");
    }

    #[test]
    fn reads_a_post_request() {
        let raw = b"POST /api/run?x=1 HTTP/1.1\r\nHost: a\r\nContent-Length: 2\r\n\r\n{}";
        let request = read_request(&mut &raw[..]).unwrap().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/run");
        assert_eq!(request.query["x"], "1");
        assert_eq!(request.body, b"{}");
    }
}
