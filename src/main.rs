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

use anyhow::{Context, Result, anyhow, ensure};
use clap::Parser;
use indexmap::IndexMap;
use itertools::Itertools;
use serde::Deserialize;

const DEFAULT_FILE: &str = ".pexposure/portfolio.yaml";

/// Default tolerance band for --rebalance, in percentage points of total value.
const DEFAULT_BAND_PP: f64 = 5.0;

/// Default leverage tolerance band for --rebalance, in percentage points.
/// Kept tighter than the class band: the overall leverage matters more than a
/// small sleeve sitting a couple of points off its target.
const DEFAULT_LEVERAGE_BAND_PP: f64 = 1.0;

/// Default minimum trade size for --rebalance, as a percentage of total value.
const DEFAULT_MIN_PCT: f64 = 0.5;

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
    #[arg(long, conflicts_with = "rebalance")]
    split: bool,

    /// Cap the number of instruments used by --split
    #[arg(long, value_name = "N")]
    max: Option<usize>,

    /// Rebalance the existing holdings, keeping the capital constant
    #[arg(long, conflicts_with = "split")]
    rebalance: bool,

    /// Tolerance band in percentage points for --rebalance
    #[arg(long, value_name = "PP", default_value_t = DEFAULT_BAND_PP)]
    band: f64,

    /// Leverage tolerance band in percentage points for --rebalance
    #[arg(long, value_name = "PP", default_value_t = DEFAULT_LEVERAGE_BAND_PP)]
    leverage_band: f64,

    /// Smallest trade as a percentage of the portfolio for --rebalance
    #[arg(long, value_name = "PCT", default_value_t = DEFAULT_MIN_PCT)]
    min: f64,

    /// Install the latest release, if newer than this build
    #[arg(long)]
    update: bool,

    /// Show command-line examples and exit
    #[arg(long)]
    usage: bool,

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

/// Instruments with the same exposure as `name`, excluding the chosen ones.
/// Such alternatives are interchangeable: only one is traded, the rest shown.
fn equivalent_instruments<'a>(
    name: &str,
    instruments: &'a IndexMap<String, IndexMap<String, f64>>,
    chosen: &[String],
) -> Vec<&'a str> {
    instruments
        .iter()
        .filter(|(other, exposure)| {
            other.as_str() != name
                && **exposure == instruments[name]
                && !chosen.iter().any(|c| c == other.as_str())
        })
        .map(|(other, _)| other.as_str())
        .collect()
}

/// Cash-neutral rebalance: signed value changes per instrument, negative for a
/// sale and positive for a purchase, summing to zero.
///
/// Only classes whose drift exceeds `band` (a fraction of total value) are
/// touched, and each is driven to the nearest band edge rather than to its
/// target, so the plan never aims for a pixel-perfect fit. The overall leverage
/// is checked against its own, usually tighter `leverage_band`. Legs below
/// `min_leg` are dropped, which keeps the plan coarse instead of selling one
/// instrument to buy many small ones.
/// Rebalance tolerances: how far each class and the leverage may drift from
/// their targets, and the smallest leg worth trading.
struct Tolerance {
    band: f64,
    leverage_band: f64,
    min_leg: f64,
}

fn plan_rebalance(portfolio: &Portfolio, tolerance: &Tolerance) -> Vec<(String, f64)> {
    let total = portfolio.total();
    if total <= 0.0 {
        return Vec::new();
    }
    let exposure = portfolio.exposure();
    let targets = &portfolio.targets;
    let instruments = &portfolio.instruments;
    let holdings = &portfolio.holdings;
    let Tolerance {
        band,
        leverage_band,
        min_leg,
    } = *tolerance;

    let classes: Vec<(&String, f64)> = targets.iter().map(|(c, t)| (c, *t)).collect();
    let target_leverage: f64 = targets.values().sum();
    let names: Vec<&String> = instruments.keys().collect();
    let matrix: Vec<Vec<f64>> = names
        .iter()
        .map(|name| {
            classes
                .iter()
                .map(|(class, _)| instruments[*name].get(*class).copied().unwrap_or(0.0))
                .collect()
        })
        .collect();
    let leverage: Vec<f64> = matrix.iter().map(|row| row.iter().sum()).collect();
    let mut values: Vec<f64> = names
        .iter()
        .map(|name| holdings.get(*name).copied().unwrap_or(0.0))
        .collect();
    let mut current: Vec<f64> = classes.iter().map(|(class, _)| exposure[*class]).collect();
    let mut total_exposure: f64 = current.iter().sum();

    // Drift beyond the band, summed over the classes and the overall leverage.
    // The leverage is the sum of the class drifts, but checking it on its own
    // keeps a portfolio that is uniformly under-levered from looking balanced.
    let violation = |current: &[f64], total_exposure: f64| -> f64 {
        let class_drift: f64 = classes
            .iter()
            .enumerate()
            .map(|(i, (_, target))| ((current[i] / total - target).abs() - band).max(0.0))
            .sum();
        let leverage_drift =
            ((total_exposure / total - target_leverage).abs() - leverage_band).max(0.0);
        class_drift + leverage_drift
    };

    let mut change = vec![0.0; names.len()];
    let max_rounds = names.len() * classes.len() + 1;

    for _ in 0..max_rounds {
        let base = violation(&current, total_exposure);
        if base <= 0.0 {
            break;
        }

        // Best swap so far: (sell, buy, delta, reduction, buy is already held).
        let mut best: Option<(usize, usize, f64, f64, bool)> = None;
        for sell in 0..names.len() {
            if values[sell] <= 0.0 {
                continue;
            }
            for buy in 0..names.len() {
                if sell == buy {
                    continue;
                }
                let dv: Vec<f64> = (0..classes.len())
                    .map(|c| matrix[buy][c] - matrix[sell][c])
                    .collect();
                let dv_leverage = leverage[buy] - leverage[sell];

                // Moving `delta` from sell to buy changes each exposure linearly,
                // so the violation is convex piecewise linear: its minimum over
                // the swap is at a band edge or at an endpoint.
                let mut points = vec![0.0, values[sell]];
                for (c, (_, target)) in classes.iter().enumerate() {
                    if dv[c] == 0.0 {
                        continue;
                    }
                    for edge in [(target - band) * total, (target + band) * total] {
                        let delta = (edge - current[c]) / dv[c];
                        if delta > 0.0 && delta < values[sell] {
                            points.push(delta);
                        }
                    }
                }
                if dv_leverage != 0.0 {
                    for edge in [
                        (target_leverage - leverage_band) * total,
                        (target_leverage + leverage_band) * total,
                    ] {
                        let delta = (edge - total_exposure) / dv_leverage;
                        if delta > 0.0 && delta < values[sell] {
                            points.push(delta);
                        }
                    }
                }
                points.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

                let mut swap_delta = 0.0;
                let mut best_value = base;
                for &delta in &points {
                    let moved: Vec<f64> = (0..classes.len())
                        .map(|c| current[c] + delta * dv[c])
                        .collect();
                    let value = violation(&moved, total_exposure + delta * dv_leverage);
                    if value < best_value - 1e-12 {
                        best_value = value;
                        swap_delta = delta;
                    }
                }

                let reduction = base - best_value;
                if swap_delta < min_leg || reduction <= 1e-12 {
                    continue;
                }
                let buy_held = values[buy] > 0.0;
                // Prefer the swap that buys the most drift per euro traded, so a
                // cheap leg is not passed over for a bigger but far costlier one.
                let efficiency = reduction / swap_delta;
                let is_better = match &best {
                    None => true,
                    Some((_, _, best_delta, best_reduction, best_held)) => {
                        let best_efficiency = best_reduction / best_delta;
                        if (efficiency - best_efficiency).abs() > 1e-15 {
                            efficiency > best_efficiency
                        } else if (reduction - best_reduction).abs() > 1e-12 {
                            reduction > *best_reduction
                        } else if buy_held != *best_held {
                            buy_held
                        } else {
                            // Same efficiency: prefer the shorter leg, which lands
                            // on the nearest band edge.
                            swap_delta < *best_delta
                        }
                    }
                };
                if is_better {
                    best = Some((sell, buy, swap_delta, reduction, buy_held));
                }
            }
        }

        let Some((sell, buy, delta, _, _)) = best else {
            break;
        };
        values[sell] -= delta;
        values[buy] += delta;
        for (c, _) in classes.iter().enumerate() {
            current[c] += delta * (matrix[buy][c] - matrix[sell][c]);
        }
        total_exposure += delta * (leverage[buy] - leverage[sell]);
        change[sell] -= delta;
        change[buy] += delta;
    }

    names
        .iter()
        .enumerate()
        .filter(|(i, _)| change[*i].abs() > 1e-9)
        .map(|(i, name)| ((*name).clone(), change[i]))
        .collect()
}

const USAGE: &str = "\
usage: pexposure [FILE] [AMOUNT] [TICKER] [OPTIONS]

  pexposure                          nominal drift vs targets
  pexposure --composition            also show the normalized-to-100 view
  pexposure 5000                     rank single instruments for a 5000 investment
  pexposure 5000 LEV15               report after investing 5000 in LEV15
  pexposure 5000 --split             split 5000 across all instruments
  pexposure 5000 --split --max 2     split across at most 2 instruments
  pexposure --rebalance              rebalance existing holdings, cash-neutral
  pexposure --rebalance --band 3     rebalance with a +/-3pp tolerance band
  pexposure my.yaml 5000 --split     custom data file (default ~/.pexposure/portfolio.yaml)
  pexposure --update                 install the latest release

Run `pexposure --help` for the full list of options.";

fn parse_amount(raw: &str) -> Result<f64> {
    raw.replace(',', "")
        .parse()
        .map_err(|_| anyhow!("invalid amount: {raw}"))
}

fn default_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(DEFAULT_FILE))
}

/// Path of the sibling `pexposure-update` program installed by the shell installer.
fn updater_binary_path(exe: &Path) -> PathBuf {
    #[cfg(windows)]
    const NAME: &str = "pexposure-update.exe";
    #[cfg(not(windows))]
    const NAME: &str = "pexposure-update";
    exe.with_file_name(NAME)
}

/// Install the latest release by running the updater installed alongside this binary.
fn run_update() -> Result<()> {
    let exe = std::env::current_exe().context("cannot locate the current executable")?;
    let updater = updater_binary_path(&exe);
    ensure!(
        updater.exists(),
        "no updater found at {}; reinstall pexposure with the shell installer to enable --update",
        updater.display()
    );
    let status = std::process::Command::new(&updater)
        .status()
        .with_context(|| format!("failed to run {}", updater.display()))?;
    ensure!(status.success(), "the updater exited with {status}");
    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.usage {
        println!("{USAGE}");
        return Ok(());
    }

    if cli.update {
        return run_update();
    }

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

    if cli.rebalance {
        ensure!(
            amount.is_none(),
            "--rebalance keeps capital constant; remove the amount"
        );
        ensure!(
            ticker.is_none(),
            "--rebalance cannot target a single instrument"
        );
        ensure!(cli.band >= 0.0, "--band must not be negative");
        ensure!(
            cli.leverage_band >= 0.0,
            "--leverage-band must not be negative"
        );
        ensure!(cli.min >= 0.0, "--min must not be negative");
    }

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

    if cli.rebalance {
        let mut trades = plan_rebalance(
            &portfolio,
            &Tolerance {
                band: cli.band / 100.0,
                leverage_band: cli.leverage_band / 100.0,
                min_leg: total * cli.min / 100.0,
            },
        );

        println!();
        if trades.is_empty() {
            println!(
                "  rebalance: within +/-{:.1}pp (leverage +/-{:.1}pp), no trades needed",
                cli.band, cli.leverage_band
            );
            return Ok(());
        }

        trades.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        let turnover: f64 = trades.iter().map(|(_, change)| change.abs()).sum();
        let traded_names: Vec<String> = trades.iter().map(|(name, _)| name.clone()).collect();

        println!(
            "  rebalance (band +/-{:.1}pp, leverage +/-{:.1}pp, min leg {:.1}%, capital constant):",
            cli.band, cli.leverage_band, cli.min
        );
        println!(
            "  {:<7}{:<14}{:>10}{:>10}",
            "action", "instrument", "amount", "leverage"
        );
        for (name, change) in &trades {
            let action = if *change < 0.0 { "sell" } else { "buy" };
            let leverage: f64 = portfolio.instruments[name].values().sum();
            let alternatives = equivalent_instruments(name, &portfolio.instruments, &traded_names);
            let note = if alternatives.is_empty() {
                String::new()
            } else {
                format!("   (= {})", alternatives.join(", "))
            };
            println!(
                "  {action:<7}{name:<14}{:>10}{leverage:>9.2}x{note}",
                money(change.abs())
            );
        }
        println!(
            "  turnover {} ({:.1}% of portfolio)",
            money(turnover),
            turnover / total * 100.0
        );

        let mut new_exposure = exposure.clone();
        for (name, change) in &trades {
            for (class, multiplier) in &portfolio.instruments[name] {
                new_exposure[class] += change * multiplier;
            }
        }
        println!();
        report(
            "after rebalance",
            total,
            &new_exposure,
            &portfolio.targets,
            cli.composition,
        );
        return Ok(());
    }

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
        let chosen_names: Vec<String> = chosen.iter().map(|(name, _)| name.clone()).collect();

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
            let alternatives = equivalent_instruments(name, &portfolio.instruments, &chosen_names);
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

/// Rust ignores `SIGPIPE`, so writing to a closed pipe panics. Restore the
/// default action so `pexposure | head` exits quietly like other Unix tools.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: only resets a signal handler to its default action, and the
    // process is still single-threaded here.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

fn main() -> ExitCode {
    reset_sigpipe();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn money_groups_thousands() {
        assert_eq!(money(0.0), "0");
        assert_eq!(money(999.0), "999");
        assert_eq!(money(1000.0), "1,000");
        assert_eq!(money(30_371.34), "30,371");
        assert_eq!(money(-2_397.0), "-2,397");
        assert_eq!(money(1_234_567.0), "1,234,567");
    }

    #[test]
    fn rank_prefers_lower_cost_then_fewer_instruments() {
        let cheaper = Rank {
            cost: 0.5,
            used: 3,
            new_positions: 3,
        };
        let fewer = Rank {
            cost: 1.0,
            used: 1,
            new_positions: 5,
        };
        let more = Rank {
            cost: 1.0,
            used: 2,
            new_positions: 0,
        };
        assert!(cheaper < fewer);
        assert!(fewer < more);
    }

    #[test]
    fn updater_sits_next_to_the_binary() {
        let expected = if cfg!(windows) {
            "pexposure-update.exe"
        } else {
            "pexposure-update"
        };
        let path = updater_binary_path(Path::new("/usr/bin/pexposure"));
        assert_eq!(path, PathBuf::from("/usr/bin").join(expected));
    }

    fn map_of(pairs: &[(&str, f64)]) -> IndexMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    fn instrument_map(pairs: &[(&str, &[(&str, f64)])]) -> IndexMap<String, IndexMap<String, f64>> {
        pairs
            .iter()
            .map(|(name, exposure)| (name.to_string(), map_of(exposure)))
            .collect()
    }

    fn plan(
        targets: &[(&str, f64)],
        instruments: &[(&str, &[(&str, f64)])],
        holdings: &[(&str, f64)],
        band: f64,
        leverage_band: f64,
        min_leg: f64,
    ) -> Vec<(String, f64)> {
        let portfolio = Portfolio {
            targets: map_of(targets),
            instruments: instrument_map(instruments),
            holdings: map_of(holdings),
        };
        plan_rebalance(
            &portfolio,
            &Tolerance {
                band,
                leverage_band,
                min_leg,
            },
        )
    }

    #[test]
    fn rebalance_drives_overweight_to_the_band_edge() {
        let trades = plan(
            &[("a", 0.5), ("b", 0.5)],
            &[("A", &[("a", 1.0)]), ("B", &[("b", 1.0)])],
            &[("A", 7000.0), ("B", 3000.0)],
            0.05,
            0.05,
            0.0,
        );
        // 70/30 against 50/50: sell A down to the 55% edge and buy B up to 45%.
        assert_eq!(
            trades,
            vec![("A".to_string(), -1500.0), ("B".to_string(), 1500.0)]
        );
    }

    #[test]
    fn rebalance_is_silent_inside_the_band() {
        let trades = plan(
            &[("a", 0.5), ("b", 0.5)],
            &[("A", &[("a", 1.0)]), ("B", &[("b", 1.0)])],
            &[("A", 5200.0), ("B", 4800.0)],
            0.05,
            0.05,
            0.0,
        );
        assert!(trades.is_empty());
    }

    #[test]
    fn rebalance_skips_legs_below_the_minimum() {
        let trades = plan(
            &[("a", 0.5), ("b", 0.5)],
            &[("A", &[("a", 1.0)]), ("B", &[("b", 1.0)])],
            &[("A", 5150.0), ("B", 4850.0)],
            0.01,
            0.01,
            100.0,
        );
        // The fix is only 50 wide, below the 100 minimum leg.
        assert!(trades.is_empty());
    }

    #[test]
    fn rebalance_conserves_capital() {
        let trades = plan(
            &[("a", 0.6), ("b", 0.4)],
            &[
                ("A", &[("a", 1.0)]),
                ("B", &[("b", 1.0)]),
                ("C", &[("a", 0.5), ("b", 0.5)]),
            ],
            &[("A", 8000.0), ("B", 1000.0), ("C", 1000.0)],
            0.02,
            0.02,
            0.0,
        );
        let net: f64 = trades.iter().map(|(_, change)| change).sum();
        assert!(net.abs() < 1e-9, "net change was {net}");
    }

    #[test]
    fn rebalance_restores_leverage_without_leaving_the_band() {
        // Every class sits on its band edge while the leverage is 1.10x against
        // a 1.20x target: only a leveraged instrument can close it.
        let trades = plan(
            &[("a", 0.6), ("b", 0.6)],
            &[
                ("A2", &[("a", 1.1)]),
                ("B2", &[("b", 1.1)]),
                ("N", &[("a", 1.1), ("b", 0.4)]),
            ],
            &[("A2", 5000.0), ("B2", 5000.0)],
            0.05,
            0.01,
            0.0,
        );
        let trade = |name: &str| {
            trades
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, change)| *change)
        };
        let close = |actual: Option<f64>, expected: f64| {
            actual.is_some_and(|a| (a - expected).abs() < 1e-6)
        };
        // The tight leverage band pulls to its edge (1.19x): 2250 of N.
        assert!(close(trade("A2"), -2250.0));
        assert!(close(trade("N"), 2250.0));
        assert_eq!(trade("B2"), None);
    }

    #[test]
    fn equivalent_instruments_lists_identical_exposure() {
        let instruments = instrument_map(&[
            ("A", &[("a", 1.0)]),
            ("B", &[("a", 1.0)]),
            ("C", &[("a", 1.0), ("b", 0.5)]),
        ]);
        assert_eq!(equivalent_instruments("A", &instruments, &[]), vec!["B"]);
        let chosen = vec!["A".to_string(), "B".to_string()];
        assert!(equivalent_instruments("A", &instruments, &chosen).is_empty());
    }
}
