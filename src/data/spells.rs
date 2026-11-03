//! Client spell rows, as Go's generated `sim/core/spelldata` store carries them.
//!
//! The fields mirror `spelldata.Spell`, `Effect` and `Power` with Go's types, so float32 columns
//! stay float32 and convert to float64 where Go converts them. A field the importer dropped as
//! zero reads back as zero.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct ClassFlags {
    #[serde(rename = "Family")]
    pub family: i32,
    #[serde(rename = "Mask")]
    pub mask: [u32; 4],
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Effect {
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(rename = "SpellID")]
    pub spell_id: i32,
    #[serde(rename = "Index")]
    pub index: u8,
    #[serde(rename = "Type")]
    pub effect_type: i32,
    #[serde(rename = "Aura")]
    pub aura: i32,
    #[serde(rename = "BasePoints")]
    pub base_points: f64,
    #[serde(rename = "PPL")]
    pub ppl: f64,
    #[serde(rename = "SpellLevel")]
    pub spell_level: i16,
    #[serde(rename = "MaxLevel")]
    pub max_level: i16,
    #[serde(rename = "Variance")]
    pub variance: f64,
    #[serde(rename = "SPCoef")]
    pub sp_coef: f64,
    #[serde(rename = "APCoef")]
    pub ap_coef: f64,
    #[serde(rename = "PvpMult")]
    pub pvp_mult: f32,
    #[serde(rename = "Amplitude")]
    pub amplitude: f32,
    #[serde(rename = "PeriodMs")]
    pub period_ms: i32,
    #[serde(rename = "RadiusMin")]
    pub radius_min: f32,
    #[serde(rename = "RadiusMax")]
    pub radius_max: f32,
    #[serde(rename = "Misc")]
    pub misc: i32,
    #[serde(rename = "Misc2")]
    pub misc2: i32,
    #[serde(rename = "ClassFlags")]
    pub class_flags: ClassFlags,
    #[serde(rename = "TriggerID")]
    pub trigger_id: i32,
    #[serde(rename = "ChainTargets")]
    pub chain_targets: i16,
    #[serde(rename = "ChainAmp")]
    pub chain_amp: f32,
    #[serde(rename = "Mechanic")]
    pub mechanic: i32,
    #[serde(rename = "PointsPerResource")]
    pub points_per_resource: f32,
    #[serde(rename = "Target")]
    pub target: [i32; 2],
    #[serde(rename = "Attributes")]
    pub attributes: i32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Power {
    #[serde(rename = "Type")]
    pub power_type: i32,
    #[serde(rename = "Cost")]
    pub cost: i32,
    #[serde(rename = "CostPerLevel")]
    pub cost_per_level: i32,
    #[serde(rename = "CostPct")]
    pub cost_pct: f32,
    #[serde(rename = "PerSecond")]
    pub per_second: i32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Spell {
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Rank")]
    pub rank: String,
    #[serde(rename = "School")]
    pub school: u8,
    #[serde(rename = "Speed")]
    pub speed: f32,
    #[serde(rename = "Attr")]
    pub attr: [u32; 17],
    #[serde(rename = "SpellLevel")]
    pub spell_level: i16,
    #[serde(rename = "BaseLevel")]
    pub base_level: i16,
    #[serde(rename = "MaxLevel")]
    pub max_level: i16,
    #[serde(rename = "CastTimeMs")]
    pub cast_time_ms: i32,
    #[serde(rename = "DurationMs")]
    pub duration_ms: i32,
    #[serde(rename = "MinRange")]
    pub min_range: f32,
    #[serde(rename = "MaxRange")]
    pub max_range: f32,
    #[serde(rename = "CooldownMs")]
    pub cooldown_ms: i32,
    #[serde(rename = "CategoryCooldownMs")]
    pub category_cooldown_ms: i32,
    #[serde(rename = "GCDMs")]
    pub gcd_ms: i32,
    #[serde(rename = "Category")]
    pub category: i16,
    #[serde(rename = "StartRecoveryCategory")]
    pub start_recovery_category: i16,
    #[serde(rename = "ChargeCategory")]
    pub charge_category: i16,
    #[serde(rename = "DefenseType")]
    pub defense_type: u8,
    #[serde(rename = "DispelType")]
    pub dispel_type: u8,
    #[serde(rename = "Mechanic")]
    pub mechanic: i32,
    #[serde(rename = "PreventionType")]
    pub prevention_type: u8,
    #[serde(rename = "MaxStack")]
    pub max_stack: i16,
    #[serde(rename = "ProcChance")]
    pub proc_chance: u8,
    #[serde(rename = "ProcCharges")]
    pub proc_charges: i16,
    #[serde(rename = "ProcFlags")]
    pub proc_flags: [u32; 2],
    #[serde(rename = "ICDMs")]
    pub icd_ms: i32,
    #[serde(rename = "RPPM")]
    pub rppm: f32,
    #[serde(rename = "FlatThreat")]
    pub flat_threat: f32,
    #[serde(rename = "AreaBonusGroups")]
    pub area_bonus_groups: Vec<i32>,
    #[serde(rename = "AreaMultiplier")]
    pub area_multiplier: f32,
    #[serde(rename = "AreaDurationMultiplier")]
    pub area_duration_multiplier: f32,
    #[serde(rename = "ClassFlags")]
    pub class_flags: ClassFlags,
    #[serde(rename = "InterruptFlags")]
    pub interrupt_flags: u32,
    #[serde(rename = "AuraInterrupt")]
    pub aura_interrupt: [u32; 2],
    #[serde(rename = "ChannelInterrupt")]
    pub channel_interrupt: [u32; 2],
    #[serde(rename = "StanceMask")]
    pub stance_mask: u64,
    #[serde(rename = "StanceExclude")]
    pub stance_exclude: u64,
    #[serde(rename = "CasterAura")]
    pub caster_aura: i32,
    #[serde(rename = "ExcludeCasterAura")]
    pub exclude_caster_aura: i32,
    #[serde(rename = "MaxTargets")]
    pub max_targets: i16,
    #[serde(rename = "TargetCreatureType")]
    pub target_creature_type: i32,
    #[serde(rename = "RequiredAreas")]
    pub required_areas: i32,
    #[serde(rename = "EquipClass")]
    pub equip_class: i8,
    #[serde(rename = "EquipSubclass")]
    pub equip_subclass: i32,
    #[serde(rename = "EquipInvType")]
    pub equip_inv_type: i32,
    #[serde(rename = "Labels")]
    pub labels: Vec<i16>,
    #[serde(rename = "RefIDs")]
    pub ref_ids: Vec<i32>,
    #[serde(rename = "ProcChanceSource")]
    pub proc_chance_source: u8,
    #[serde(rename = "ProcChanceEffect")]
    pub proc_chance_effect: i8,
    #[serde(rename = "ProcHint")]
    pub proc_hint: u8,
    #[serde(rename = "SplitsDamage")]
    pub splits_damage: bool,
    #[serde(rename = "Effects")]
    pub effects: Vec<Effect>,
    #[serde(rename = "Powers")]
    pub powers: Vec<Power>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Extras {
    curves: HashMap<i32, Vec<Vec<f64>>>,
    hand_triggers: HashMap<i32, Vec<i32>>,
}

fn extras() -> &'static Extras {
    static EXTRAS: OnceLock<Extras> = OnceLock::new();
    EXTRAS.get_or_init(|| {
        serde_json::from_str(include_str!("../../data/spell-extras.json"))
            .expect("data/spell-extras.json parses")
    })
}

/// The row for an id, parsed once and kept for the process. `None` where Go's `Find` answers
/// its zero `Nil` row.
pub(crate) fn find(id: i32) -> Option<&'static Spell> {
    static CACHE: OnceLock<Mutex<HashMap<i32, Option<&'static Spell>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().expect("spell cache");
    *cache.entry(id).or_insert_with(|| {
        super::SPELL_ROWS.row(i64::from(id)).map(|text| {
            // patchsim: a row override applies as the row is read (no-op unless installed).
            let text = crate::overrides::patched_row(id, text);
            let row: Spell = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("data/spells.jsonl row {id}: {err}"));
            &*Box::leak(Box::new(row))
        })
    })
}

/// Go's `MustFind`: a row preparation depends on.
pub(crate) fn must_find(id: i32) -> &'static Spell {
    find(id).unwrap_or_else(|| panic!("spell {id} is not in data/spells.jsonl"))
}

/// A talent's curve values by effect position and rank, `[effect][rank - 1]`.
pub(crate) fn curve(id: i32) -> Option<&'static [Vec<f64>]> {
    // patchsim: a curve override replaces the curve (no-op unless installed).
    crate::overrides::patched_curve(id).or_else(|| raw_curve(id))
}

/// A talent's curve as the data carries it, without overrides.
pub(crate) fn raw_curve(id: i32) -> Option<&'static [Vec<f64>]> {
    extras().curves.get(&id).map(Vec::as_slice)
}

/// The spells a server side handler casts off this one.
pub(crate) fn hand_triggers(id: i32) -> &'static [i32] {
    extras()
        .hand_triggers
        .get(&id)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Every row id, ascending.
pub(crate) fn ids() -> impl Iterator<Item = i32> {
    super::SPELL_ROWS.keys().map(|key| key as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_parses() {
        for id in ids() {
            let row = find(id).unwrap();
            assert_eq!(row.id, id);
        }
        let blizzard = must_find(10);
        assert_eq!(blizzard.name, "Blizzard");
        assert_eq!(blizzard.effects[0].sp_coef, 0.029999999329447746);
        assert_eq!(blizzard.powers[0].cost, 320);
        assert!(find(1).is_none());
        assert_eq!(hand_triggers(20230), &[20240]);
    }
}
