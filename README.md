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

## quick start

```bash
vee init -y && vee add react
```

---

## usage

```bash
vee install                 # install deps (you can also use: vee i)
vee add [package]           # add a package
vee add -D [package]        # add a dev dependency
vee add [package]@[version] # add a specific version (or use --version)
vee remove [package]        # remove a package
vee update [packages]       # update deps
vee run [script]            # run a script from package.json
vee [script]                # short for vee run
vee init                    # scaffold a new project
vee create [package] [args] # create a project from a template (vee create vite my-app)
vee exec [package] [args]   # run a package w/o installing (you can also use: vee dlx)
vee list --tree             # show the dependency tree
vee list --prod             # list production deps only
vee list --dev              # list dev deps only
vee outdated                # check for outdated packages
vee cache clean             # clear the cache
vee cache info              # show cache size and location
```

---

## what do you mean fast???

specs: macOS 15.7.3 / apple m4 / 16gb ram  
versions: vee 0.1.0, bun 1.2.19, pnpm 10.30.3, npm 11.5.1  
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

vee works with your `package.json`, uses the npm registry (and mirrors) + produces a `node_modules` layout with symlinks that node will understand. so your existing vite, next or whatever should work (˶ᵔ ᵕ ᵔ˶)

---

## license

MIT - do whatever you want

---

## ai usage disclaimer while coding (it's good to be transparent and honest!)

this was one of my most complex projects and i got some help from claude's sonnet and opus 4.6 especially with testing + fixing a shit load of unompimised code and fixing my install.sh code bc it just wouldn't work when i did it :')

---

made with :3 and <3 by @v1peridae ⸜(｡˃ ᵕ ˂ )⸝♡
