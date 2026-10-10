# Working in DevOption

- Open or identify a scoped GitHub issue before any work, including documentation
  and maintenance. Follow `CONTRIBUTING.md` and link every commit and PR to it.
- Keep changes small and fresh. Do not import code, configuration, or documentation
  from private prototypes. Preserve existing user work.
- Use atomic Conventional Commits on topic branches and draft PRs. The initial
  bootstrap is the only direct push to `main`; never force-push shared history.
- Keep implemented behavior separate from roadmap claims. Add crates only when
  a real implementation needs them; this is one Rust monorepo.
- Run the relevant README checks and review the public diff for private data.
  Keep the changelog and tested instructions accurate.
- Do not create credentials, enable paid services, change repository security
  settings, or publish releases/packages without separate explicit authorization.
