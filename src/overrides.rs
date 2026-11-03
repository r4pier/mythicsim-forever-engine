//! Local patch overrides for `patchsim` (fork-only).
//!
//! An overrides file lists balance changes from patch notes before the client data or the
//! upstream engine carries them. Each change names a spell (by name, which covers every rank, or
//! by ID) and one of:
//!
//! - a shortcut on the prepared spell: `damage`, `healing`, `coefficient`, `cooldown`, `cost`
//!   or `cast_time`;
//! - `row`: a path into the client spell row that preparation reads (`Effects/0/BasePoints`);
//! - `curve`: a talent's per-rank values (`data/spell-extras.json`);
//! - `prepared`: a path inside every prepared object that carries the spell's ID.
//!
//! Row and curve changes act while preparation reads the data, so talents and coefficients flow
//! through everything the engine derives from them. Prepared changes act on the prepared state
//! after preparation and before the fight. Nothing here runs unless `install` is called, so the
//! engine's own commands and tests are untouched.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{json, Map, Value};

/// How a change moves a number.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Multiply by this factor (`+10%` is 1.1, `x0.5` is 0.5).
    Mul(f64),
    /// Add this amount, in the field's own unit (seconds for the time shortcuts).
    Add(f64),
    /// Replace the value.
    Set(Value),
}

impl Op {
    /// Reads `"+10%"`, `"-15%"`, `"x1.2"`, `"+0.5s"`, `"-2"`, `"=30s"`, or a bare JSON value to
    /// set. `seconds` scales an `s` suffixed amount to the field's unit (1e9 for nanoseconds).
    pub fn parse(value: &Value, seconds: Option<f64>) -> Result<Op, String> {
        let text = match value {
            Value::String(text) => text.trim().replace(' ', ""),
            other => return Ok(Op::Set(other.clone())),
        };
        let number = |body: &str| -> Result<f64, String> {
            body.parse::<f64>()
                .map_err(|_| format!("cannot read {text:?} as a change"))
        };
        let scaled = |body: &str| -> Result<f64, String> {
            match body.strip_suffix('s') {
                Some(body) => match seconds {
                    Some(scale) => Ok(number(body)? * scale),
                    None => Err(format!("{text:?}: this field takes no seconds")),
                },
                None => number(body),
            }
        };
        if let Some(body) = text.strip_suffix('%') {
            let pct = number(body)?;
            return Ok(Op::Mul(1.0 + pct / 100.0));
        }
        if let Some(body) = text.strip_prefix('x').or_else(|| text.strip_prefix('*')) {
            return Ok(Op::Mul(number(body)?));
        }
        if let Some(body) = text.strip_prefix('=') {
            return Ok(Op::Set(json!(scaled(body)?)));
        }
        if text.starts_with('+') || text.starts_with('-') {
            return Ok(Op::Add(scaled(&text)?));
        }
        Ok(Op::Set(json!(scaled(&text)?)))
    }

    fn apply(&self, value: &mut Value) -> bool {
        let keep_integer = value.is_i64() || value.is_u64();
        let current = value.as_f64();
        let next = match (self, current) {
            (Op::Set(new), _) => match new.as_f64() {
                // A time or count field stays an integer: "=8s" into nanoseconds.
                Some(set) if keep_integer && set.fract() == 0.0 => json!(set as i64),
                _ => new.clone(),
            },
            (Op::Mul(factor), Some(old)) => number(old * factor, keep_integer),
            (Op::Add(amount), Some(old)) => number(old + amount, keep_integer),
            _ => return false,
        };
        *value = next;
        true
    }

    fn describe(&self) -> String {
        match self {
            Op::Mul(factor) => format!("x{factor}"),
            Op::Add(amount) => format!("{amount:+}"),
            Op::Set(value) => format!("= {value}"),
        }
    }
}

fn number(value: f64, keep_integer: bool) -> Value {
    if keep_integer {
        json!(value.round() as i64)
    } else {
        json!(value)
    }
}

/// One entry of the file, as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChange {
    spell: Value,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    damage: Option<Value>,
    #[serde(default)]
    healing: Option<Value>,
    #[serde(default)]
    coefficient: Option<Value>,
    #[serde(default)]
    cooldown: Option<Value>,
    #[serde(default)]
    cost: Option<Value>,
    #[serde(default)]
    cast_time: Option<Value>,
    #[serde(default)]
    row: Option<String>,
    #[serde(default)]
    curve: Option<Value>,
    #[serde(default)]
    prepared: Option<String>,
    #[serde(default)]
    value: Option<Value>,
    #[serde(default)]
    disabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    notes: Option<Value>,
    changes: Vec<RawChange>,
}

/// Where a change acts.
#[derive(Clone, Debug)]
pub enum Kind {
    /// Paths inside each prepared object that carries one of the spell's IDs. Every path is
    /// tried; the ones the object has are changed.
    Prepared(Vec<&'static str>, Op),
    /// A generic prepared path, as written.
    PreparedPath(String, Op),
    /// A path inside the client spell row.
    Row(String, Op),
    /// A talent curve: every value, one effect's values, or the whole table.
    Curve(Option<usize>, Op),
}

/// A resolved change.
#[derive(Clone, Debug)]
pub struct Change {
    pub index: usize,
    pub spell: String,
    pub ids: Vec<i32>,
    pub kind: Kind,
    pub note: Option<String>,
}

impl Change {
    pub fn describe(&self) -> String {
        let what = match &self.kind {
            Kind::Prepared(paths, op) => {
                let op = match op {
                    Op::Add(ns) if paths[0].ends_with("_ns") => format!("{:+}s", ns / 1e9),
                    Op::Set(ns) if paths[0].ends_with("_ns") => {
                        format!("= {}s", ns.as_f64().unwrap_or(0.0) / 1e9)
                    }
                    op => op.describe(),
                };
                format!("{} {op}", shortcut_name(paths))
            }
            Kind::PreparedPath(path, op) => format!("prepared {path} {}", op.describe()),
            Kind::Row(path, op) => format!("row {path} {}", op.describe()),
            Kind::Curve(Some(effect), op) => format!("curve effect {effect} {}", op.describe()),
            Kind::Curve(None, op) => format!("curve {}", op.describe()),
        };
        format!("#{} {}: {what}", self.index + 1, self.spell)
    }
}

const DAMAGE: &[&str] = &["damage_multiplier"];
const HEALING: &[&str] = &["direct/average", "tick", "absorb"];
const COEFFICIENT: &[&str] = &["bonus_coefficient", "dot/bonus_coefficient"];
const COOLDOWN: &[&str] = &["cd/duration_ns"];
const COST: &[&str] = &["cost/base_cost"];
const CAST_TIME: &[&str] = &["default_cast/cast_time_ns"];

fn shortcut_name(paths: &[&str]) -> &'static str {
    [
        (DAMAGE, "damage"),
        (HEALING, "healing"),
        (COEFFICIENT, "coefficient"),
        (COOLDOWN, "cooldown"),
        (COST, "cost"),
        (CAST_TIME, "cast_time"),
    ]
    .iter()
    .find(|(known, _)| *known == paths)
    .map_or("prepared", |(_, name)| name)
}

/// The installed overrides.
pub struct Overrides {
    pub label: Option<String>,
    pub changes: Vec<Change>,
    rows: HashMap<i32, Vec<(String, Op)>>,
    curves: HashMap<i32, Vec<Vec<f64>>>,
}

static INSTALLED: OnceLock<Overrides> = OnceLock::new();

/// A row's identity, read without the engine's strict row type.
#[derive(Deserialize)]
struct RowName {
    #[serde(rename = "ID")]
    id: i32,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Rank", default)]
    rank: String,
}

fn names() -> &'static Vec<RowName> {
    static NAMES: OnceLock<Vec<RowName>> = OnceLock::new();
    NAMES.get_or_init(|| {
        crate::data::spells::ids()
            .filter_map(|id| crate::data::SPELL_ROWS.row(i64::from(id)))
            .filter_map(|text| serde_json::from_str(text).ok())
            .collect()
    })
}

/// The client name and rank of a spell ID, from the unpatched rows.
pub fn spell_label(id: i32) -> Option<(String, String)> {
    let names = names();
    let at = names.binary_search_by_key(&id, |row| row.id).ok()?;
    Some((names[at].name.clone(), names[at].rank.clone()))
}

/// Every spell ID whose client name is `name`, ignoring case.
pub fn ids_named(name: &str) -> Vec<i32> {
    names()
        .iter()
        .filter(|row| row.name.eq_ignore_ascii_case(name))
        .map(|row| row.id)
        .collect()
}

/// Spells whose name contains `query` (or whose ID it is), with their raw rows and curves, for
/// writing overrides.
pub fn describe_spells(query: &str) -> Vec<Value> {
    let ids: Vec<i32> = match query.trim().parse::<i32>() {
        Ok(id) => vec![id],
        Err(_) => {
            let query = query.to_ascii_lowercase();
            names()
                .iter()
                .filter(|row| row.name.to_ascii_lowercase().contains(&query))
                .map(|row| row.id)
                .collect()
        }
    };
    ids.into_iter()
        .filter_map(|id| {
            let text = crate::data::SPELL_ROWS.row(i64::from(id))?;
            let mut row: Value = serde_json::from_str(text).ok()?;
            if let Some(curve) = crate::data::spells::raw_curve(id) {
                row["TalentCurve"] = json!(curve);
            }
            Some(row)
        })
        .collect()
}

/// Reads and installs an overrides file. Call once, before anything is prepared.
pub fn install(text: &str) -> Result<&'static Overrides, String> {
    let overrides = parse(text)?;
    INSTALLED
        .set(overrides)
        .map_err(|_| "overrides are already installed".to_string())?;
    Ok(INSTALLED.get().expect("just installed"))
}

/// The installed overrides, if any.
pub fn installed() -> Option<&'static Overrides> {
    INSTALLED.get()
}

fn parse(text: &str) -> Result<Overrides, String> {
    let file: RawFile =
        serde_json::from_str(text).map_err(|err| format!("overrides file: {err}"))?;
    let _ = file.notes;
    let mut changes = Vec::new();
    for (index, raw) in file.changes.into_iter().enumerate() {
        if raw.disabled {
            continue;
        }
        let at = |message: String| format!("change #{}: {message}", index + 1);
        let (spell, ids) = match &raw.spell {
            Value::Number(id) => {
                let id = id
                    .as_i64()
                    .ok_or_else(|| at("spell ID must be an integer".into()))?;
                (format!("spell {id}"), vec![id as i32])
            }
            Value::String(name) => {
                let ids = ids_named(name);
                if ids.is_empty() {
                    return Err(at(format!(
                        "no spell is named {name:?} (try `patchsim spell {name}`)"
                    )));
                }
                (name.clone(), ids)
            }
            Value::Array(list) => {
                let ids: Option<Vec<i32>> = list
                    .iter()
                    .map(|id| id.as_i64().map(|id| id as i32))
                    .collect();
                let ids = ids.ok_or_else(|| at("a spell list holds IDs".into()))?;
                (format!("spells {ids:?}"), ids)
            }
            _ => return Err(at("spell is a name, an ID or a list of IDs".into())),
        };
        let mut kinds = Vec::new();
        let shortcuts: [(&Option<Value>, &'static [&'static str], Option<f64>); 6] = [
            (&raw.damage, DAMAGE, None),
            (&raw.healing, HEALING, None),
            (&raw.coefficient, COEFFICIENT, None),
            (&raw.cooldown, COOLDOWN, Some(1e9)),
            (&raw.cost, COST, None),
            (&raw.cast_time, CAST_TIME, Some(1e9)),
        ];
        for (value, paths, seconds) in shortcuts {
            if let Some(value) = value {
                let op = Op::parse(value, seconds).map_err(at)?;
                if paths == DAMAGE || paths == HEALING || paths == COEFFICIENT {
                    if !matches!(op, Op::Mul(_)) {
                        return Err(at(
                            "damage, healing and coefficient take a percent or xN".into()
                        ));
                    }
                }
                kinds.push(Kind::Prepared(paths.to_vec(), op));
            }
        }
        let value = || -> Result<Op, String> {
            let value = raw
                .value
                .as_ref()
                .ok_or_else(|| at("row, curve and prepared need a value".into()))?;
            Op::parse(value, None).map_err(at)
        };
        if let Some(path) = &raw.row {
            kinds.push(Kind::Row(path.clone(), value()?));
        }
        if let Some(path) = &raw.prepared {
            kinds.push(Kind::PreparedPath(path.clone(), value()?));
        }
        if let Some(curve) = &raw.curve {
            let effect = match curve {
                Value::Bool(true) => None,
                Value::Number(n) => {
                    Some(n.as_u64().ok_or_else(|| at("curve effect".into()))? as usize)
                }
                _ => return Err(at("curve is true (every effect) or an effect index".into())),
            };
            kinds.push(Kind::Curve(effect, value()?));
        }
        if kinds.is_empty() {
            return Err(at("names no change".into()));
        }
        for kind in kinds {
            changes.push(Change {
                index,
                spell: spell.clone(),
                ids: ids.clone(),
                kind,
                note: raw.note.clone(),
            });
        }
    }
    let mut rows: HashMap<i32, Vec<(String, Op)>> = HashMap::new();
    let mut curves = HashMap::new();
    for change in &changes {
        match &change.kind {
            Kind::Row(path, op) => {
                for id in &change.ids {
                    rows.entry(*id)
                        .or_default()
                        .push((path.clone(), op.clone()));
                }
            }
            Kind::Curve(effect, op) => {
                // A name covers the talent and its auras; only the talent rows carry curves.
                let with_curve: Vec<i32> = change
                    .ids
                    .iter()
                    .copied()
                    .filter(|id| crate::data::spells::raw_curve(*id).is_some())
                    .collect();
                if with_curve.is_empty() {
                    return Err(format!(
                        "change #{}: {} has no talent curve (`patchsim spell` shows TalentCurve on talents)",
                        change.index + 1,
                        change.spell
                    ));
                }
                for id in &with_curve {
                    let mut curve = curves
                        .get(id)
                        .cloned()
                        .or_else(|| crate::data::spells::raw_curve(*id).map(<[_]>::to_vec))
                        .expect("filtered to curves");
                    apply_curve(&mut curve, *effect, op)
                        .map_err(|err| format!("change #{}: {err}", change.index + 1))?;
                    curves.insert(*id, curve);
                }
            }
            _ => {}
        }
    }
    Ok(Overrides {
        label: file.label,
        changes,
        rows,
        curves,
    })
}

fn apply_curve(curve: &mut Vec<Vec<f64>>, effect: Option<usize>, op: &Op) -> Result<(), String> {
    match (effect, op) {
        (None, Op::Set(value)) => {
            *curve = serde_json::from_value(value.clone())
                .map_err(|_| "a whole curve is a list of lists, one per effect".to_string())?;
        }
        (Some(effect), Op::Set(value)) => {
            let values: Vec<f64> = serde_json::from_value(value.clone())
                .map_err(|_| "an effect's curve is a list of numbers, one per rank".to_string())?;
            let slot = curve
                .get_mut(effect)
                .ok_or_else(|| format!("the curve has no effect {effect}"))?;
            *slot = values;
        }
        (effect, op) => {
            for (at, values) in curve.iter_mut().enumerate() {
                if effect.is_some_and(|effect| effect != at) {
                    continue;
                }
                for value in values.iter_mut() {
                    let mut number = json!(*value);
                    op.apply(&mut number);
                    *value = number.as_f64().unwrap_or(*value);
                }
            }
        }
    }
    Ok(())
}

fn path_get<'a>(value: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let mut at = value;
    for part in path.split('/').filter(|part| !part.is_empty()) {
        at = match at {
            Value::Object(map) => map.get_mut(part)?,
            Value::Array(list) => list.get_mut(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(at)
}

/// Row text for preparation: the client row with any row overrides applied. Called by
/// `crate::data::spells::find`.
pub(crate) fn patched_row(id: i32, text: &'static str) -> std::borrow::Cow<'static, str> {
    let Some(edits) = installed().and_then(|overrides| overrides.rows.get(&id)) else {
        return std::borrow::Cow::Borrowed(text);
    };
    let mut row: Value = serde_json::from_str(text).expect("spell row parses");
    for (path, op) in edits {
        match path_get(&mut row, path) {
            Some(value) => {
                op.apply(value);
            }
            None => {
                // A missing field reads as zero in the row type, so create it for a set or an add.
                if let Some(slot) = create_path(&mut row, path) {
                    *slot = json!(0);
                    op.apply(slot);
                }
            }
        }
    }
    std::borrow::Cow::Owned(row.to_string())
}

fn create_path<'a>(value: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let (last, parents) = parts.split_last()?;
    let parent = path_get(value, &parents.join("/"))?;
    let map: &mut Map<String, Value> = parent.as_object_mut()?;
    Some(map.entry(last.to_string()).or_insert(Value::Null))
}

/// A talent curve with any curve override applied. Called by `crate::data::spells::curve`.
pub(crate) fn patched_curve(id: i32) -> Option<&'static [Vec<f64>]> {
    installed()
        .and_then(|overrides| overrides.curves.get(&id))
        .map(Vec::as_slice)
}

/// What one change did to the prepared state.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Applied {
    pub change: String,
    pub note: Option<String>,
    /// Prepared fields changed, as JSON paths.
    pub fields: Vec<String>,
    /// Client rows or curves the change patches (acted during preparation).
    pub rows: Vec<i32>,
}

/// Applies the prepared changes to a prepared input and reports what every change touched.
pub fn apply_prepared(prepared: &mut Value) -> Vec<Applied> {
    let Some(overrides) = installed() else {
        return Vec::new();
    };
    let mut report = Vec::new();
    for change in &overrides.changes {
        let mut applied = Applied {
            change: change.describe(),
            note: change.note.clone(),
            ..Applied::default()
        };
        let ids: HashSet<i32> = change.ids.iter().copied().collect();
        match &change.kind {
            Kind::Prepared(paths, op) => {
                let paths: Vec<String> = paths.iter().map(|path| path.to_string()).collect();
                walk(prepared, "", &ids, &paths, op, &mut applied.fields);
            }
            Kind::PreparedPath(path, op) => {
                walk(prepared, "", &ids, &[path.clone()], op, &mut applied.fields);
            }
            Kind::Row(..) | Kind::Curve(..) => {
                applied.rows = change.ids.clone();
            }
        }
        report.push(applied);
    }
    report
}

fn object_spell_id(map: &Map<String, Value>) -> Option<i64> {
    map.get("spell_id")
        .and_then(Value::as_i64)
        .or_else(|| map.get("action_id")?.get("spell_id")?.as_i64())
}

fn walk(
    value: &mut Value,
    at: &str,
    ids: &HashSet<i32>,
    paths: &[String],
    op: &Op,
    changed: &mut Vec<String>,
) {
    match value {
        Value::Object(map) => {
            if object_spell_id(map).is_some_and(|id| ids.contains(&(id as i32))) {
                let mut object = Value::Object(std::mem::take(map));
                for path in paths {
                    if let Some(field) = path_get(&mut object, path) {
                        if !field.is_null() && op.apply(field) {
                            changed.push(format!("{at}/{path}"));
                        }
                    }
                }
                if let Value::Object(back) = object {
                    *map = back;
                }
            }
            for (key, child) in map.iter_mut() {
                walk(child, &format!("{at}/{key}"), ids, paths, op, changed);
            }
        }
        Value::Array(list) => {
            for (index, child) in list.iter_mut().enumerate() {
                walk(child, &format!("{at}/{index}"), ids, paths, op, changed);
            }
        }
        _ => {}
    }
}

/// The installed changes as JSON, for reports.
pub fn summary() -> Value {
    let Some(overrides) = installed() else {
        return Value::Null;
    };
    let changes: BTreeMap<String, Value> = overrides
        .changes
        .iter()
        .map(|change| {
            (
                change.describe(),
                json!({"ids": change.ids, "note": change.note}),
            )
        })
        .collect();
    json!({"label": overrides.label, "changes": changes})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_parse() {
        assert_eq!(Op::parse(&json!("+10%"), None), Ok(Op::Mul(1.1)));
        assert_eq!(Op::parse(&json!("-25%"), None), Ok(Op::Mul(0.75)));
        assert_eq!(Op::parse(&json!("x2"), None), Ok(Op::Mul(2.0)));
        assert_eq!(Op::parse(&json!("-2s"), Some(1e9)), Ok(Op::Add(-2e9)));
        assert_eq!(Op::parse(&json!("=8s"), Some(1e9)), Ok(Op::Set(json!(8e9))));
        assert_eq!(Op::parse(&json!(3), None), Ok(Op::Set(json!(3))));
        assert!(Op::parse(&json!("+2s"), None).is_err());
    }

    #[test]
    fn integers_stay_integers() {
        let mut value = json!(300);
        Op::Mul(1.1).apply(&mut value);
        assert_eq!(value, json!(330));
    }

    #[test]
    fn prepared_walk_changes_matching_spells_only() {
        let mut prepared = json!({"player": {"spells": [
            {"action_id": {"spell_id": 1}, "damage_multiplier": 1.0, "cd": null},
            {"action_id": {"spell_id": 2}, "damage_multiplier": 1.0}
        ]}});
        let mut changed = Vec::new();
        let ids: HashSet<i32> = [1].into();
        walk(
            &mut prepared,
            "",
            &ids,
            &["damage_multiplier".into(), "cd/duration_ns".into()],
            &Op::Mul(1.2),
            &mut changed,
        );
        assert_eq!(
            prepared["player"]["spells"][0]["damage_multiplier"],
            json!(1.2)
        );
        assert_eq!(
            prepared["player"]["spells"][1]["damage_multiplier"],
            json!(1.0)
        );
        assert_eq!(
            changed,
            vec!["/player/spells/0/damage_multiplier".to_string()]
        );
    }

    #[test]
    fn curves_scale_and_set() {
        let mut curve = vec![vec![2.0, 4.0, 6.0], vec![1.0]];
        apply_curve(&mut curve, Some(0), &Op::Mul(0.5)).unwrap();
        assert_eq!(curve, vec![vec![1.0, 2.0, 3.0], vec![1.0]]);
        apply_curve(&mut curve, Some(1), &Op::Set(json!([5]))).unwrap();
        assert_eq!(curve[1], vec![5.0]);
    }
}
