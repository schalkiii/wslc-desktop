# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.2.5] - 2026-10-06

### Security

- **Removed real credentials from the bundled command presets.** The seed
  library shipped actual passwords (`WEBUI_PASSWORD`, `POSTGRES_PASSWORD`,
  `redis --requirepass`) and intranet proxy IPs that had been pushed to the
  public repository. They are now `<your-password>` / `<proxy-host>`
  placeholders, with a regression test (`saved_command_presets_are_sanitized_and_runnable`)
  guarding against reintroduction. Note: the old values remain in git history —
  rotate those credentials.

### Added

- **Comprehensive test plan** (`docs/测试计划.md`): feature-matrix coverage
  strategy, the full 41-case automated matrix, review-fix log, a manual smoke
  checklist for release gating, and known limitations.
- Unit tests grew from 20 to **45, all passing**, covering: both wslc JSON
  schema generations (containers/images/networks/volumes), copyright-header
  stripping edge cases, `RunSpec` argument construction (full flag matrix,
  quote payloads, preview quoting), tokenizer edge cases, filter/truncate/
  state-color/history-cap logic, image-reference normalization, and the CJK
  font glyph set.

### Fixed

- The run wizard split its Command field on whitespace, tearing
  `bash -c "echo hi && ls"` payloads into wrong arguments; it now uses the
  same shell-style tokenizer as saved commands.
- The run preview did not quote arguments containing spaces, so a copied
  preview line was not directly executable.
- A volume-list parse failure used to blank the whole snapshot (containers and
  images included); volumes/networks are now best-effort while
  containers/images remain hard errors.

## [0.2.4] - 2026-10-06

### Fixed

- **wslc 3.x compatibility: container/image lists came up empty.** wslc 3.0.1.0
  changed all `--format json` output destructively (verified locally): the
  single JSON array became newline-delimited objects, container keys `Id`/`Name`
  were renamed to `ID`/`Names`, numeric `State` codes became strings
  (`"exited"`), unix timestamps became human strings
  (`"2026-07-15 21:03:40 +0800 GMT+8"`), container `Ports` became docker-style
  text (`"127.0.0.1:8082->8088/tcp"`), image `Size` became strings (`"210MB"`)
  and `stats` switched to full 64-char IDs while `list` still reports 12-char
  short IDs. Parsing therefore failed wholesale and every list in the GUI
  stayed empty (previously populated under wslc 2.9.3.0).
  - `run_json` now accepts both generations: it tries the legacy JSON array
    first, then falls back to line-by-line NDJSON parsing (a single bad line
    no longer blanks the whole list; an all-lines failure still errors).
  - `types.rs` deserializers accept numeric *and* string forms of state,
    times, sizes and ports; `Id`/`ID`, `Name`/`Names`, `Created`/`CreatedAt`
    key aliases cover both schemas.
  - Stats maps normalize keys to the 12-char short ID, so CPU/memory columns
    and history charts match containers again under wslc 3.x.
  - The container table's "changed" column now shows the richer wslc ≥3.x
    `Status` line (e.g. `Exited (255) 21 minutes ago`, `Up 3 seconds`), falling
    back to relative time for ≤2.x.
- Fixed a `clippy::for_kv_map` warning surfaced by the newer toolchain.

### Changed

- Verified against **wslc 3.0.1.0** in addition to 2.9.3.0; README + design doc
  record the 3.x schema drift (see `docs/竞品拆解与技术方案.md` §3.1.1).
