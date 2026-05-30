# Project Memory - mikrom-rust

## GitHub Operations
- **Review PR Comments:** To view detailed line-by-line review comments for a pull request, use:
  ```bash
  gh pr view <pr_number> --json reviews -q '.reviews[] | .comments[] | {path: .path, line: .line, body: .body}'
  ```
