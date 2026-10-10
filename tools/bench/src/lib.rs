// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::hint::black_box;
use std::time::Instant;

/// Maximum number of samples accepted by a measurement call.
pub const MAX_ITERATIONS: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchmarkResult {
    pub case: String,
    pub iterations: usize,
    /// Median elapsed time per run, in nanoseconds.
    pub median_ns: u128,
}

pub struct BenchmarkCase {
    name: String,
    run: Box<dyn FnMut()>,
}

impl BenchmarkCase {
    /// Register a benchmark workload.
    ///
    /// The closure returns `()`, so it must keep its own work observable with
    /// [`std::hint::black_box`], for example:
    ///
    /// ```
    /// use std::hint::black_box;
    /// let mut values = black_box([3, 1, 2]);
    /// values.sort_unstable();
    /// black_box(values);
    /// ```
    pub fn new(name: impl Into<String>, run: impl FnMut() + 'static) -> Result<Self, String> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self {
            name,
            run: Box::new(run),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn measure(&mut self, iterations: usize) -> Result<BenchmarkResult, String> {
        validate_iterations(iterations)?;
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            (self.run)();
            // Keep emitted reference rows valid even when the platform timer's
            // resolution is coarser than a very short workload.
            samples.push(start.elapsed().as_nanos().max(1));
        }
        let median_ns = median(&mut samples)?;
        Ok(BenchmarkResult {
            case: self.name.clone(),
            iterations,
            median_ns,
        })
    }
}

#[derive(Default)]
pub struct Registry {
    cases: Vec<BenchmarkCase>,
}

impl Registry {
    pub fn register(&mut self, case: BenchmarkCase) -> Result<(), String> {
        if self.cases.iter().any(|existing| existing.name == case.name) {
            return Err(format!("duplicate benchmark case '{}'", case.name));
        }
        self.cases.push(case);
        Ok(())
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.cases.iter().map(BenchmarkCase::name)
    }
    pub fn measure(
        &mut self,
        selected: &[String],
        iterations: usize,
    ) -> Result<Vec<BenchmarkResult>, String> {
        validate_iterations(iterations)?;
        let wanted: BTreeSet<&str> = selected.iter().map(String::as_str).collect();
        for name in &wanted {
            if !self.cases.iter().any(|case| case.name == *name) {
                return Err(format!("unknown benchmark case '{name}'"));
            }
        }
        let mut results = Vec::new();
        for case in &mut self.cases {
            if wanted.is_empty() || wanted.contains(case.name.as_str()) {
                results.push(case.measure(iterations)?);
            }
        }
        Ok(results)
    }
}

fn validate_iterations(iterations: usize) -> Result<(), String> {
    if iterations == 0 {
        return Err("iteration count must be greater than zero".into());
    }
    if iterations > MAX_ITERATIONS {
        return Err(format!(
            "iteration count exceeds the maximum of {MAX_ITERATIONS}"
        ));
    }
    Ok(())
}

fn median(samples: &mut [u128]) -> Result<u128, String> {
    if samples.is_empty() {
        return Err("cannot calculate the median of zero samples".into());
    }
    samples.sort_unstable();
    let middle = samples.len() / 2;
    if samples.len() % 2 == 1 {
        Ok(samples[middle])
    } else {
        let low = samples[middle - 1];
        let high = samples[middle];
        Ok(low / 2 + high / 2 + (low % 2 + high % 2) / 2)
    }
}

pub fn to_csv(results: &[BenchmarkResult]) -> String {
    let mut csv = String::from("case,iterations,median_ns\n");
    for result in results {
        csv.push_str(&format!(
            "{},{},{}\n",
            csv_field(&result.case),
            result.iterations,
            result.median_ns
        ));
    }
    csv
}

pub fn parse_csv(input: &str) -> Result<Vec<BenchmarkResult>, String> {
    let mut lines = input.lines();
    if lines.next() != Some("case,iterations,median_ns") {
        return Err("invalid CSV header; expected case,iterations,median_ns".into());
    }
    let mut results = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, line) in lines.enumerate() {
        if line.is_empty() {
            return Err(format!("CSV line {} is empty", index + 2));
        }
        let fields = parse_csv_row(line).map_err(|e| format!("CSV line {}: {e}", index + 2))?;
        if fields.len() != 3 {
            return Err(format!("CSV line {}: expected 3 fields", index + 2));
        }
        let name = fields[0].clone();
        validate_name(&name).map_err(|e| format!("CSV line {}: {e}", index + 2))?;
        if !seen.insert(name.clone()) {
            return Err(format!("duplicate benchmark case '{name}' in CSV"));
        }
        let iterations = fields[1]
            .parse::<usize>()
            .map_err(|_| format!("CSV line {}: invalid iteration count", index + 2))?;
        if iterations == 0 {
            return Err(format!(
                "CSV line {}: iteration count must be greater than zero",
                index + 2
            ));
        }
        let median_ns = fields[2]
            .parse::<u128>()
            .map_err(|_| format!("CSV line {}: invalid median_ns", index + 2))?;
        if median_ns == 0 {
            return Err(format!(
                "CSV line {}: median_ns must be greater than zero",
                index + 2
            ));
        }
        results.push(BenchmarkResult {
            case: name,
            iterations,
            median_ns,
        });
    }
    if results.is_empty() {
        return Err("CSV contains no benchmark results".into());
    }
    Ok(results)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Regression {
    pub case: String,
    pub reference_ns: u128,
    pub measured_ns: u128,
}

/// A regression is strictly greater than 10%; exactly 110% is accepted.
pub fn compare(
    measured: &[BenchmarkResult],
    reference: &[BenchmarkResult],
) -> Result<Vec<Regression>, String> {
    let refs = unique_map(reference, "reference")?;
    let current = unique_map(measured, "measurement")?;
    for name in current.keys() {
        if !refs.contains_key(name) {
            return Err(format!("reference is missing benchmark case '{name}'"));
        }
    }
    for name in refs.keys() {
        if !current.contains_key(name) {
            return Err(format!("measurement is missing benchmark case '{name}'"));
        }
    }
    Ok(current
        .iter()
        .filter_map(|(name, value)| {
            let baseline = refs[name].median_ns;
            (value.median_ns > baseline.saturating_add(baseline / 10)).then(|| Regression {
                case: name.clone(),
                reference_ns: baseline,
                measured_ns: value.median_ns,
            })
        })
        .collect())
}

fn unique_map<'a>(
    results: &'a [BenchmarkResult],
    label: &str,
) -> Result<BTreeMap<String, &'a BenchmarkResult>, String> {
    if results.is_empty() {
        return Err(format!("{label} contains no benchmark results"));
    }
    let mut map = BTreeMap::new();
    for result in results {
        validate_name(&result.case)?;
        if result.iterations == 0 {
            return Err(format!(
                "{label} case '{}' has zero iterations",
                result.case
            ));
        }
        if result.median_ns == 0 {
            return Err(format!("{label} case '{}' has zero median", result.case));
        }
        if map.insert(result.case.clone(), result).is_some() {
            return Err(format!(
                "duplicate benchmark case '{}' in {label}",
                result.case
            ));
        }
    }
    Ok(map)
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("benchmark case name must not be empty".into());
    }
    if name.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
        return Err("benchmark case name contains an invalid character".into());
    }
    Ok(())
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn parse_csv_row(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        let mut field = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            loop {
                match chars.next() {
                    Some('"') if chars.peek() == Some(&'"') => {
                        chars.next();
                        field.push('"');
                    }
                    Some('"') => break,
                    Some(c) => field.push(c),
                    None => return Err("unterminated quoted field".into()),
                }
            }
            if chars.peek().is_some_and(|c| *c != ',') {
                return Err("unexpected character after quoted field".into());
            }
        } else {
            while chars.peek().is_some_and(|c| *c != ',') {
                field.push(chars.next().expect("peeked character"));
            }
        }
        fields.push(field);
        match chars.next() {
            Some(',') => {}
            None => break,
            Some(_) => unreachable!(),
        }
    }
    Ok(fields)
}

/// Two actual standard-library workloads for validating the multi-case path.
pub fn default_registry() -> Registry {
    let mut registry = Registry::default();
    registry
        .register(
            BenchmarkCase::new("utf8_scan", || {
                let text = black_box("Atelier — performance, édition et caractères Unicode.");
                black_box(text.chars().filter(|c| c.is_alphabetic()).count());
            })
            .expect("static case name"),
        )
        .expect("unique static case");
    registry
        .register(
            BenchmarkCase::new("sort_u64", || {
                let mut values = black_box([91_u64, 3, 44, 18, 7, 256, 1, 32]);
                values.sort_unstable();
                black_box(values);
            })
            .expect("static case name"),
        )
        .expect("unique static case");
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(case: &str, median_ns: u128) -> BenchmarkResult {
        BenchmarkResult {
            case: case.into(),
            iterations: 5,
            median_ns,
        }
    }

    #[test]
    fn measures_n_times_and_reports_case_and_iterations() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = calls.clone();
        let mut case = BenchmarkCase::new("counter", move || seen.set(seen.get() + 1)).unwrap();
        let measured = case.measure(4).unwrap();
        assert_eq!(calls.get(), 4);
        assert_eq!(
            (measured.case.as_str(), measured.iterations),
            ("counter", 4)
        );
        assert!(measured.median_ns > 0);
    }

    #[test]
    fn rejects_extreme_iteration_counts_before_allocating() {
        let mut case = BenchmarkCase::new("counter", || {}).unwrap();
        assert!(case.measure(usize::MAX).unwrap_err().contains("maximum"));
        let mut registry = default_registry();
        assert!(
            registry
                .measure(&[], usize::MAX)
                .unwrap_err()
                .contains("maximum")
        );
    }

    #[test]
    fn median_handles_odd_even_and_large_samples_without_overflow() {
        assert_eq!(median(&mut [9, 1, 3]).unwrap(), 3);
        assert_eq!(median(&mut [12, 1, 9, 4]).unwrap(), 6);
        assert_eq!(median(&mut [u128::MAX, u128::MAX]).unwrap(), u128::MAX);
        assert_eq!(
            median(&mut [u128::MAX, u128::MAX - 1]).unwrap(),
            u128::MAX - 1
        );
        assert!(median(&mut []).is_err());
    }

    #[test]
    fn rejects_zero_iterations_and_duplicate_or_unknown_cases() {
        let mut registry = default_registry();
        assert!(
            registry
                .measure(&[], 0)
                .unwrap_err()
                .contains("greater than zero")
        );
        assert!(
            registry
                .register(BenchmarkCase::new("utf8_scan", || {}).unwrap())
                .unwrap_err()
                .contains("duplicate")
        );
        assert!(
            registry
                .measure(&["missing".into()], 1)
                .unwrap_err()
                .contains("unknown")
        );
        assert!(BenchmarkCase::new("", || {}).is_err());
    }

    #[test]
    fn csv_round_trip_escapes_names_and_rejects_malformed_or_duplicate_rows() {
        let rows = vec![result("sort,\"fast\"", 25)];
        assert_eq!(parse_csv(&to_csv(&rows)).unwrap(), rows);
        for invalid in [
            "wrong\nfoo,1,2\n",
            "case,iterations,median_ns\nfoo,0,2\n",
            "case,iterations,median_ns\nfoo,1,0\n",
            "case,iterations,median_ns\nfoo,1,x\n",
            "case,iterations,median_ns\nfoo,1,2\nfoo,1,3\n",
            "case,iterations,median_ns\n\"bad,1,2\n",
        ] {
            assert!(parse_csv(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn accepts_exactly_ten_percent_and_flags_more() {
        let reference = [result("a", 100)];
        assert!(compare(&[result("a", 110)], &reference).unwrap().is_empty());
        assert_eq!(compare(&[result("a", 111)], &reference).unwrap().len(), 1);
    }

    #[test]
    fn comparison_rejects_missing_duplicate_and_zero_median_reference_rows() {
        assert!(
            compare(&[result("a", 100)], &[result("b", 100)])
                .unwrap_err()
                .contains("missing")
        );
        assert!(
            compare(&[result("a", 100)], &[result("a", 100), result("a", 101)])
                .unwrap_err()
                .contains("duplicate")
        );
        assert!(
            compare(&[result("a", 100)], &[result("a", 0)])
                .unwrap_err()
                .contains("zero median")
        );
    }
}
