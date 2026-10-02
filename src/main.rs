//! pexposure - look-through portfolio allocation report.
//!
//! Reads `$HOME/.pexposure/portfolio.yaml` by default, or a file passed as the
//! first argument. Targets are in % of total value and sum to the target
//! leverage; instruments give exposure per 1.00 of value, so their sum is the
//! instrument leverage.
//!
//! Two views of the same drift:
//!   nominal     actual - target on the leverage-inclusive basis (sums above 100%)
//!   composition actual - target in share of exposure (normalized to 100%)

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, anyhow, ensure};
use clap::Parser;
use indexmap::IndexMap;
use itertools::Itertools;
use serde::Deserialize;

const DEFAULT_FILE: &str = ".pexposure/portfolio.yaml";

#[derive(Parser)]
#[command(
    name = "pexposure",
    version,
    about = "Look-through portfolio allocation report"
)]
struct Cli {
    /// Add the normalized-to-100 composition view
    #[arg(long)]
    composition: bool,

    /// Split the amount across instruments instead of ranking them
    #[arg(long)]
    split: bool,

    /// Cap the number of instruments used by --split
    #[arg(long, value_name = "N")]
    max: Option<usize>,

    /// [FILE] [AMOUNT] [TICKER]
    #[arg(value_name = "ARG")]
    args: Vec<String>,
}

#[derive(Deserialize)]
struct Config {
    targets: IndexMap<String, f64>,
    instruments: IndexMap<String, IndexMap<String, f64>>,
    holdings: IndexMap<String, f64>,
}

struct Portfolio {
    targets: IndexMap<String, f64>,
    instruments: IndexMap<String, IndexMap<String, f64>>,
    holdings: IndexMap<String, f64>,
}

impl Portfolio {
    fn load(path: &Path) -> Result<Self> {
        ensure!(path.exists(), "file not found: {}", path.display());
        let text = std::fs::read_to_string(path)?;
        let config: Config = serde_saphyr::from_str(&text)?;

        for (name, exposure) in &config.instruments {
            for class in exposure.keys() {
                ensure!(
                    config.targets.contains_key(class),
                    "{name}: unknown class '{class}'"
                );
            }
        }
        for name in config.holdings.keys() {
            ensure!(
                config.instruments.contains_key(name),
                "holdings: unknown instrument '{name}'"
            );
        }

        Ok(Portfolio {
            targets: config.targets,
            instruments: config.instruments,
            holdings: config.holdings,
        })
    }

    fn total(&self) -> f64 {
        self.holdings.values().sum()
    }

    /// Look-through exposure per class, unchanged keys ordered like `targets`.
    fn exposure(&self) -> IndexMap<String, f64> {
        let mut exposure: IndexMap<String, f64> =
            self.targets.keys().map(|c| (c.clone(), 0.0)).collect();
        for (name, value) in &self.holdings {
            for (class, multiplier) in &self.instruments[name] {
                *exposure.get_mut(class).unwrap() += value * multiplier;
            }
        }
        exposure
    }

    /// Sum of absolute drift, used to rank single-instrument investments.
    fn drift_sum(&self, total: f64, exposure: &IndexMap<String, f64>) -> f64 {
        self.targets
            .iter()
            .map(|(class, target)| (exposure[class] / total - target).abs())
            .sum()
    }
}

/// Thousands-separated integer, like Python's `{:,.0f}`.
fn money(x: f64) -> String {
    let formatted = format!("{x:.0}");
    let (sign, digits) = formatted
        .strip_prefix('-')
        .map_or(("", formatted.as_str()), |d| ("-", d));

    let mut out = String::with_capacity(formatted.len() + formatted.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    format!("{sign}{out}")
}

fn table_nominal(total: f64, exposure: &IndexMap<String, f64>, targets: &IndexMap<String, f64>) {
    println!(
        "  {:<18}{:>9}{:>9}{:>11}{:>12}",
        "class", "actual", "target", "drift", "gap"
    );

    let mut rows: Vec<_> = targets
        .iter()
        .map(|(class, target)| {
            let actual = exposure[class] / total;
            (
                class.as_str(),
                actual,
                *target,
                actual - target,
                target * total - exposure[class],
            )
        })
        .collect();
    rows.sort_by(|a, b| b.3.abs().partial_cmp(&a.3.abs()).unwrap_or(Ordering::Equal));

    for (class, actual, target, drift, gap) in rows {
        println!(
            "  {class:<18}{:>7.2}%{:>8.2}%{:>+9.2}pp{:>12}",
            actual * 100.0,
            target * 100.0,
            drift * 100.0,
            money(gap)
        );
    }
}

fn table_composition(exposure: &IndexMap<String, f64>, targets: &IndexMap<String, f64>) {
    println!(
        "  {:<18}{:>9}{:>9}{:>11}",
        "class", "actual", "target", "drift"
    );

    let exposure_total: f64 = exposure.values().sum();
    let target_total: f64 = targets.values().sum();
    let mut rows: Vec<_> = targets
        .iter()
        .map(|(class, target)| {
            let share = exposure[class] / exposure_total;
            let target_share = target / target_total;
            (class.as_str(), share, target_share, share - target_share)
        })
        .collect();
    rows.sort_by(|a, b| b.3.abs().partial_cmp(&a.3.abs()).unwrap_or(Ordering::Equal));

    for (class, share, target_share, drift) in rows {
        println!(
            "  {class:<18}{:>7.2}%{:>8.2}%{:>+9.2}pp",
            share * 100.0,
            target_share * 100.0,
            drift * 100.0
        );
    }
}

fn report(
    label: &str,
    total: f64,
    exposure: &IndexMap<String, f64>,
    targets: &IndexMap<String, f64>,
    composition: bool,
) {
    let exposure_total: f64 = exposure.values().sum();
    let target_total: f64 = targets.values().sum();

    println!("{label}");
    println!(
        "  total {}   exposure {}   leverage {:.3}x (target {:.3}x)",
        money(total),
        money(exposure_total),
        exposure_total / total,
        target_total
    );
    println!();
    println!("  nominal");
    table_nominal(total, exposure, targets);

    if composition {
        println!();
        println!("  composition (share of exposure, normalized to 100%)");
        table_composition(exposure, targets);
    }
}

/// Greedy split of `amount` over `subset`, minimizing the sum of squared drift.
fn solve(
    total: f64,
    exposure: &IndexMap<String, f64>,
    targets: &IndexMap<String, f64>,
    instruments: &IndexMap<String, IndexMap<String, f64>>,
    amount: f64,
    subset: &[String],
    steps: usize,
) -> (IndexMap<String, f64>, f64) {
    let new_total = total + amount;
    let mut weights: IndexMap<String, f64> = subset.iter().map(|n| (n.clone(), 0.0)).collect();

    let cost = |weights: &IndexMap<String, f64>| {
        targets
            .iter()
            .map(|(class, target)| {
                let added: f64 = subset
                    .iter()
                    .map(|name| {
                        weights[name] * instruments[name].get(class).copied().unwrap_or(0.0)
                    })
                    .sum();
                ((exposure[class] + added) / new_total - target).powi(2)
            })
            .sum()
    };

    let step = amount / steps as f64;
    for _ in 0..steps {
        let base = cost(&weights);
        let mut best: Option<(f64, usize)> = None;
        for (i, name) in subset.iter().enumerate() {
            weights[name] += step;
            let candidate = cost(&weights);
            weights[name] -= step;
            if best.is_none_or(|(cost, _)| candidate < cost) {
                best = Some((candidate, i));
            }
        }

        let Some((best_cost, best_index)) = best else {
            break;
        };
        if best_cost >= base {
            break;
        }
        weights[&subset[best_index]] += step;
    }

    let final_cost = cost(&weights);
    (weights, final_cost)
}

/// Ranking key for candidate subsets, compared lexicographically like a Python
/// tuple: least drift, then fewest instruments, then fewest new positions.
#[derive(PartialEq, PartialOrd)]
struct Rank {
    cost: f64,
    used: usize,
    new_positions: usize,
}

/// Pick the subset (up to `max`) and split that leaves the least drift.
/// Ties prefer fewer instruments and no new positions.
fn split(
    total: f64,
    exposure: &IndexMap<String, f64>,
    targets: &IndexMap<String, f64>,
    instruments: &IndexMap<String, IndexMap<String, f64>>,
    holdings: &IndexMap<String, f64>,
    amount: f64,
    max: Option<usize>,
) -> IndexMap<String, f64> {
    let names: Vec<String> = instruments.keys().cloned().collect();
    let subsets: Vec<Vec<String>> = match max {
        None => vec![names],
        Some(max) => (1..=max.min(names.len()))
            .flat_map(|k| names.iter().cloned().combinations(k))
            .collect(),
    };

    let mut best: Option<(Rank, IndexMap<String, f64>)> = None;
    for subset in subsets {
        let (weights, cost) = solve(total, exposure, targets, instruments, amount, &subset, 400);
        let chosen: Vec<&String> = weights
            .iter()
            .filter(|(_, weight)| **weight != 0.0)
            .map(|(name, _)| name)
            .collect();

        // Rounding to 12 decimals keeps floating-point noise from splitting.
        let key = Rank {
            cost: (cost * 1e12).round() / 1e12,
            used: chosen.len(),
            new_positions: chosen
                .iter()
                .filter(|name| !holdings.contains_key(name.as_str()))
                .count(),
        };
        if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
            best = Some((key, weights));
        }
    }

    best.expect("at least one subset").1
}

fn parse_amount(raw: &str) -> Result<f64> {
    raw.replace(',', "")
        .parse()
        .map_err(|_| anyhow!("invalid amount: {raw}"))
}

fn default_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(DEFAULT_FILE))
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let mut positional = cli.args.iter();
    let mut file = default_path()?;
    let mut amount_arg = None;
    let mut ticker = None;

    if let Some(first) = positional.next() {
        if first.ends_with(".yaml") || first.ends_with(".yml") {
            file = PathBuf::from(first);
            amount_arg = positional.next();
            ticker = positional.next();
        } else {
            amount_arg = Some(first);
            ticker = positional.next();
        }
    }

    let amount = amount_arg.map(|raw| parse_amount(raw)).transpose()?;

    let portfolio = Portfolio::load(&file)?;
    if let Some(ticker) = ticker {
        ensure!(
            portfolio.instruments.contains_key(ticker),
            "unknown instrument: {ticker}"
        );
    }

    let total = portfolio.total();
    let exposure = portfolio.exposure();
    report(
        "current",
        total,
        &exposure,
        &portfolio.targets,
        cli.composition,
    );

    let Some(amount) = amount else {
        return Ok(());
    };
    println!();

    if cli.split {
        let weights = split(
            total,
            &exposure,
            &portfolio.targets,
            &portfolio.instruments,
            &portfolio.holdings,
            amount,
            cli.max,
        );

        let mut new_exposure = exposure.clone();
        for (name, weight) in &weights {
            for (class, multiplier) in &portfolio.instruments[name] {
                new_exposure[class] += weight * multiplier;
            }
        }

        let chosen: Vec<(String, f64)> = weights
            .iter()
            .filter(|(_, weight)| **weight != 0.0)
            .map(|(name, weight)| (name.clone(), *weight))
            .collect();

        println!(
            "  split {} over {} instrument(s):",
            money(amount),
            chosen.len()
        );
        println!(
            "  {:<14}{:>10}{:>9}{:>10}",
            "instrument", "amount", "share", "leverage"
        );
        for (name, weight) in &chosen {
            let leverage: f64 = portfolio.instruments[name].values().sum();
            let alternatives: Vec<&str> = portfolio
                .instruments
                .iter()
                .filter(|(other, exposure)| {
                    other.as_str() != name.as_str()
                        && **exposure == portfolio.instruments[name]
                        && !chosen
                            .iter()
                            .any(|(chosen, _)| chosen.as_str() == other.as_str())
                })
                .map(|(other, _)| other.as_str())
                .collect();
            let note = if alternatives.is_empty() {
                String::new()
            } else {
                format!("   (= {})", alternatives.join(", "))
            };
            println!(
                "  {name:<14}{:>10}{:>7.0}%{:>9.2}x{note}",
                money(*weight),
                weight / amount * 100.0,
                leverage
            );
        }
        println!();
        report(
            &format!("after +{}", money(amount)),
            total + amount,
            &new_exposure,
            &portfolio.targets,
            cli.composition,
        );
        return Ok(());
    }

    if let Some(ticker) = ticker {
        let mut new_exposure = exposure.clone();
        for (class, multiplier) in &portfolio.instruments[ticker] {
            new_exposure[class] += amount * multiplier;
        }
        report(
            &format!("after +{} {ticker}", money(amount)),
            total + amount,
            &new_exposure,
            &portfolio.targets,
            cli.composition,
        );
        return Ok(());
    }

    let new_total = total + amount;
    let current = portfolio.drift_sum(total, &exposure);
    let mut offers: Vec<_> = portfolio
        .instruments
        .iter()
        .map(|(name, instrument)| {
            let shifted: IndexMap<String, f64> = portfolio
                .targets
                .keys()
                .map(|class| {
                    (
                        class.clone(),
                        exposure[class] + amount * instrument.get(class).copied().unwrap_or(0.0),
                    )
                })
                .collect();
            let exposure_total: f64 = shifted.values().sum();
            (
                portfolio.drift_sum(new_total, &shifted),
                name.as_str(),
                exposure_total / new_total,
            )
        })
        .collect();
    offers.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.1.cmp(b.1))
            .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(Ordering::Equal))
    });

    println!("  invest {} in one instrument:", money(amount));
    println!(
        "  {:<14}{:>9}{:>9}{:>10}",
        "instrument", "drift", "change", "leverage"
    );
    for (drift, name, leverage) in offers {
        println!(
            "  {name:<14}{:>7.2}pp{:>+7.2}pp{leverage:>9.3}x",
            drift * 100.0,
            (drift - current) * 100.0
        );
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
