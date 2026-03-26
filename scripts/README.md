# Scripts

## Public Promotion Guard

Use this from a promotion branch that is based on `public/main` after cherry-picking only OSS-safe commits.

### Run Locally

Python 3.11+ is required for the guard and its regression tests because `check_public_promotion.py` uses `tomllib`.

- Fetch the public base ref if needed:
  - `git fetch public main`
- Run the guard:
  - `scripts/check-public-promotion`
- Run the guard regression tests:
  - `python3 -m unittest scripts/tests/test_check_public_promotion.py`

The guard compares the current branch against `public/main`, fails on blocked or non-allowlisted paths, scans added lines for private markers, and checks a few public invariants before you open a public PR.
