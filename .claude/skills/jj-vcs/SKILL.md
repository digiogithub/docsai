---
name: jj-vcs
description: Version control for this repository, which is a colocated Jujutsu (jj) repo. Use for ANY version-control action here — committing, branching, pushing, inspecting history, undoing, putting work aside — and whenever git reports something odd (detached HEAD, phantom staged files, a confusing index). Never run a git command that writes.
---

# Version control here is jj, not git

`.jj/` and `.git/` sit side by side: git is the storage backend and the network transport,
**jj is the interface**. Read `kb/26-jj-vcs.md` for how this repository got burned by ignoring it.

## The one rule

**Never run a `git` command that writes.** Forbidden: `git commit`, `add`, `reset`, `checkout`,
`switch`, `restore`, `stash`, `rebase`, `merge`, `cherry-pick`, `read-tree`, `update-index`,
`push`, `pull`.

Read-only git is allowed when it is genuinely clearer — `git log`, `git show`, `git diff`,
`git cat-file` — but jj has an equivalent for each, so prefer jj.

## Two things that look broken and are not

Do not "fix" these. Fixing them is what breaks the repository.

1. **`HEAD` is detached** ("Current branch: HEAD"). Git's `HEAD` tracks `@-`, the parent of jj's
   working-copy commit. jj's working copy *is* a commit, so there is no branch to be on.
2. **`git status` shows files staged (`A`) whose blob equals `HEAD`.** jj rewrites `.git/index`
   on every snapshot. jj has no staging area; neither does this repository.

If `git status` output looks wrong, the answer is always `jj status`, never `git reset`.

## Command map

| Intent | jj |
|---|---|
| What changed | `jj status` |
| History | `jj log` (add `-r 'all()'` to see every head) |
| The current diff | `jj diff` (`jj diff -r <rev>` for another change) |
| Stage something | nothing to do — jj snapshots the working copy automatically |
| Commit the work | `jj commit -m "…"` — describes `@` and starts a new empty `@` |
| Reword the last commit | `jj describe -r @- -m "…"` |
| Add more to the last commit | edit the files, then `jj squash` |
| Put work aside | `jj new <rev>` — the change stays a change, nothing is hidden |
| Name a line of work (a branch) | `jj bookmark create <name> -r @-` |
| Move that name | `jj bookmark set <name> -r @-` |
| List branches | `jj bookmark list` |
| Push | `jj git push --bookmark <name>` (first push of a new one: `--allow-new`) |
| Fetch | `jj git fetch` |
| Undo the last operation | `jj undo`; the history of operations is `jj op log` |
| Drop a change | `jj abandon -r <rev>` |

A long commit message goes through a file or a heredoc, exactly as with git:

```bash
jj commit -m "$(cat <<'EOF'
feat(scope): subject line

Body.
EOF
)"
```

## What committing looks like

```bash
jj status                       # confirm what the change contains
jj diff --stat                  # review it
jj commit -m "feat(scope): …"   # describe `@`, open a fresh empty `@`
jj log --limit 3                # confirm
```

There is no `git add` step and no amend dance: the working copy was already the commit.

## Branch and push

`jj` bookmarks are git branches. A chain of commits with no bookmark on it is invisible to git
and unpushable, which is exactly the state this repository's Phase 11 work ended up in.

```bash
jj bookmark create feat/my-work -r @-
jj git push --bookmark feat/my-work --allow-new
```

Push only to the bookmark you were told to push to; never to `main` (AGENTS.md §7 rule 7).

## When something goes wrong

`jj undo` reverts the last operation, whatever it was — commit, rebase, abandon, bookmark move.
`jj op log` lists every operation, and `jj op restore <id>` goes back to any of them. Nothing is
lost and there is no reflog archaeology to do, so **never** reach for a git recovery command.
