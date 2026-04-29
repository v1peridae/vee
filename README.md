# vee

<div align="center">
  <img src="https://files.catbox.moe/9t711w.png" alt="vee banner" width="800"/>
</div>
<br>

a fast rust-based package manager for javascript/node! :3

vee is currently available only on linux and macos!

i made it because i got into an interesting disagreement with my friends (@meghanam4, @neongamerbot-qk, @froppii and @tanmayrajk) about which is the best package manager and they kept saying pnpm but i do NOT like pnpm (we have our differences </3) so i made my own so they can use this instead of pnpm.

---

## installation (macos + linux)

### option 1: install from release (recommended)

```bash
curl -sSL https://raw.githubusercontent.com/v1peridae/vee/main/install.sh | sh
```

### option 2: build from source!

```bash
git clone https://github.com/v1peridae/vee
cargo install --path vee/vee
```

---

## quick start example

```bash
vee create vite my-app
cd my-app
vee install
vee run dev
```

---

## usage

```bash
# global flags
vee -v, --verbose             # verbose logs
vee -S, --simulate            # prints what it would do

# project + dependencies
vee install                   # install dependencies (alias - vee i)
vee install -P, --production  # install production dependencies only
vee install --frozen-lockfile # error if vee.lock missing/outdated
vee install --ignore-scripts  # skip package lifecycle scripts

vee add <pkg>                 # add dependency
vee add -D, --dev <pkg>       # add dev dependency
vee add <pkg>@<version>       # pin version (supported way)

vee remove <pkg>              # remove dependency
vee update [pkgs...]          # update all dependencies, or only named ones

# scripts / running
vee run <script> [-- args...] # run a package.json script
vee <script> [-- args...]     # shorthand for `vee run <script>`
vee run <file.js> [-- args...]# run a JS file directly (if it exists)

# scaffolding
vee init [-y, --yes]          # create package.json
vee create <name> [args...]   # runs create-<name> (supports <name>@<ver> too)

# one-off executables
vee exec <pkg> [args...]      # run a package binary without installing (alias - vee dlx)
vee exec <pkg>@<ver> [args...]# versioned exec

# inspection
vee list [--prod] [--dev]     # list direct dependencies (alias - vee ls)
vee list --tree               # show dependency tree
vee outdated                  # show outdated dependencies

# cache
vee cache clean               # clear ~/.vee cache
vee cache info                # show cache size + location
```

---

## what do you mean fast???

specs: macOS 15.7.3 / apple m4 / 16gb ram  
versions: vee 0.1.1, bun 1.2.19, pnpm 10.30.3, npm 11.5.1  
110 packages (see TESTS.md)

note:tests 1 + 2 depend on your network speed so they may vary

| test                                                        | vee   | bun   | pnpm  | npm   |
| ----------------------------------------------------------- | ----- | ----- | ----- | ----- |
| 1. clean install - no cache, no lockfile, no `node_modules` | ~9.5s | ~5.4s | ~7.4s | ~32s  |
| 2. with lockfile, no cache                                  | ~3.6s | ~3.2s | ~3.5s | ~3.4s |

note: tests 3 + 4 are local-only, run with `hyperfine` (7–10 runs, mean ± standard deviation):

| test                                           | vee               | bun           | pnpm         | npm           |
| ---------------------------------------------- | ----------------- | ------------- | ------------ | ------------- |
| 3. with cache + lockfile, no `node_modules`    | **44ms** ± 4ms    | 51ms ± 17ms   | 491ms ± 32ms | 1.01s ± 0.02s |
| 4. warm install (everything already installed) | **3.0ms** ± 0.2ms | 6.6ms ± 0.4ms | 202ms ± 17ms | 1.02s ± 0.02s |

tldr; for the cold installs bun is faster but once the cache is warm, vee is the fastest (2x bun, ~68x pnpm, ~340x npm on warm installs)

wanna reproduce this? check out TESTS.md

---

## compatibility

vee works with your `package.json`, uses the npm registry (configurable via `.npmrc`) + produces a `node_modules` layout with symlinks that node will understand. so your existing vite, next or whatever should work (˶ᵔ ᵕ ᵔ˶)

---

## license

MIT - do whatever you want

---

## ai usage disclaimer while coding (it's good to be transparent and honest!)

this was one of my most complex projects and i got some help from claude's sonnet and opus 4.6 especially with testing + fixing a shit load of unompimised code and fixing my install.sh code bc it just wouldn't work when i did it :')

---

made with :3 and <3 by @v1peridae ⸜(｡˃ ᵕ ˂ )⸝♡
