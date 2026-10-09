# OAC
The Open Agent Channel is a tool to allow communication between multiple harness

## Checks before a pull request

There is no GitHub-hosted CI. Before opening or merging a pull request, run the checks
client-side on Windows and in WSL, and paste each summary (it names the git HEAD SHA) into
the pull request:

    node scripts/local-ci.mjs

Opt-in tiers run on demand: `node scripts/local-ci.mjs --tier keystore|scale|mutation|g3-macos`.
See `docs/planning/v0.1/09-test-strategy.md` §3.
