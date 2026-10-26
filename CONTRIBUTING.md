# Contribution Guidelines

## Commit discipline

This project follows Zulip's [commit discipline].

[commit discipline]: https://zulip.readthedocs.io/en/12.1/contributing/commit-discipline.html

### Submitting a pull request

To submit a pull request, follow Zulip's suggestions for [reviewable PRs].

[reviewable PRs]: https://zulip.readthedocs.io/en/12.1/contributing/reviewable-prs.html

## AI Coding Assistants

This section provides guidance for AI tools and developers using AI
assistance when contributing to the Hermit kernel. It is adapted from
the Linux document on [AI Coding Assistants].

AI tools helping with Hermit kernel development should follow the standard
kernel development process.

[AI Coding Assistants]: https://docs.kernel.org/process/coding-assistants.html

### Licensing and Legal Requirements

All contributions must comply with the kernel's licensing requirements:

- All code must be compatible with `MIT OR Apache-2.0`

### Co-authored-by and Developer Responsibility

AI agents MUST NOT add Co-authored-by tags. The human submitter
is responsible for:

- Reviewing all AI-generated code
- Ensuring compliance with licensing requirements
- Taking full responsibility for the contribution

### Attribution

When AI tools contribute to kernel development, proper attribution
helps track the evolving role of AI in the development process.
Contributions should include an Assisted-by tag in the following format:

```
Assisted-by: AGENT_NAME:MODEL_VERSION [TOOL1] [TOOL2]
```

Where:

- `AGENT_NAME` is the name of the AI tool or framework
- `MODEL_VERSION` is the specific model version used
- `[TOOL1] [TOOL2]` are optional specialized analysis tools used
  (e.g., coccinelle, sparse, smatch, clang-tidy)

Basic development tools (git, gcc, make, editors) should not be listed.

Example:

```
Assisted-by: Claude:claude-3-opus coccinelle sparse
```

### Code Comments

<!--- Adapted from https://zulip.readthedocs.io/en/12.1/contributing/contributing.html#using-ai-as-a-coding-assistant -->

Don’t simply ask an LLM to add code comments, as it will likely produce
a bunch of text that unnecessarily explains what’s already clear from the
code. If using an LLM to generate comments, be really specific in your
request, demand succinctness, and carefully edit the result.
