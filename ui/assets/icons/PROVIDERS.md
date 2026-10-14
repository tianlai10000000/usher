# Repository provider marks

The project-card identity assets live in `ui/assets/provider-ribbons/`:

- `provider-ribbon-github-full.svg` and `provider-ribbon-github-compact.svg`
- `provider-ribbon-gitlab-full.svg` and `provider-ribbon-gitlab-compact.svg`
- `provider-ribbon-gitee-full.svg` and `provider-ribbon-gitee-compact.svg`

Each application-owned SVG bundles its provider mark, band, and caption into
fixed geometry. `GitHubProjectProviderRibbon` in
`ui/components/project/github_project_components.slint` selects the full or
compact asset. These are provider identity assets, not application command
icons, and are therefore outside the Octicon command-icon rule.

- GitHub mark: GitHub brand resources
- GitLab mark: GitLab brand resources
- Gitee mark: Gitee brand resources

These assets may only be used to identify their corresponding provider. They
must not be reused as generic controls, status symbols, or navigation icons.
