# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/anacromaniac/pexposure/compare/v0.3.0...v0.4.0) - 2026-10-02

### Added

- add --rebalance with a class band, a tighter leverage band and a minimum leg ([#9](https://github.com/anacromaniac/pexposure/pull/9))

### Added

- add --rebalance to move existing holdings toward the targets, keeping the capital
  constant, with a class tolerance band, a tighter leverage band, and a minimum leg

## [0.3.0](https://github.com/anacromaniac/pexposure/compare/v0.2.0...v0.3.0) - 2026-10-02

### Added

- add --usage flag with command-line examples ([#6](https://github.com/anacromaniac/pexposure/pull/6))

## [0.2.0](https://github.com/anacromaniac/pexposure/compare/v0.1.0...v0.2.0) - 2026-10-02

### Added

- add --update to install the latest release ([#3](https://github.com/anacromaniac/pexposure/pull/3))

### Fixed

- take the release baseline from git tags ([#4](https://github.com/anacromaniac/pexposure/pull/4))

### Other

- add CI workflow and unit tests ([#2](https://github.com/anacromaniac/pexposure/pull/2))
- release v0.1.0 ([#1](https://github.com/anacromaniac/pexposure/pull/1))

## [0.1.0](https://github.com/anacromaniac/pexposure/releases/tag/v0.1.0) - 2026-10-02

### Added

- add look-through allocation report

### Fixed

- exit quietly when the stdout pipe closes

### Other

- add release-plz for versioning and changelog
- add cargo-dist release pipeline
- move example data under example/
- add AGPL-3.0 license
