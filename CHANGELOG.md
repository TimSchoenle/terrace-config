# Changelog

## Unreleased

### Features

* **schema:** element schemas for container-typed keys. `#[config(element)]` and
  `#[config(element_values)]` report what one element of a `Vec`, `VecDeque`, `HashSet`,
  `BTreeSet`, `HashMap`, `BTreeMap` or array holds, and `Key::constraint` carries it nested —
  `items` for a sequence, `additionalProperties` for a map, composed through both when they are
  stacked, so `HashMap<String, HashSet<Method>>` reaches the enum. `Schema::to_json_schema`
  renders the same shape at the key's position. `Sink::repeated` is the hand-written equivalent.

### What a consumer needs to know

* `SCHEMA_VERSION` is now **2**. Nothing was removed and nothing changed meaning, so a version-1
  consumer that ignores what it does not recognise needs no change, and one that hands
  `constraint` to a JSON Schema validator gets stricter for free. The bump is for the consumer
  that walks the keywords itself: widen the allowlist to `items`, `additionalProperties`,
  `properties`, `required`, `uniqueItems`, `minItems`, `maxItems` and `description`, and recurse
  rather than assume scalars. `CONTRACT_VERSION` is unchanged — the envelope did not move.
* **No new keys.** An element has no path, so `Schema::keys` gains nothing and every
  environment-variable gate reads exactly what it read before.
* **Opt-in.** A container whose element type does not describe itself emits the bytes it always
  did, and `text_form` and `text_constraint` are untouched for every key: a container is still
  supplied through the environment as one TOML literal.

## [0.9.2](https://github.com/TimSchoenle/terrace-config/compare/v0.9.1...v0.9.2) (2026-09-01)


### Bug Fixes

* **loader:** withhold the loader's own variables from the environment layer ([#69](https://github.com/TimSchoenle/terrace-config/issues/69)) ([850cb66](https://github.com/TimSchoenle/terrace-config/commit/850cb661543235113d95334b8c395d0ebb5e1430))


### Miscellaneous

* **deps:** update taiki-e/install-action action to v2.86.7 ([#60](https://github.com/TimSchoenle/terrace-config/issues/60)) ([d492ed1](https://github.com/TimSchoenle/terrace-config/commit/d492ed1f01aec03de5ae539d12dea38bb40abca8))
* **deps:** update taiki-e/install-action action to v2.86.8 ([#62](https://github.com/TimSchoenle/terrace-config/issues/62)) ([3611190](https://github.com/TimSchoenle/terrace-config/commit/36111905991e2c80e5fb2abf298b7dc434801dbc))
* **deps:** update taiki-e/install-action action to v2.87.0 ([#67](https://github.com/TimSchoenle/terrace-config/issues/67)) ([2c707f7](https://github.com/TimSchoenle/terrace-config/commit/2c707f7d41ecfaa207bc629d38f50a1d00d69a6a))
* **deps:** update taiki-e/install-action action to v2.87.1 ([#70](https://github.com/TimSchoenle/terrace-config/issues/70)) ([78f25ec](https://github.com/TimSchoenle/terrace-config/commit/78f25ec1e96eac2fed089dd49e3f12ae23246a4f))
* **deps:** update timschoenle/actions/.github/workflows/maintenance-timed-auto-pr-approve.yaml to vworkflows-maintenance-timed-auto-pr-approve-v1.2.33 ([#64](https://github.com/TimSchoenle/terrace-config/issues/64)) ([492f411](https://github.com/TimSchoenle/terrace-config/commit/492f4117fbca843915b6ec5e6b5b08cbcb56288d))
* **deps:** update timschoenle/actions/actions/common/commit-changes to vactions-common-commit-changes-v1.4.0 ([#68](https://github.com/TimSchoenle/terrace-config/issues/68)) ([c7451b4](https://github.com/TimSchoenle/terrace-config/commit/c7451b40ad0e200e5d5cb715c6d43878f697eafb))
* **deps:** update timschoenle/actions/actions/common/readme-variables to vactions-common-readme-variables-v1.1.1 ([#65](https://github.com/TimSchoenle/terrace-config/issues/65)) ([57a97c1](https://github.com/TimSchoenle/terrace-config/commit/57a97c15cc247ec0bbb1715f60fa15bc25b254c5))
* **deps:** update timschoenle/actions/actions/common/render-template to vactions-common-render-template-v1.1.2 ([#66](https://github.com/TimSchoenle/terrace-config/issues/66)) ([87a6c7f](https://github.com/TimSchoenle/terrace-config/commit/87a6c7f484f8d23510eb6dc96f74d7a95c265937))
* **deps:** update timschoenle/actions/actions/common/render-template-and-commit to vactions-common-render-template-and-commit-v1.1.4 ([#63](https://github.com/TimSchoenle/terrace-config/issues/63)) ([3abfe95](https://github.com/TimSchoenle/terrace-config/commit/3abfe950fe97d02206138977406d3db4cc74365e))

## [0.9.1](https://github.com/TimSchoenle/terrace-config/compare/v0.9.0...v0.9.1) (2026-08-26)


### Bug Fixes

* **schema:** print exactly one trailing newline from Cli::main ([#48](https://github.com/TimSchoenle/terrace-config/issues/48)) ([1a87714](https://github.com/TimSchoenle/terrace-config/commit/1a8771465096da1688ea2f93c59afa70e765c8b6))


### Miscellaneous

* **deps:** update taiki-e/install-action action to v2.86.2 ([#50](https://github.com/TimSchoenle/terrace-config/issues/50)) ([4128975](https://github.com/TimSchoenle/terrace-config/commit/41289758bde574b71d16c4d104c26c0fcd93e049))
* **deps:** update taiki-e/install-action action to v2.86.3 ([#53](https://github.com/TimSchoenle/terrace-config/issues/53)) ([0e7f918](https://github.com/TimSchoenle/terrace-config/commit/0e7f9189a0b32992ef7791f9283bf9d1a77f00aa))
* **deps:** update taiki-e/install-action action to v2.86.4 ([#54](https://github.com/TimSchoenle/terrace-config/issues/54)) ([9733a80](https://github.com/TimSchoenle/terrace-config/commit/9733a801e78480a52b57c5f41439967f9eaf226a))
* **deps:** update taiki-e/install-action action to v2.86.5 ([#57](https://github.com/TimSchoenle/terrace-config/issues/57)) ([64be52d](https://github.com/TimSchoenle/terrace-config/commit/64be52dcc551695200300793dd394537fda10c0a))
* **deps:** update taiki-e/install-action action to v2.86.6 ([#59](https://github.com/TimSchoenle/terrace-config/issues/59)) ([0bef463](https://github.com/TimSchoenle/terrace-config/commit/0bef4635dd9d3908bb08ecc40d9af858879395e4))
* **deps:** update timschoenle/actions/.github/workflows/maintenance-auto-approve-renovate.yaml to vworkflows-maintenance-auto-approve-renovate-v1.4.21 ([#51](https://github.com/TimSchoenle/terrace-config/issues/51)) ([4c89386](https://github.com/TimSchoenle/terrace-config/commit/4c89386ffa9f71dbe925ed055545cebed0db8fb9))
* **deps:** update timschoenle/actions/.github/workflows/maintenance-timed-auto-pr-approve.yaml to vworkflows-maintenance-timed-auto-pr-approve-v1.2.32 ([#52](https://github.com/TimSchoenle/terrace-config/issues/52)) ([8d0868a](https://github.com/TimSchoenle/terrace-config/commit/8d0868a91afa479286842f137f2013f87f9885ed))
* **deps:** update timschoenle/actions/actions/common/commit-changes to vactions-common-commit-changes-v1.3.3 ([#58](https://github.com/TimSchoenle/terrace-config/issues/58)) ([8dc824f](https://github.com/TimSchoenle/terrace-config/commit/8dc824f1e5943b19d3a9c7e576ab183dad96ecfd))

## [0.9.0](https://github.com/TimSchoenle/terrace-config/compare/v0.8.0...v0.9.0) (2026-08-19)


### Features

* **schema:** finish the second layer, and the third markdown rendering ([#46](https://github.com/TimSchoenle/terrace-config/issues/46)) ([3cf04fa](https://github.com/TimSchoenle/terrace-config/commit/3cf04fa3a5ed6fc5c9b7621470f151440228278a))

## [0.8.0](https://github.com/TimSchoenle/terrace-config/compare/v0.7.0...v0.8.0) (2026-08-19)


### Features

* **schema:** give the generator the two renderings its consumers still hand-rolled ([#44](https://github.com/TimSchoenle/terrace-config/issues/44)) ([8ba68e5](https://github.com/TimSchoenle/terrace-config/commit/8ba68e58dd9522f3440488352acead22d47cd0bd))

## [0.7.0](https://github.com/TimSchoenle/terrace-config/compare/v0.6.0...v0.7.0) (2026-08-19)


### Features

* **schema:** ship the generator every consumer was writing by hand ([#43](https://github.com/TimSchoenle/terrace-config/issues/43)) ([ca21526](https://github.com/TimSchoenle/terrace-config/commit/ca21526cdcc7b1e0644d18dbc0052e5fa84d2871))


### Miscellaneous

* **deps:** update taiki-e/install-action action to v2.86.1 ([#35](https://github.com/TimSchoenle/terrace-config/issues/35)) ([97ed80a](https://github.com/TimSchoenle/terrace-config/commit/97ed80a47d0ec7dd339db125b6a387f80555cf34))
* **deps:** update timschoenle/actions/actions/common/commit-changes to vactions-common-commit-changes-v1.3.2 ([#37](https://github.com/TimSchoenle/terrace-config/issues/37)) ([bdebfab](https://github.com/TimSchoenle/terrace-config/commit/bdebfab2c28df79c8937716053c8a853f84ecca9))

## [0.6.0](https://github.com/TimSchoenle/terrace-config/compare/v0.5.0...v0.6.0) (2026-08-18)


### Features

* **schema:** publish a config contract with the image ([#31](https://github.com/TimSchoenle/terrace-config/issues/31)) ([3074f1a](https://github.com/TimSchoenle/terrace-config/commit/3074f1aa9edce441d5cf851937afbea250c2f919))


### Miscellaneous

* **deps:** update taiki-e/install-action action to v2.85.14 ([#33](https://github.com/TimSchoenle/terrace-config/issues/33)) ([0070103](https://github.com/TimSchoenle/terrace-config/commit/0070103cd44304093291bce4dfcfb598ca60dac1))
* **deps:** update taiki-e/install-action action to v2.86.0 ([#34](https://github.com/TimSchoenle/terrace-config/issues/34)) ([c43263a](https://github.com/TimSchoenle/terrace-config/commit/c43263ab8d3d8b387f00c609245fd09f5f2d66fd))

## [0.5.0](https://github.com/TimSchoenle/terrace-config/compare/v0.4.0...v0.5.0) (2026-08-18)


### Features

* add example config generator ([#30](https://github.com/TimSchoenle/terrace-config/issues/30)) ([b1056c4](https://github.com/TimSchoenle/terrace-config/commit/b1056c457e96f7c7611ef54c2ccfc05b8d90eff8))
* **explain:** report which layer supplied each key ([#28](https://github.com/TimSchoenle/terrace-config/issues/28)) ([690056f](https://github.com/TimSchoenle/terrace-config/commit/690056f8c07d7556f207ffd877373099c805f361))
* **testing:** add a test harness for consuming projects ([#29](https://github.com/TimSchoenle/terrace-config/issues/29)) ([4643e38](https://github.com/TimSchoenle/terrace-config/commit/4643e385a38609529050157fc8eafd7d31ec92bd))


### Miscellaneous

* **deps:** update step-security/harden-runner action to v2.21.0 ([#26](https://github.com/TimSchoenle/terrace-config/issues/26)) ([379a84f](https://github.com/TimSchoenle/terrace-config/commit/379a84f8df318f584735865a49ac4380bfb37dce))

## [0.4.0](https://github.com/TimSchoenle/terrace-config/compare/v0.3.0...v0.4.0) (2026-08-18)


### Features

* **schema:** render the Markdown tables for reading ([#24](https://github.com/TimSchoenle/terrace-config/issues/24)) ([cb9e15e](https://github.com/TimSchoenle/terrace-config/commit/cb9e15ecf709a70918dcbecf7448fc6a44bd6231))

## [0.3.0](https://github.com/TimSchoenle/terrace-config/compare/v0.2.0...v0.3.0) (2026-08-17)


### Features

* add schema crate to automate config automation ([#23](https://github.com/TimSchoenle/terrace-config/issues/23)) ([e8bda9f](https://github.com/TimSchoenle/terrace-config/commit/e8bda9f8d62eebf833cf71f7cf87b5cb6051713c))


### Miscellaneous

* **deps:** update taiki-e/install-action action to v2.85.10 ([#12](https://github.com/TimSchoenle/terrace-config/issues/12)) ([df82fb9](https://github.com/TimSchoenle/terrace-config/commit/df82fb9ca21ee9122b3888d85bb8d6fc6e387bb7))
* **deps:** update taiki-e/install-action action to v2.85.11 ([#15](https://github.com/TimSchoenle/terrace-config/issues/15)) ([61d89e5](https://github.com/TimSchoenle/terrace-config/commit/61d89e54b0b5564298dc466a59f72e70653072b4))
* **deps:** update taiki-e/install-action action to v2.85.12 ([#21](https://github.com/TimSchoenle/terrace-config/issues/21)) ([71b049c](https://github.com/TimSchoenle/terrace-config/commit/71b049cb49e7ca914cba59c09070f24143cbb394))
* **deps:** update taiki-e/install-action action to v2.85.13 ([#22](https://github.com/TimSchoenle/terrace-config/issues/22)) ([d2029b0](https://github.com/TimSchoenle/terrace-config/commit/d2029b014b21874b27d03737646614705fa011f5))
* **deps:** update taiki-e/install-action action to v2.85.9 ([#10](https://github.com/TimSchoenle/terrace-config/issues/10)) ([62c0755](https://github.com/TimSchoenle/terrace-config/commit/62c07553267a72453abab36b67dffc5f37040d0b))
* **deps:** update timschoenle/actions/.github/workflows/maintenance-auto-approve-renovate.yaml to vworkflows-maintenance-auto-approve-renovate-v1.4.19 ([#13](https://github.com/TimSchoenle/terrace-config/issues/13)) ([7afd126](https://github.com/TimSchoenle/terrace-config/commit/7afd126ff4eb4ff7de00f2549d15f70085baa75a))
* **deps:** update timschoenle/actions/.github/workflows/maintenance-auto-approve-renovate.yaml to vworkflows-maintenance-auto-approve-renovate-v1.4.20 ([#19](https://github.com/TimSchoenle/terrace-config/issues/19)) ([c14f9bb](https://github.com/TimSchoenle/terrace-config/commit/c14f9bb35ec18b19fb6a761c5132bcd521f6a247))
* **deps:** update timschoenle/actions/.github/workflows/maintenance-timed-auto-pr-approve.yaml to vworkflows-maintenance-timed-auto-pr-approve-v1.2.30 ([#14](https://github.com/TimSchoenle/terrace-config/issues/14)) ([b900d1a](https://github.com/TimSchoenle/terrace-config/commit/b900d1a81180672abf2d84f7ee38a0eb4538e9a0))
* **deps:** update timschoenle/actions/.github/workflows/maintenance-timed-auto-pr-approve.yaml to vworkflows-maintenance-timed-auto-pr-approve-v1.2.31 ([#20](https://github.com/TimSchoenle/terrace-config/issues/20)) ([d87dcb2](https://github.com/TimSchoenle/terrace-config/commit/d87dcb243ddab7d71662f41a6355cd1c46deda99))
* **deps:** update timschoenle/actions/actions/common/commit-changes to vactions-common-commit-changes-v1.3.1 ([#16](https://github.com/TimSchoenle/terrace-config/issues/16)) ([4fb329e](https://github.com/TimSchoenle/terrace-config/commit/4fb329efd1adbb5e11c804207b9e0a2a870d33d5))
* **deps:** update timschoenle/actions/actions/common/render-template to vactions-common-render-template-v1.1.1 ([#17](https://github.com/TimSchoenle/terrace-config/issues/17)) ([96902b4](https://github.com/TimSchoenle/terrace-config/commit/96902b4552690c6342ecf60c2f808e7605bded14))
* **deps:** update timschoenle/actions/actions/common/render-template-and-commit to vactions-common-render-template-and-commit-v1.1.3 ([#18](https://github.com/TimSchoenle/terrace-config/issues/18)) ([b7ffa03](https://github.com/TimSchoenle/terrace-config/commit/b7ffa0322675b080da4aa46723ff73f3f4de97bd))

## [0.2.0](https://github.com/TimSchoenle/terrace-config/compare/v0.1.0...v0.2.0) (2026-08-08)


### Features

* add config prototype ([#2](https://github.com/TimSchoenle/terrace-config/issues/2)) ([fd72402](https://github.com/TimSchoenle/terrace-config/commit/fd724026c6a75d05669eaacd6a9a412f9713b9db))


### Bug Fixes

* release please failing to create release ([b6b673e](https://github.com/TimSchoenle/terrace-config/commit/b6b673e1dcc71a88fe63fa97f0e4d07fd070c3fd))


### Miscellaneous

* **deps:** pin dependencies ([#3](https://github.com/TimSchoenle/terrace-config/issues/3)) ([58c5afa](https://github.com/TimSchoenle/terrace-config/commit/58c5afa980d0be618625abcee93e545ac1f1cdc4))
* **deps:** update taiki-e/install-action action to v2.85.8 ([#6](https://github.com/TimSchoenle/terrace-config/issues/6)) ([5bf854b](https://github.com/TimSchoenle/terrace-config/commit/5bf854bee3ee6a92d78425025b1fe6b43f3a1d5a))
* release main ([#8](https://github.com/TimSchoenle/terrace-config/issues/8)) ([c2d510a](https://github.com/TimSchoenle/terrace-config/commit/c2d510ad061431c8964cf32d3f280003a91bd29c))

## 0.1.0 (2026-08-08)


### Features

* add config prototype ([#2](https://github.com/TimSchoenle/terrace-config/issues/2)) ([fd72402](https://github.com/TimSchoenle/terrace-config/commit/fd724026c6a75d05669eaacd6a9a412f9713b9db))


### Miscellaneous

* **deps:** pin dependencies ([#3](https://github.com/TimSchoenle/terrace-config/issues/3)) ([58c5afa](https://github.com/TimSchoenle/terrace-config/commit/58c5afa980d0be618625abcee93e545ac1f1cdc4))
* **deps:** update taiki-e/install-action action to v2.85.8 ([#6](https://github.com/TimSchoenle/terrace-config/issues/6)) ([5bf854b](https://github.com/TimSchoenle/terrace-config/commit/5bf854bee3ee6a92d78425025b1fe6b43f3a1d5a))
