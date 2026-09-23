# Changelog

## [0.6.0](https://github.com/TimSchoenle/terrace-config/compare/terrace-contract-v0.5.0...terrace-contract-v0.6.0) (2026-09-23)


### Features

* **cli:** narrow the chart gates to named charts ([#192](https://github.com/TimSchoenle/terrace-config/issues/192)) ([97772da](https://github.com/TimSchoenle/terrace-config/commit/97772daee08af7b4575fa879017883f47ab6836a))


### Bug Fixes

* **cli:** count render prerequisites a chart value carries into a required map ([#191](https://github.com/TimSchoenle/terrace-config/issues/191)) ([a86180d](https://github.com/TimSchoenle/terrace-config/commit/a86180dc353a2c5079502383956651cde052befc))

## [0.5.0](https://github.com/TimSchoenle/terrace-config/compare/terrace-contract-v0.4.1...terrace-contract-v0.5.0) (2026-09-23)


### Features

* publish and enforce whether a configuration change needs a restart ([#187](https://github.com/TimSchoenle/terrace-config/issues/187)) ([e0fd0de](https://github.com/TimSchoenle/terrace-config/commit/e0fd0de5c4992adecb1162063436b4ca95f7c411))

## [0.4.1](https://github.com/TimSchoenle/terrace-config/compare/terrace-contract-v0.4.0...terrace-contract-v0.4.1) (2026-09-22)


### Bug Fixes

* **cli:** pin the container image by a tag that exists ([#185](https://github.com/TimSchoenle/terrace-config/issues/185)) ([f1825bd](https://github.com/TimSchoenle/terrace-config/commit/f1825bd83f0730b4a46ea5c386608f45fb0c82c6))

## [0.4.0](https://github.com/TimSchoenle/terrace-config/compare/terrace-contract-v0.3.0...terrace-contract-v0.4.0) (2026-09-22)


### Features

* schema refinements — constraints a type cannot state, supplied at schema-build time ([#174](https://github.com/TimSchoenle/terrace-config/issues/174)) ([5812492](https://github.com/TimSchoenle/terrace-config/commit/581249281f8ecb0736827de07d2d5ad192ae6472))


### Bug Fixes

* **deps:** update rust crate saphyr-parser to 0.1.0 ([#177](https://github.com/TimSchoenle/terrace-config/issues/177)) ([b4bf142](https://github.com/TimSchoenle/terrace-config/commit/b4bf1420d3692e4cf3de33eb4a87c44dec6ec58d))

## [0.3.0](https://github.com/TimSchoenle/terrace-config/compare/terrace-contract-v0.2.2...terrace-contract-v0.3.0) (2026-09-22)


### Features

* implement experimental java implementation ([#98](https://github.com/TimSchoenle/terrace-config/issues/98)) ([26ffc70](https://github.com/TimSchoenle/terrace-config/commit/26ffc70d67d286964688f03d651fe51977e5df4f))


### Bug Fixes

* **deps:** update rust crate jsonschema to v0.55.1 ([#127](https://github.com/TimSchoenle/terrace-config/issues/127)) ([8599dbc](https://github.com/TimSchoenle/terrace-config/commit/8599dbc9a0a1a671ae6274855c6295e877df5f6a))
* **deps:** update rust crate jsonschema to v0.56.0 ([#131](https://github.com/TimSchoenle/terrace-config/issues/131)) ([14b5729](https://github.com/TimSchoenle/terrace-config/commit/14b5729ab582f721dae9eab80d3d18fed9ae0484))
* **deps:** update rust crate toml to v1.1.6 ([#144](https://github.com/TimSchoenle/terrace-config/issues/144)) ([f7b403b](https://github.com/TimSchoenle/terrace-config/commit/f7b403b938576c0c1766b02b367bc4e35bfb77cf))


### Miscellaneous

* **deps:** update rust crate toml to v1.1.6 ([#163](https://github.com/TimSchoenle/terrace-config/issues/163)) ([1934d03](https://github.com/TimSchoenle/terrace-config/commit/1934d03aeca21462490649a6ed6de7bf32a52b6d))


### Dependencies

* **deps:** lock file maintenance ([#172](https://github.com/TimSchoenle/terrace-config/issues/172)) ([bb1a406](https://github.com/TimSchoenle/terrace-config/commit/bb1a406dd2be432080730502f6b02a74eaf4d616))

## [0.2.2](https://github.com/TimSchoenle/terrace-config/compare/terrace-contract-v0.2.1...terrace-contract-v0.2.2) (2026-09-10)


### Miscellaneous

* **cli:** re-release as 0.2.2 to publish the image 0.2.1 never got ([#119](https://github.com/TimSchoenle/terrace-config/issues/119)) ([87b2ce8](https://github.com/TimSchoenle/terrace-config/commit/87b2ce84717ca3bb1930b2308c7cd630dce89607))

## [0.2.1](https://github.com/TimSchoenle/terrace-config/compare/terrace-contract-v0.2.0...terrace-contract-v0.2.1) (2026-09-10)


### Bug Fixes

* **deps:** update rust crate sha2 to 0.11 ([#112](https://github.com/TimSchoenle/terrace-config/issues/112)) ([6131467](https://github.com/TimSchoenle/terrace-config/commit/61314675a6d6b54a6ee96ff8290afcb8992f3c29))
* **deps:** update rust crate toml to v1 ([#101](https://github.com/TimSchoenle/terrace-config/issues/101)) ([78bd77f](https://github.com/TimSchoenle/terrace-config/commit/78bd77f872470d20b77f4f00b540f7f449e69e59))

## [0.2.0](https://github.com/TimSchoenle/terrace-config/compare/terrace-contract-v0.1.0...terrace-contract-v0.2.0) (2026-09-10)


### Features

* **cli:** the consumer half — the gates, the markers, the derived documents and the network ([#104](https://github.com/TimSchoenle/terrace-config/issues/104)) ([a6ce4e5](https://github.com/TimSchoenle/terrace-config/commit/a6ce4e5e05d76c5ef190c11b1d45ba5f451e9153))
* one contract toolchain for every implementation ([#96](https://github.com/TimSchoenle/terrace-config/issues/96)) ([59ee6a6](https://github.com/TimSchoenle/terrace-config/commit/59ee6a6c9e49e88bbcc0a37a4d6821652b388bf7))


### Bug Fixes

* **deps:** update rust crate toml to 0.9 ([#100](https://github.com/TimSchoenle/terrace-config/issues/100)) ([0cf063c](https://github.com/TimSchoenle/terrace-config/commit/0cf063c36897a9f15c7843690f579f06ba0c9224))


### Miscellaneous

* **deps:** pin rust crate serde_norway to =0.9.42 ([#109](https://github.com/TimSchoenle/terrace-config/issues/109)) ([ee20865](https://github.com/TimSchoenle/terrace-config/commit/ee20865b4ac9cc4b12ce4bb7630165626531a9d9))
* **deps:** pin rust crate toml to =0.8.23 ([#99](https://github.com/TimSchoenle/terrace-config/issues/99)) ([2f1049e](https://github.com/TimSchoenle/terrace-config/commit/2f1049ef28a888c8a09712f9765d7450f78261d8))
