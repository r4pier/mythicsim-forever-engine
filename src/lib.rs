//! A bounded Forever simulation engine organized by shared systems and class/spec domains.
//!
//! Current simulation support is prepared, single-target Frostbolt only. The prepared v2
//! contract describes complete reset Go simulations; [`check_prepared`] reports which
//! mechanics an input still needs. Character preparation, complete Mage mechanics and
//! production reports remain separate migration work. The root re-exports preserve the
//! original public API and prepared v1 JSON contract.

pub mod contracts;
pub mod report;

mod classes;
mod core;
#[allow(dead_code)]
mod data;
mod engine;
mod mechanics;
pub mod overrides;
#[allow(dead_code)]
mod prepare;
mod rotation;

pub use classes::mage::spells::frostbolt::hit_chance;
pub use contracts::{Caster, Request, Spell, Target};
pub use engine::prepared::{
    check as check_prepared, coverage as prepared_coverage, refusals as prepared_refusals,
    simulate as simulate_prepared, simulate_gated as simulate_prepared_gated,
    validate as validate_prepared, EngineIdentity, Gated, PreparedError, PreparedReport, Refusal,
    CLIENT_BUILD, REFUSAL_CODES,
};
pub use engine::simulate;
pub use prepare::{
    prepare, prepare_json, prepare_json_with, PrepareError, Refusal as PrepareRefusal, Rules,
};

/// Prepared v2 effect kinds this engine executes. The release manifest must agree.
pub fn implemented_prepared_effects() -> &'static [&'static str] {
    engine::coverage::implemented_effects()
}
pub use report::{Counts, Report, TraceEvent, Work};

/// The pinned Go reference revision, from upstream/sources.json.
pub const SOURCE_REVISION: &str = env!("FOREVER_REFERENCE_REVISION");
