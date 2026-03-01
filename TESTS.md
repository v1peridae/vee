# reproduce my results

<div align="center">
  <img src="https://files.catbox.moe/8fj85q.png" alt="vee banner" width="600"/>
</div>

specs these were run on: macOS 15.7.3 / apple m4 / 16gb ram

versions: vee 0.1.0, bun 1.2.19, pnpm 10.30.3, npm 11.5.1

you'll need [hyperfine](https://github.com/sharkdp/hyperfine) for tests 3 and 4 (`brew install hyperfine`).

---

Create a fresh directory and add this `package.json` :

```json
{
  "name": "tests",
  "version": "1.0.0",
  "dependencies": {
    "express": "^4.21.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0",
    "axios": "^1.7.0",
    "zod": "^3.23.0",
    "dotenv": "^16.4.0",
    "lodash": "^4.17.21",
    "chalk": "^4.1.2",
    "commander": "^12.1.0",
    "dayjs": "^1.11.13"
  },
  "devDependencies": {
    "typescript": "^5.6.0",
    "@types/react": "^19.0.0",
    "@types/node": "^22.0.0",
    "@types/lodash": "^4.17.0",
    "@types/express": "^4.17.21"
  }
}
```

---

## test 1 : clean install with no cache or lockfile or node_modules

this is network-dependent so numbers may vary.

```bash
# vee
rm -rf node_modules vee.lock && vee cache clean && time vee install

# bun
rm -rf node_modules bun.lock && rm -rf "$(bun pm cache)" && time bun install

# pnpm
rm -rf node_modules pnpm-lock.yaml && pnpm store prune --force && time pnpm install

# npm
rm -rf node_modules package-lock.json && npm cache clean --force && time npm install
```

---

## test 2 : with lockfile, no cache

generate the lockfiles first by running each install once, then:

```bash
# vee
rm -rf node_modules ~/.vee/store && time vee install

# bun
rm -rf node_modules "$(bun pm cache)" && time bun install

# pnpm
rm -rf node_modules ~/Library/pnpm/store/v10 && time pnpm install

# npm
rm -rf node_modules && npm cache clean --force && time npm ci
```

---

## test 3 & 4 : local

generate a lockfile for each and warmup all caches by running each install once:

```bash
vee install && cp vee.lock vee.lock.bak
bun install && cp bun.lock bun.lock.bak
pnpm install && cp pnpm-lock.yaml pnpm-lock.yaml.bak
npm install && cp package-lock.json package-lock.json.bak
```

### test 3 : with cache + lockfile but no node_modules

```bash
hyperfine --warmup 1 --runs 7 \
  --prepare 'rm -rf node_modules; cp vee.lock.bak vee.lock; rm -f bun.lock pnpm-lock.yaml package-lock.json' \
  'vee install' \
  --prepare 'rm -rf node_modules; cp bun.lock.bak bun.lock; rm -f vee.lock pnpm-lock.yaml package-lock.json' \
  'bun install' \
  --prepare 'rm -rf node_modules; cp pnpm-lock.yaml.bak pnpm-lock.yaml; rm -f vee.lock bun.lock package-lock.json' \
  'pnpm install' \
  --prepare 'rm -rf node_modules; cp package-lock.json.bak package-lock.json; rm -f vee.lock bun.lock pnpm-lock.yaml' \
  'npm ci'
```

### test 4 - warm install (second run, nothing changed)

set up separate directories so they don't interfere with each other:

```bash
for pm in vee bun pnpm npm; do mkdir -p /tmp/bench-$pm; cp package.json /tmp/bench-$pm/; done

cd /tmp/bench-vee && cp $OLDPWD/vee.lock.bak vee.lock && vee install > /dev/null
cd /tmp/bench-bun && cp $OLDPWD/bun.lock.bak bun.lock && bun install > /dev/null
cd /tmp/bench-pnpm && cp $OLDPWD/pnpm-lock.yaml.bak pnpm-lock.yaml && pnpm install > /dev/null
cd /tmp/bench-npm && cp $OLDPWD/package-lock.json.bak package-lock.json && npm ci > /dev/null

hyperfine --warmup 2 --runs 10 \
  'cd /tmp/bench-vee && vee install' \
  'cd /tmp/bench-bun && bun install' \
  'cd /tmp/bench-pnpm && pnpm install' \
  'cd /tmp/bench-npm && npm install'
```
