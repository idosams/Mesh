//! Shape verification: does the corpus a generator produced match the corpus
//! plan §12.2 asked for?
//!
//! A determinism test proves a generator repeats itself. It says nothing about
//! whether it repeats the *right thing* — a generator that emits one empty file
//! is perfectly deterministic. The shape report is the other half: every
//! generator states, per scale, what it intends to produce, then measures what
//! it actually produced and reports both.
//!
//! One fact type, deliberately. A count, a ratio and a yes/no all become
//! `stated`, `observed` and `tolerance`, so the rule for "does it hold" is a
//! single line that `verify.mjs` can re-implement and disagree with. Two
//! independent checkers of one rule is worth more than one checker of three.

use crate::json::{Json, JsonObject};

/// One measurable property of a generated corpus.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeFact {
    /// What is being measured, in `snake_case`.
    pub name: String,
    /// What the workload definition asks for.
    pub stated: f64,
    /// What the generator produced.
    pub observed: f64,
    /// Allowed absolute deviation. Zero means exact.
    pub tolerance: f64,
    /// The unit, for a reader: `files`, `bytes`, `ratio`, `present`.
    pub unit: &'static str,
}

impl ShapeFact {
    /// An exact count.
    #[must_use]
    pub fn count(name: impl Into<String>, stated: u64, observed: u64) -> Self {
        ShapeFact {
            name: name.into(),
            stated: stated as f64,
            observed: observed as f64,
            tolerance: 0.0,
            unit: "files",
        }
    }

    /// An exact byte total.
    #[must_use]
    pub fn bytes(name: impl Into<String>, stated: u64, observed: u64) -> Self {
        ShapeFact {
            name: name.into(),
            stated: stated as f64,
            observed: observed as f64,
            tolerance: 0.0,
            unit: "bytes",
        }
    }

    /// A ratio with an allowed absolute deviation.
    #[must_use]
    pub fn ratio(name: impl Into<String>, stated: f64, observed: f64, tolerance: f64) -> Self {
        ShapeFact {
            name: name.into(),
            stated,
            observed,
            tolerance,
            unit: "ratio",
        }
    }

    /// A property that is either there or is not.
    #[must_use]
    pub fn present(name: impl Into<String>, observed: bool) -> Self {
        ShapeFact {
            name: name.into(),
            stated: 1.0,
            observed: if observed { 1.0 } else { 0.0 },
            tolerance: 0.0,
            unit: "present",
        }
    }

    /// A tolerance with the same value on both sides, given as a fraction of
    /// `stated` — the form a statistical bound is usually stated in.
    #[must_use]
    pub fn with_relative_tolerance(mut self, fraction: f64) -> Self {
        self.tolerance = (self.stated * fraction).abs();
        self
    }

    /// Whether the observation is within tolerance of the statement.
    ///
    /// The one rule, so `verify.mjs` can implement exactly this and no more:
    /// `|observed - stated| <= tolerance`.
    #[must_use]
    pub fn holds(&self) -> bool {
        (self.observed - self.stated).abs() <= self.tolerance
    }

    /// The fact as JSON.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("name", Json::string(self.name.as_str()))
                .with("stated", Json::Float(self.stated))
                .with("observed", Json::Float(self.observed))
                .with("tolerance", Json::Float(self.tolerance))
                .with("unit", Json::string(self.unit))
                .with("holds", Json::Bool(self.holds())),
        )
    }
}

/// Every fact about one generated corpus.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeReport {
    facts: Vec<ShapeFact>,
}

impl ShapeReport {
    /// An empty report.
    #[must_use]
    pub fn new() -> Self {
        ShapeReport::default()
    }

    /// Returns a report with `fact` appended.
    #[must_use]
    pub fn with(mut self, fact: ShapeFact) -> Self {
        self.facts.push(fact);
        self
    }

    /// The facts, in the order the generator stated them.
    #[must_use]
    pub fn facts(&self) -> &[ShapeFact] {
        &self.facts
    }

    /// The facts that do not hold.
    #[must_use]
    pub fn violations(&self) -> Vec<&ShapeFact> {
        self.facts.iter().filter(|fact| !fact.holds()).collect()
    }

    /// Whether every fact holds.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.violations().is_empty()
    }

    /// One named fact, if the report has it.
    #[must_use]
    pub fn fact(&self, name: &str) -> Option<&ShapeFact> {
        self.facts.iter().find(|fact| fact.name == name)
    }

    /// The report as JSON.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("holds", Json::Bool(self.holds()))
                .with(
                    "facts",
                    Json::array(self.facts.iter().map(ShapeFact::to_json)),
                ),
        )
    }
}

/// Three standard deviations of a binomial proportion, as an absolute
/// tolerance on a measured rate.
///
/// A Bernoulli-driven parameter — W3's overlap probability, for instance — does
/// not land on its stated value; it lands near it, and how near depends on how
/// many draws there were. Stating the tolerance as a fixed percentage would be
/// either a lie at small scales or a rubber stamp at large ones, so it is
/// derived from the sample count instead. A floor keeps the band usable when
/// `draws` is tiny, where three sigma is still wider than the interval itself.
#[must_use]
pub fn binomial_tolerance(probability: f64, draws: u64) -> f64 {
    if draws == 0 {
        return 1.0;
    }
    let sigma = (probability * (1.0 - probability) / draws as f64).sqrt();
    (3.0 * sigma).max(0.005)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exact_count_holds_only_when_exact() {
        assert!(ShapeFact::count("files", 10, 10).holds());
        assert!(!ShapeFact::count("files", 10, 11).holds());
    }

    #[test]
    fn a_ratio_holds_inside_its_tolerance() {
        assert!(ShapeFact::ratio("overlap", 0.2, 0.21, 0.02).holds());
        assert!(!ShapeFact::ratio("overlap", 0.2, 0.24, 0.02).holds());
    }

    #[test]
    fn a_relative_tolerance_scales_with_the_statement() {
        let fact = ShapeFact::ratio("overlap", 0.2, 0.21, 0.0).with_relative_tolerance(0.1);
        assert!((fact.tolerance - 0.02).abs() < 1e-12);
        assert!(fact.holds());
    }

    #[test]
    fn presence_is_a_fact_like_any_other() {
        assert!(ShapeFact::present("has_append", true).holds());
        assert!(!ShapeFact::present("has_append", false).holds());
    }

    #[test]
    fn a_report_fails_when_any_fact_fails() {
        let report = ShapeReport::new()
            .with(ShapeFact::count("files", 3, 3))
            .with(ShapeFact::present("has_append", false));
        assert!(!report.holds());
        assert_eq!(report.violations().len(), 1);
        assert_eq!(report.violations()[0].name, "has_append");
    }

    #[test]
    fn an_empty_report_holds_vacuously() {
        assert!(ShapeReport::new().holds());
    }

    #[test]
    fn facts_are_addressable_by_name() {
        let report = ShapeReport::new().with(ShapeFact::bytes("logical_bytes", 100, 100));
        assert_eq!(
            report.fact("logical_bytes").map(|fact| fact.stated),
            Some(100.0)
        );
        assert!(report.fact("nope").is_none());
    }

    #[test]
    fn the_report_serialises_its_verdict_with_its_facts() {
        let json = ShapeReport::new()
            .with(ShapeFact::count("files", 1, 1))
            .to_json();
        let object = json.as_object().expect("an object");
        assert_eq!(object.get("holds").and_then(Json::as_bool), Some(true));
        assert_eq!(
            object
                .get("facts")
                .and_then(Json::as_array)
                .map(<[Json]>::len),
            Some(1)
        );
    }

    #[test]
    fn the_binomial_band_narrows_as_draws_grow() {
        let few = binomial_tolerance(0.2, 100);
        let many = binomial_tolerance(0.2, 100_000);
        assert!(few > many, "{few} should exceed {many}");
        assert!(many >= 0.005, "the floor keeps the band usable");
    }

    #[test]
    fn no_draws_means_no_claim() {
        assert_eq!(binomial_tolerance(0.2, 0), 1.0);
    }
}
