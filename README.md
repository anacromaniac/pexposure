# pexposure

Look-through portfolio allocation report.

Given a portfolio that contains leveraged or mixed instruments, `pexposure` computes
the real exposure to each asset class and shows how far it drifts from your targets,
plus where new money should go.

The motivating case: a leveraged fund can report `100` of value while providing more
than `100` of exposure, split across several asset classes. It must be counted by that
real exposure, not as a single class.

## Install

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/anacromaniac/pexposure/releases/latest/download/pexposure-installer.sh | sh
```

Installs `pexposure` (and its updater) into `~/.cargo/bin`. Update to the latest
release with:

```sh
pexposure --update
```

## Build from source

```bash
cargo build --release
ln -s "$PWD/target/release/pexposure" ~/.local/bin/pexposure
```

Requires `~/.local/bin` on `PATH`. The binary reads its data file from
`$HOME/.pexposure/portfolio.yaml` by default, independent of the current directory.

## Data

Copy the example and edit it:

```bash
mkdir -p ~/.pexposure
cp example/portfolio.yaml ~/.pexposure/portfolio.yaml
```

```yaml
targets:            # % of total value; the sum IS the target leverage (1.30x)
  equities: 0.80
  treasuries: 0.30
  gold: 0.20

instruments:        # exposure per 1.00 of value; the sum is the instrument leverage
  LEV15: {equities: 0.75, treasuries: 0.75}   # 1.5x, split 50/50
  WORLD: {equities: 1.00}

holdings:           # current value per instrument; sum positions across brokers
  LEV15: 12000
  WORLD: 15000
```

## Usage

```bash
pexposure                          # nominal drift vs targets
pexposure --composition            # also show the normalized-to-100 view
pexposure 5000                     # rank single instruments for a 5000 investment
pexposure 5000 LEV15               # report after investing 5000 in LEV15
pexposure 5000 --split             # split 5000 across all instruments
pexposure 5000 --split --max 2     # split across at most 2 instruments
pexposure my.yaml 5000 --split     # custom data file
pexposure --update                 # install the latest release
pexposure --usage                  # show command-line examples
```

The data file can be given as the first argument; otherwise the default is used.

## Reading the output

- **nominal** — percentages of total value. The actual column sums to the current
  leverage (above 100%), so it is what you will see. `gap` is the euro exposure needed
  to reach the target on that basis.
- **composition** (`--composition`) — the same positions with the leverage divided out
  (share of exposure, always 100%). This is the pure tilt, regardless of leverage.

The two differ on purpose: a class can look underweight nominally mostly because the
whole portfolio is under-levered. Keeping them separate avoids chasing the wrong number.

## Notes

- Targets sum to the target leverage. With unlevered instruments only, realized leverage
  drifts down over time.
- `--split` minimizes the resulting drift and prefers fewer instruments and positions you
  already hold. `--max N` caps the number of instruments; it uses fewer when a second one
  would not reduce drift.
- Instruments with identical exposure are alternatives: only one is chosen, the other is
  shown as `(= TICKER)`.

## License

AGPL-3.0.
