# 0002: Cohesive product interface

Status: Accepted design constraints, not a claim that every future screen exists.

## Decision

Extend the existing full interface and navigation structure. Avoid piecemeal
redesigns or hiding the main workflow behind accordions. Keep labels concise,
content readable, keyboard access usable and mobile layouts practical.

- Use dark by default with Nord/purple as the baseline; retain light appearance
  support and existing appearance choices.
- Give individual projects their own navigation and directly visible Code/files.
  Create/manage repositories through the platform; manual server checkout
  attachment is an optional import/inspection path. Repository names are scoped
  to projects, so separate projects can each contain a repository named `ui`.
- Show the user's profile display name and navigate into the workspace after
  successful sign-in. Avoid unnecessary About, in-development or local-workspace
  labels in the interface.
- Initial owner enrollment uses the local/SSH CLI; browser enrollment is deferred.
  [Setup documentation](../local-enrollment.md) owns the detailed flow and limits.

## Consequences

Reuse existing components and page structure before introducing new interaction
patterns. The [user guide](../using-devoption.md) owns current behavior; the
[gallery](../screenshots/gallery.md) provides visual reference. Follow
[contribution policy](../../CONTRIBUTING.md) for the concise README, screenshot
limits and required capture updates.
