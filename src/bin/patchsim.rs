//! `patchsim`: sim a character with patch-note changes applied, next to the unchanged engine.
//!
//! See `patches/README.md`. Fork-only tooling; the engine's own `forever-engine` binary is
//! unchanged.

#[path = "../web/mod.rs"]
mod web;

use forever_engine::{
    contracts::prepared_v2::PreparedV2, overrides, prepare_json_with, simulate_prepared_gated,
    validate_prepared, Gated, PrepareError, Rules,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command},
};

const USAGE: &str = "patchsim: sim with patch-note changes, against the unchanged engine

USAGE
  patchsim run (--preset NAME | --request FILE) [options]
      --overrides FILE   changes to apply (default: patches/overrides.json)
      --talents STRING   replace the character's talent string
      --iterations N     fights per sim (default 10000)
      --seed N           random seed (default 42); both sims use the same one
      --duration SECONDS fight length (default: the preset's)
      --top N            actions to list (default 15)
      --json FILE        also write both summaries as JSON (FILE `-`: JSON on stdout only)
  patchsim spell QUERY [--full] [--json]
      client rows for a spell name (substring) or ID, to write row and curve changes
  patchsim presets [--json]
      the preset characters under patches/presets
  patchsim serve [--port 8080] [--no-open]
      the web page on http://127.0.0.1:8080/: build a character, edit overrides, compare
  patchsim import --addon FILE --preset NAME [--outfile FILE]
      build a request from an addon or WoWSims Exporter export, on top of a preset
";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("spell") => spell(&args[1..]),
        Some("presets") => presets().map(|list| {
            if args.iter().any(|arg| arg == "--json") {
                let list: Vec<Value> = list
                    .iter()
                    .map(|(name, path)| json!({"name": name, "path": path}))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&list).unwrap());
                return;
            }
            for (name, path) in list {
                println!("{name:32} {}", path.display());
            }
        }),
        Some("import") => import(&args[1..]),
        Some("serve") => web::serve(&args[1..]),
        Some("__one") => one(&args[1..]),
        _ => {
            print!("{USAGE}");
            Ok(())
        }
    };
    if let Err(message) = result {
        eprintln!("patchsim: {message}");
        process::exit(1);
    }
}

/// `--flag value` pairs and bare flags.
struct Args {
    values: BTreeMap<String, String>,
    flags: Vec<String>,
    positional: Vec<String>,
}

fn parse_args(args: &[String], with_value: &[&str], bare: &[&str]) -> Result<Args, String> {
    let mut parsed = Args {
        values: BTreeMap::new(),
        flags: Vec::new(),
        positional: Vec::new(),
    };
    let mut at = 0;
    while at < args.len() {
        let arg = &args[at];
        if with_value.contains(&arg.as_str()) {
            let value = args
                .get(at + 1)
                .ok_or_else(|| format!("{arg} needs a value"))?;
            parsed.values.insert(arg.clone(), value.clone());
            at += 2;
        } else if bare.contains(&arg.as_str()) {
            parsed.flags.push(arg.clone());
            at += 1;
        } else if arg.starts_with("--") {
            return Err(format!("unknown option {arg}\n\n{USAGE}"));
        } else {
            parsed.positional.push(arg.clone());
            at += 1;
        }
    }
    Ok(parsed)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn presets_dir() -> PathBuf {
    repo_root().join("patches").join("presets")
}

fn presets() -> Result<Vec<(String, PathBuf)>, String> {
    let dir = presets_dir();
    let mut list: Vec<(String, PathBuf)> = fs::read_dir(&dir)
        .map_err(|err| format!("{}: {err}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|path| {
            let name = path
                .file_name()?
                .to_str()?
                .strip_suffix(".request.json")?
                .to_string();
            Some((name, path))
        })
        .collect();
    list.sort();
    Ok(list)
}

fn preset_path(name: &str) -> Result<PathBuf, String> {
    let list = presets()?;
    if let Some((_, path)) = list.iter().find(|(preset, _)| preset == name) {
        return Ok(path.clone());
    }
    let close: Vec<&str> = list
        .iter()
        .map(|(preset, _)| preset.as_str())
        .filter(|preset| preset.contains(name))
        .collect();
    match close.as_slice() {
        [only] => preset_path(only),
        [] => Err(format!("no preset {name:?}; `patchsim presets` lists them")),
        many => Err(format!(
            "{name:?} matches several presets: {}",
            many.join(", ")
        )),
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
}

fn player_mut(request: &mut Value) -> Result<&mut Value, String> {
    request
        .pointer_mut("/raid/parties/0/players/0")
        .ok_or_else(|| "the request has no player at raid.parties[0].players[0]".to_string())
}

fn run(args: &[String]) -> Result<(), String> {
    let args = parse_args(
        args,
        &[
            "--preset",
            "--request",
            "--overrides",
            "--talents",
            "--iterations",
            "--seed",
            "--duration",
            "--top",
            "--json",
        ],
        &[],
    )?;
    let (label, path) = match (args.values.get("--preset"), args.values.get("--request")) {
        (Some(name), None) => {
            let path = preset_path(name)?;
            let label = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .trim_end_matches(".request.json")
                .to_string();
            (label, path)
        }
        (None, Some(file)) => (file.clone(), PathBuf::from(file)),
        _ => return Err("run takes one of --preset or --request".into()),
    };
    let mut request = read_json(&path)?;
    if let Some(talents) = args.values.get("--talents") {
        player_mut(&mut request)?["talentsString"] = json!(talents);
    }
    let iterations: u64 = args
        .values
        .get("--iterations")
        .map_or(Ok(10_000), |n| n.parse())
        .map_err(|_| "--iterations is a whole number")?;
    let seed = args.values.get("--seed").cloned().unwrap_or("42".into());
    request["simOptions"]["iterations"] = json!(iterations);
    request["simOptions"]["randomSeed"] = json!(seed);
    request["simOptions"]["debugFirstIteration"] = json!(false);
    if let Some(duration) = args.values.get("--duration") {
        let seconds: f64 = duration.parse().map_err(|_| "--duration is in seconds")?;
        request["encounter"]["duration"] = json!(seconds);
    }
    let overrides_path = args
        .values
        .get("--overrides")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("patches").join("overrides.json"));
    if !overrides_path.exists() {
        return Err(format!("no overrides file at {}", overrides_path.display()));
    }
    // Check the file before running anything, so a typo fails fast and plainly.
    overrides::install(&fs::read_to_string(&overrides_path).map_err(|err| err.to_string())?)?;

    let temp = env::temp_dir().join(format!("patchsim-{}.request.json", process::id()));
    fs::write(&temp, serde_json::to_string(&request).unwrap()).map_err(|err| err.to_string())?;
    let exe = env::current_exe().map_err(|err| err.to_string())?;
    let spawn = |with_overrides: bool| {
        let mut command = Command::new(&exe);
        command.arg("__one").arg("--request").arg(&temp);
        if with_overrides {
            command.arg("--overrides").arg(&overrides_path);
        }
        command.output()
    };
    let (baseline, patched) = std::thread::scope(|scope| {
        let baseline = scope.spawn(|| spawn(false));
        let patched = scope.spawn(|| spawn(true));
        (baseline.join().unwrap(), patched.join().unwrap())
    });
    let _ = fs::remove_file(&temp);
    let read = |output: std::io::Result<process::Output>, which: &str| -> Result<Value, String> {
        let output = output.map_err(|err| err.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "{which} sim failed:\n{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        serde_json::from_slice(&output.stdout).map_err(|err| format!("{which}: {err}"))
    };
    let baseline = read(baseline, "baseline")?;
    let patched = read(patched, "patched")?;
    let top: usize = args
        .values
        .get("--top")
        .map_or(Ok(15), |n| n.parse())
        .map_err(|_| "--top is a whole number")?;
    let both = json!({
        "preset": label,
        "overrides": {"path": overrides_path, "label": overrides::installed().and_then(|o| o.label.clone())},
        "baseline": baseline,
        "patched": patched,
    });
    if args.values.get("--json").is_some_and(|out| out == "-") {
        println!("{}", serde_json::to_string_pretty(&both).unwrap());
        return Ok(());
    }
    print_comparison(&label, &overrides_path, &baseline, &patched, top);
    if let Some(out) = args.values.get("--json") {
        fs::write(out, serde_json::to_string_pretty(&both).unwrap())
            .map_err(|err| err.to_string())?;
        println!("\nWrote {out}");
    }
    Ok(())
}

/// One sim in its own process, so the unpatched data never mixes with the patched rows.
fn one(args: &[String]) -> Result<(), String> {
    let args = parse_args(args, &["--request", "--overrides"], &[])?;
    let request = fs::read(args.values.get("--request").ok_or("--request")?)
        .map_err(|err| err.to_string())?;
    if let Some(path) = args.values.get("--overrides") {
        overrides::install(&fs::read_to_string(path).map_err(|err| err.to_string())?)?;
    }
    let prepared =
        std::panic::catch_unwind(|| prepare_json_with(&request, "patchsim", Rules::Forever))
            .map_err(|panic| {
                panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "preparation panicked".into())
            })?;
    let mut prepared = prepared.map_err(|err| match err {
        PrepareError::Refused(refusal) => format!(
            "the engine refuses this character ({}): {}",
            refusal.code, refusal.reason
        ),
        PrepareError::Invalid(reason) => format!("invalid request: {reason}"),
        PrepareError::Fault(reason) => format!("preparation failed: {reason}"),
    })?;
    let applied = overrides::apply_prepared(&mut prepared);
    let prepared: PreparedV2 = serde_json::from_value(prepared)
        .map_err(|err| format!("the overrides broke the prepared input: {err}"))?;
    validate_prepared(&prepared).map_err(|err| format!("prepared input rejected: {err}"))?;
    let report = match simulate_prepared_gated(&prepared).map_err(|err| err.to_string())? {
        Gated::Simulated(report) => report,
        Gated::Refused(refusals) => {
            let reasons: Vec<String> = refusals.iter().map(|r| r.reason.clone()).collect();
            return Err(format!(
                "the engine cannot sim this exactly:\n  {}",
                reasons.join("\n  ")
            ));
        }
    };
    let report = serde_json::to_value(&report).map_err(|err| err.to_string())?;
    let summary = summarize(&report["result"], applied);
    println!("{}", serde_json::to_string(&summary).unwrap());
    Ok(())
}

fn action_key(id: &Value) -> (String, Option<i64>) {
    let tag = id["tag"].as_i64().filter(|tag| *tag != 0);
    let tagged = |base: String| match tag {
        Some(tag) => format!("{base} #{tag}"),
        None => base,
    };
    if let Some(spell) = id["spellId"].as_i64() {
        let name = overrides::spell_label(spell as i32)
            .map(|(name, rank)| {
                if rank.is_empty() {
                    format!("{name} ({spell})")
                } else {
                    format!("{name} {rank} ({spell})")
                }
            })
            .unwrap_or_else(|| format!("spell {spell}"));
        return (tagged(name), Some(spell));
    }
    if let Some(item) = id["itemId"].as_i64() {
        return (tagged(format!("item {item}")), None);
    }
    if let Some(other) = id["otherId"].as_str() {
        return (
            tagged(other.trim_start_matches("OtherAction").to_string()),
            None,
        );
    }
    (tagged("unknown".into()), None)
}

fn sum(targets: &Value, field: &str) -> f64 {
    targets
        .as_array()
        .map(|list| list.iter().filter_map(|t| t[field].as_f64()).sum())
        .unwrap_or(0.0)
}

fn summarize(result: &Value, applied: Vec<overrides::Applied>) -> Value {
    let player = &result["raidMetrics"]["parties"][0]["players"][0];
    let iterations = result["iterationsDone"].as_f64().unwrap_or(1.0).max(1.0);
    let seconds = result["avgIterationDuration"]
        .as_f64()
        .unwrap_or(1.0)
        .max(1e-9);
    let per_second = |total: f64| total / iterations / seconds;
    let mut actions = Vec::new();
    let mut collect = |unit: &Value, owner: Option<&str>| {
        for action in unit["actions"].as_array().into_iter().flatten() {
            let targets = &action["targets"];
            let damage = sum(targets, "damage");
            let healing = sum(targets, "healing") + sum(targets, "shielding");
            let casts = sum(targets, "casts");
            if damage == 0.0 && healing == 0.0 && casts == 0.0 {
                continue;
            }
            let (name, spell) = action_key(&action["id"]);
            let name = match owner {
                Some(owner) => format!("{owner}: {name}"),
                None => name,
            };
            actions.push(json!({
                "name": name, "spell": spell,
                "dps": per_second(damage), "hps": per_second(healing),
                "casts": casts / iterations,
            }));
        }
    };
    collect(player, None);
    for pet in player["pets"].as_array().into_iter().flatten() {
        let name = pet["name"].as_str().unwrap_or("pet").to_string();
        collect(pet, Some(&name));
    }
    json!({
        "dps": player["dps"]["avg"].as_f64().unwrap_or(0.0),
        "dps_stdev": player["dps"]["stdev"].as_f64().unwrap_or(0.0),
        "hps": player["hps"]["avg"].as_f64().unwrap_or(0.0),
        "tps": player["threat"]["avg"].as_f64().unwrap_or(0.0),
        "dtps": player["dtps"]["avg"].as_f64().unwrap_or(0.0),
        "oom_seconds": player["secondsOomAvg"].as_f64().unwrap_or(0.0),
        "iterations": iterations,
        "fight_seconds": seconds,
        "actions": actions,
        "applied": applied,
    })
}

fn pct(before: f64, after: f64) -> String {
    if before.abs() < 1e-9 {
        if after.abs() < 1e-9 {
            "".into()
        } else {
            "new".into()
        }
    } else {
        format!("{:+.2}%", (after / before - 1.0) * 100.0)
    }
}

fn print_comparison(
    label: &str,
    overrides_path: &Path,
    baseline: &Value,
    patched: &Value,
    top: usize,
) {
    let installed = overrides::installed();
    println!(
        "{label}  |  {} fights of {:.0} s  |  overrides: {}{}",
        baseline["iterations"].as_f64().unwrap_or(0.0),
        baseline["fight_seconds"].as_f64().unwrap_or(0.0),
        overrides_path.display(),
        installed
            .and_then(|o| o.label.clone())
            .map(|label| format!(" ({label})"))
            .unwrap_or_default(),
    );
    println!();
    println!(
        "{:<14}{:>12}{:>12}{:>10}",
        "", "baseline", "patched", "change"
    );
    for (name, key) in [
        ("DPS", "dps"),
        ("HPS", "hps"),
        ("TPS", "tps"),
        ("DTPS", "dtps"),
        ("OOM s", "oom_seconds"),
    ] {
        let before = baseline[key].as_f64().unwrap_or(0.0);
        let after = patched[key].as_f64().unwrap_or(0.0);
        if before == 0.0 && after == 0.0 {
            continue;
        }
        println!(
            "{name:<14}{before:>12.1}{after:>12.1}{:>10}",
            pct(before, after)
        );
    }
    let stdev = baseline["dps_stdev"].as_f64().unwrap_or(0.0);
    let n = baseline["iterations"].as_f64().unwrap_or(1.0);
    if stdev > 0.0 {
        println!(
            "(DPS standard error about {:.2}; same seed in both sims)",
            stdev / n.sqrt()
        );
    }

    // Actions, matched by name, ordered by baseline damage plus healing.
    let index = |summary: &Value| -> BTreeMap<String, Value> {
        summary["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                (
                    a["name"].as_str().unwrap_or_default().to_string(),
                    a.clone(),
                )
            })
            .collect()
    };
    let before = index(baseline);
    let after = index(patched);
    let changed_ids: Vec<i64> = installed
        .map(|o| {
            o.changes
                .iter()
                .flat_map(|c| c.ids.iter().map(|id| *id as i64))
                .collect()
        })
        .unwrap_or_default();
    let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
    names.sort();
    names.dedup();
    let output = |map: &BTreeMap<String, Value>, name: &str| {
        map.get(name)
            .map(|a| a["dps"].as_f64().unwrap_or(0.0) + a["hps"].as_f64().unwrap_or(0.0))
            .unwrap_or(0.0)
    };
    names.sort_by(|a, b| {
        output(&before, b)
            .max(output(&after, b))
            .total_cmp(&output(&before, a).max(output(&after, a)))
    });
    println!();
    println!(
        "{:<46}{:>10}{:>10}{:>10}{:>9}",
        "action (damage+healing per s)", "baseline", "patched", "change", "casts"
    );
    for name in names.iter().take(top) {
        let b = output(&before, name);
        let a = output(&after, name);
        if b == 0.0 && a == 0.0 {
            continue;
        }
        let spell = after
            .get(*name)
            .or(before.get(*name))
            .and_then(|x| x["spell"].as_i64());
        let marker = if spell.is_some_and(|id| changed_ids.contains(&id)) {
            " *"
        } else {
            ""
        };
        let casts = after
            .get(*name)
            .and_then(|x| x["casts"].as_f64())
            .unwrap_or(0.0);
        let mut shown = format!("{name}{marker}");
        if shown.len() > 45 {
            shown.truncate(45);
        }
        println!("{shown:<46}{b:>10.1}{a:>10.1}{:>10}{casts:>9.1}", pct(b, a));
    }
    println!("(* a spell your overrides name)");

    println!();
    println!("Changes");
    for applied in patched["applied"].as_array().into_iter().flatten() {
        let change = applied["change"].as_str().unwrap_or_default();
        let fields = applied["fields"].as_array().map_or(0, Vec::len);
        let rows = applied["rows"].as_array().map_or(0, Vec::len);
        let status = if rows > 0 {
            format!("patches {rows} client row(s) during preparation")
        } else if fields > 0 {
            format!("changed {fields} prepared field(s)")
        } else {
            "MATCHED NOTHING in the prepared character (not cast by this build, or modelled \
             elsewhere; try a row change)"
                .to_string()
        };
        println!("  {change}: {status}");
    }
    // A named spell that the character casts but whose output did not move.
    if let Some(o) = installed {
        for change in &o.changes {
            let moved = names.iter().any(|name| {
                let spell = after
                    .get(*name)
                    .or(before.get(*name))
                    .and_then(|x| x["spell"].as_i64());
                spell.is_some_and(|id| change.ids.contains(&(id as i32)))
                    && (output(&before, name) - output(&after, name)).abs() > 1e-6
            });
            let present = names.iter().any(|name| {
                after
                    .get(*name)
                    .and_then(|x| x["spell"].as_i64())
                    .is_some_and(|id| change.ids.contains(&(id as i32)))
            });
            let is_output = matches!(&change.kind, overrides::Kind::Prepared(paths, _) if paths.iter().any(|p| p.contains("damage_multiplier") || p.contains("average") || p.contains("coefficient")));
            if present && !moved && is_output {
                println!(
                    "  warning: {} did not change {}'s output; the engine may compute it elsewhere",
                    change.describe(),
                    change.spell
                );
            }
        }
    }
}

fn spell(args: &[String]) -> Result<(), String> {
    let args = parse_args(args, &[], &["--full", "--json"])?;
    let query = args.positional.join(" ");
    if query.is_empty() {
        return Err("spell takes a name or an ID".into());
    }
    let rows = overrides::describe_spells(&query);
    if rows.is_empty() {
        return Err(format!("no spell matches {query:?}"));
    }
    if args.flags.iter().any(|f| f == "--json") {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap());
        return Ok(());
    }
    if args.flags.iter().any(|f| f == "--full") || rows.len() == 1 {
        for row in &rows {
            println!("{}", serde_json::to_string_pretty(row).unwrap());
        }
        return Ok(());
    }
    println!(
        "{:>8}  {:<36}{:<10}{:>5}  effects (type: base points, SP coef)",
        "id", "name", "rank", "lvl"
    );
    for row in rows.iter().take(200) {
        let effects: Vec<String> = row["Effects"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                format!(
                    "{}:{}{}",
                    e["Type"].as_i64().unwrap_or(0),
                    e["BasePoints"].as_f64().unwrap_or(0.0),
                    e["SPCoef"]
                        .as_f64()
                        .map(|c| format!("/{c:.3}"))
                        .unwrap_or_default()
                )
            })
            .collect();
        println!(
            "{:>8}  {:<36}{:<10}{:>5}  {}",
            row["ID"].to_string(),
            row["Name"].as_str().unwrap_or_default(),
            row["Rank"].as_str().unwrap_or_default(),
            row["SpellLevel"].as_i64().unwrap_or(0),
            effects.join("  ")
        );
    }
    if rows.len() > 200 {
        println!("... {} more; narrow the query", rows.len() - 200);
    }
    println!("\n`patchsim spell ID` prints one row in full.");
    Ok(())
}

/// The proto race name for an export's race.
fn race_name(race: &str) -> Option<&'static str> {
    let key: String = race
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    let key = key.trim_start_matches("race");
    Some(match key {
        "human" => "RaceHuman",
        "dwarf" => "RaceDwarf",
        "nightelf" => "RaceNightElf",
        "gnome" => "RaceGnome",
        "orc" => "RaceOrc",
        "undead" | "scourge" => "RaceUndead",
        "tauren" => "RaceTauren",
        "troll" => "RaceTroll",
        "bloodelf" => "RaceBloodElf",
        "highorderskyborne" | "skybornehighorder" | "highorder" => "RaceHighOrderSkyborne",
        "windshaperskyborne" | "skybornewindshaper" | "windshaper" => "RaceWindshaperSkyborne",
        _ => return None,
    })
}

fn import(args: &[String]) -> Result<(), String> {
    let args = parse_args(args, &["--addon", "--preset", "--outfile"], &[])?;
    let export = read_json(Path::new(
        args.values.get("--addon").ok_or("import needs --addon")?,
    ))?;
    let preset = preset_path(args.values.get("--preset").ok_or("import needs --preset")?)?;
    let mut request = read_json(&preset)?;
    let player = player_mut(&mut request)?;
    let mut notes = Vec::new();

    if let Some(name) = export["name"].as_str() {
        player["name"] = json!(name);
    }
    if let Some(race) = export["race"].as_str() {
        let race = race_name(race).ok_or_else(|| format!("unknown race {race:?}"))?;
        player["race"] = json!(race);
    }
    let talents = export["talents"]
        .as_str()
        .or(export["talentsString"].as_str())
        .or(export["talentString"].as_str());
    match talents {
        Some(talents) => player["talentsString"] = json!(talents),
        None => notes.push("the export has no talent string; kept the preset's".to_string()),
    }
    let items = ["/equipment/items", "/gear/items", "/items"]
        .iter()
        .find_map(|path| export.pointer(path))
        .and_then(Value::as_array);
    match items {
        Some(items) => {
            let items: Vec<Value> = items
                .iter()
                .map(|item| {
                    let mut out = serde_json::Map::new();
                    for (from, to) in [
                        ("id", "id"),
                        ("enchant", "enchant"),
                        ("randomSuffix", "randomSuffix"),
                        ("random_suffix", "randomSuffix"),
                        ("suffix", "randomSuffix"),
                    ] {
                        if let Some(value) =
                            item.get(from).and_then(Value::as_i64).filter(|v| *v != 0)
                        {
                            out.insert(to.to_string(), json!(value));
                        }
                    }
                    Value::Object(out)
                })
                .collect();
            player["equipment"] = json!({"items": items});
        }
        None => notes.push("the export has no item list; kept the preset's gear".to_string()),
    }
    if let Some(professions) = export["professions"].as_array() {
        let names: Vec<&str> = professions
            .iter()
            .filter_map(|p| p["name"].as_str().or(p.as_str()))
            .collect();
        let as_enum = |name: &str| {
            let mut name: String = name.chars().filter(char::is_ascii_alphabetic).collect();
            if let Some(first) = name.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            name
        };
        if let Some(first) = names.first() {
            player["profession1"] = json!(as_enum(first));
        }
        if let Some(second) = names.get(1) {
            player["profession2"] = json!(as_enum(second));
        }
    }
    let output = serde_json::to_string_pretty(&request).unwrap();
    match args.values.get("--outfile") {
        Some(path) => {
            fs::write(path, output + "\n").map_err(|err| err.to_string())?;
            println!(
                "Wrote {path} (buffs, consumables and rotation from {})",
                preset.display()
            );
        }
        None => println!("{output}"),
    }
    for note in notes {
        eprintln!("note: {note}");
    }
    Ok(())
}
