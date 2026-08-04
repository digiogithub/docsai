# 26 — This repository is driven by Jujutsu (jj), not by git

Recorded after Phase 11 F, when the "git index anomaly" that had been showing up since 11-C turned
out not to be an anomaly at all: **the working copy is managed by [jj](https://jj-vcs.github.io/),
and every `git` command run against it was fighting the tool that owns the tree.**

## What is actually here

A **colocated** repository: `.jj/` and `.git/` side by side. Git is the storage backend and the
network transport; jj is the interface. Consequences, all of them things that look like breakage
if you assume git:

| What git shows | Why | What it means |
|---|---|---|
| `HEAD` detached, "Current branch: HEAD" | git's `HEAD` tracks `@-`, the **parent** of jj's working-copy commit | Normal. There is no branch to be "on"; jj's working copy is itself a commit. |
| Files staged as `A` with a blob **identical** to `HEAD` | jj rewrites `.git/index` on every snapshot; the index is jj's scratch space, not a staging area | Ignore it. jj has no staging area at all. |
| `git status` disagrees with itself after `git reset` | `git reset`/`git checkout`/`git read-tree` rewrite the index jj is maintaining | Do not run them. |
| `git fsck`: `duplicateEntries` in two trees | old unreachable objects (`corpus` twice, `docx` twice, both with the identical hash) | Cosmetic, unreachable from any bookmark, and not on the current chain. |

## What running `git commit` did to this repository

It worked — and it left a mess behind each time. jj had already snapshotted the tree into its
working-copy commit; the `git commit` created a *second* commit from the index, and jj imported it
as a new change while **abandoning its own working-copy commit as a side head**. `jj log` shows one
per commit made this way, seven of them:

```
○  uxplsvpu  62e8684c  feat(convert,cli,docmark): Phase 11 F — …
│ ○  zsyrvwnx  03a39bd3  (no description set)      ← orphan left by `git commit`
├─╯
○  ovzrklxs  35918526  feat(model,docmark,cli): Phase 11 E — …
│ ○  lnytuzkq  c5286e1f  (no description set)      ← and again
├─╯
```

Worse, and the part that matters: **no bookmark points at any of the Phase 11 work.** `main` is at
`9dd637a` and `feat/phase-10-addressing` at `2aa0b21`; commits `f84e9bb`…`62e8684` (Phase 11 A–F)
sit on an anonymous head, visible to jj and reachable from nothing git-side.

## The rule

**Use `jj`. Never use a `git` command that writes** — `commit`, `add`, `reset`, `checkout`,
`stash`, `rebase`, `merge`, `restore`, `read-tree`. Read-only git (`git log`, `git show`,
`git diff`, `git cat-file`) is fine and sometimes clearer, but jj has an equivalent for each.

The `git stash pop` that "corrupted the index" in 11-C, and the phantom staged entries in 11-D and
11-F, were all the same mistake wearing different clothes.

## Command map

| Intent | git (do not use) | jj |
|---|---|---|
| See what changed | `git status` | `jj status` |
| See history | `git log --oneline` | `jj log` |
| See a diff | `git diff` | `jj diff` |
| Stage something | `git add` | *nothing* — jj snapshots the working copy automatically |
| Commit the work | `git commit -m …` | `jj commit -m …` (describes `@` and starts a new empty `@`) |
| Reword the last commit | `git commit --amend` | `jj describe -r @-` |
| Add to the last commit | `git commit --amend` | just edit the files, then `jj squash` |
| Put work aside | `git stash` | `jj new <rev>` — the old change stays a change, nothing is hidden |
| Name a line of work | `git branch x` / `git checkout -b x` | `jj bookmark create x -r @-` |
| Move the branch pointer | `git branch -f x` | `jj bookmark set x -r @-` |
| Push | `git push origin x` | `jj git push --bookmark x` |
| Fetch | `git fetch` | `jj git fetch` |
| Undo the last operation | — | `jj undo` (jj records an operation log; nothing is lost) |

Two jj facts that remove most of the reasons to reach for git:

- **The working copy is a commit.** `@` is always a real change; there is no dirty/clean state to
  manage and nothing to stash.
- **`jj undo` exists for everything.** Any operation can be reverted from the op log
  (`jj op log`), which is why jj needs no equivalent of `git reflog` gymnastics.

## What still has to be repaired

Not done here, because it rewrites what is already committed and that is the repository owner's
call:

1. ~~The Phase 11 chain needs a bookmark~~ **Done.** `main` was moved onto it with
   `jj bookmark set main -r @-` — a pure fast-forward, since the old `main` (`9dd637a`) was a
   direct ancestor. `main@origin` is still at `9dd637a`, seven commits behind; pushing is a
   separate, outward-facing decision.
2. ~~The side heads~~ **Done.** All nine were duplicates and were abandoned. They were verified
   by **tree hash**, not by eyeballing the diff: each one's tree is byte-identical to a commit
   already on `main` (`wonsutnx` and `f84e9bb` both point at tree `51f10f57`, `wzmyyqvr` and
   `82cfb96` at `fd9aee9e`, and so on). Nothing was merged because there was nothing to merge.

   **The trap: two of them carried a description that had nothing to do with their content** —
   `35f0a4c` said *"feat: new mcp tools"* and `30d2d0f2` said *"feat: pptx format
   implementation"*, while holding the Phase 11 A raw-sidecar work and the Phase 11 D dictionary
   respectively. A jj working-copy commit can be described *before* it is finished; when
   `git commit` then took the content out from under it, the stale description stayed on the
   abandoned twin. So on a repository that has been driven with git by mistake, **a side head's
   description is not evidence of anything** — compare `git rev-parse <rev>^{tree}` against the
   trees on the branch and let the hashes answer.

The rules are carried into every session by `.claude/skills/jj-vcs/SKILL.md` and by AGENTS.md §4.1.
