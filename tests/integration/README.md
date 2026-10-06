# tests/integration/

Fake-endpoint and opt-in live-provider integration tests
(`docs/planning/v0.1/07-repository-and-dependencies.md` §2, `tests/` row;
`docs/planning/v0.1/09-test-strategy.md`).

Empty at the scaffold (#50). This directory is a leaf: it is not a workspace member, and no
product crate or root manifest may pull it into a default build
(`scripts/check-herdr-containment.mjs`, check 9).
