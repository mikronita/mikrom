# Project Memory - mikrom-rust

## GitHub Operations
- **Review PR Comments:** To view detailed line-by-line review comments for a pull request, use:
  ```bash
  gh api repos/<owner>/<repo>/pulls/<pr_number>/comments --paginate
  ```
- Never run `git commit` with `--no-verify` unless the user explicitly asks for it.

## Build Workflow
- Use the workspace `target/` directory by default.
- Do not introduce alternate `CARGO_TARGET_DIR` values unless the user explicitly asks for them.
- If the build target is locked by another process, wait for the lock instead of switching to a separate target directory.
