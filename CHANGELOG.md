# Changelog

## [1.3.1](https://github.com/Martin-Rancourt/spl-asdlc/compare/v1.3.0...v1.3.1) (2026-10-20)


### Bug Fixes

* **bridge:** a PR held for an incomplete contract is said on its ticket too (SID-1222) ([#94](https://github.com/Martin-Rancourt/spl-asdlc/issues/94)) ([c42e66e](https://github.com/Martin-Rancourt/spl-asdlc/commit/c42e66ead9677868fa983f6d3de96e4a4e72fb2f))

## [1.3.0](https://github.com/Martin-Rancourt/spl-asdlc/compare/v1.2.0...v1.3.0) (2026-10-18)


### Features

* **agents:** one process page, copied into each project and guarded on every PR ([#88](https://github.com/Martin-Rancourt/spl-asdlc/issues/88)) ([56f5045](https://github.com/Martin-Rancourt/spl-asdlc/commit/56f50459d5df72dcf44edc166c38e1c6d10d03a1))
* **bridge:** angle texts follow the project's pinned spl-asdlc version, and a retired angle is reported (SID-1192) ([#89](https://github.com/Martin-Rancourt/spl-asdlc/issues/89)) ([8b0dfc0](https://github.com/Martin-Rancourt/spl-asdlc/commit/8b0dfc0dba2a6e3872951367c06db7a8661b61b0))
* **doctor:** check that the tests a business rule cites exist ([#92](https://github.com/Martin-Rancourt/spl-asdlc/issues/92)) ([bdf3297](https://github.com/Martin-Rancourt/spl-asdlc/commit/bdf3297a16522c7e4039c308f32fd20701146f4b))
* **doctor:** the method checks cannot be turned off, and spl-asdlc's tickets are Done at merge ([#86](https://github.com/Martin-Rancourt/spl-asdlc/issues/86)) ([e3d9a19](https://github.com/Martin-Rancourt/spl-asdlc/commit/e3d9a193545c69a86a24038fc74501cd35ccfcaa))


### Bug Fixes

* **release:** finish a closure the next cycle overtook, then leave its item alone ([#87](https://github.com/Martin-Rancourt/spl-asdlc/issues/87)) ([af92acd](https://github.com/Martin-Rancourt/spl-asdlc/commit/af92acdf863218e038d7745a2313d63f092e3782))
* **required-check:** the method's guards are never skippable, and the rulesets require the title check ([#90](https://github.com/Martin-Rancourt/spl-asdlc/issues/90)) ([6cd7804](https://github.com/Martin-Rancourt/spl-asdlc/commit/6cd780479b9c018e36076e90aa61fdbd5d657b1c))


### Documentation

* **brand:** add the &lt;/&gt; mark and the &lt;asdlc /&gt; wordmark ([#79](https://github.com/Martin-Rancourt/spl-asdlc/issues/79)) ([6b6f3f1](https://github.com/Martin-Rancourt/spl-asdlc/commit/6b6f3f1ce81470eb9b4e299d6c251e2891f1a9e3))
* **development:** how this repository's tickets move, and a CLAUDE.md ([#85](https://github.com/Martin-Rancourt/spl-asdlc/issues/85)) ([4129eb5](https://github.com/Martin-Rancourt/spl-asdlc/commit/4129eb52f03f949cd86627f3f1be583fcc63a206))
* **method:** fix the spec review's gaps the owner approved ([#91](https://github.com/Martin-Rancourt/spl-asdlc/issues/91)) ([f3f9b5e](https://github.com/Martin-Rancourt/spl-asdlc/commit/f3f9b5e6408d88409516419027c30d765c8c33da))


### Tests

* **job:** give each test its own configuration directory ([#93](https://github.com/Martin-Rancourt/spl-asdlc/issues/93)) ([aabb443](https://github.com/Martin-Rancourt/spl-asdlc/commit/aabb443c80b299939451db3ed7316462d66b4000))

## [1.2.0](https://github.com/Martin-Rancourt/spl-asdlc/compare/v1.1.1...v1.2.0) (2026-10-18)


### Features

* **bridge:** pause a project or hold a release's GO without touching its host ([#82](https://github.com/Martin-Rancourt/spl-asdlc/issues/82)) ([34b71bd](https://github.com/Martin-Rancourt/spl-asdlc/commit/34b71bdc80bdf670bb30c928bf6c5af1d57134ae))
* **release:** retract a release whose publication failed before deploying ([#81](https://github.com/Martin-Rancourt/spl-asdlc/issues/81)) ([c160e3c](https://github.com/Martin-Rancourt/spl-asdlc/commit/c160e3cbdfb954fc64e11b0f17bd35f8f90caa6a))
* **release:** the publication reads the gate workflow from the configuration ([#80](https://github.com/Martin-Rancourt/spl-asdlc/issues/80)) ([2564333](https://github.com/Martin-Rancourt/spl-asdlc/commit/2564333443faee271d9a82502a40f2e7a4ce1694))
* **tracker:** create the bridge's items in the configured tracker project ([#76](https://github.com/Martin-Rancourt/spl-asdlc/issues/76)) ([526c178](https://github.com/Martin-Rancourt/spl-asdlc/commit/526c178904dc2310c0e3e5b844ebcf74d09c91fa))


### Bug Fixes

* **release:** find the gate's automatic rerun on the default branch's head ([#77](https://github.com/Martin-Rancourt/spl-asdlc/issues/77)) ([b555114](https://github.com/Martin-Rancourt/spl-asdlc/commit/b555114a548d8ca65d91419f620a4c4a53a39388))
* **tracker:** find the bridge's items even when their description loses its marker ([#83](https://github.com/Martin-Rancourt/spl-asdlc/issues/83)) ([4dcbd6c](https://github.com/Martin-Rancourt/spl-asdlc/commit/4dcbd6cac7ea9eb31aa230bcb7d6ddb41fe029f8))

## [1.1.1](https://github.com/Martin-Rancourt/spl-asdlc/compare/v1.1.0...v1.1.1) (2026-10-16)


### Bug Fixes

* **release:** one note write, In Progress during its cycle, doctor's owner line, end marker ([#74](https://github.com/Martin-Rancourt/spl-asdlc/issues/74)) ([ec6c8ce](https://github.com/Martin-Rancourt/spl-asdlc/commit/ec6c8cedbb1c534e53df0af6811f4894d768265c))

## [1.1.0](https://github.com/Martin-Rancourt/spl-asdlc/compare/v1.0.0...v1.1.0) (2026-10-15)


### Features

* **release:** the release issue is assigned to the project's owner ([#72](https://github.com/Martin-Rancourt/spl-asdlc/issues/72)) ([bca824b](https://github.com/Martin-Rancourt/spl-asdlc/commit/bca824b6060761bed97b260354e5a3120fda7b26))


### Bug Fixes

* **test:** the default-file check passes on Windows ([#71](https://github.com/Martin-Rancourt/spl-asdlc/issues/71)) ([c7a5230](https://github.com/Martin-Rancourt/spl-asdlc/commit/c7a5230518dc13e6bf7fec3cdfb6be009818225a))

## [1.0.0](https://github.com/Martin-Rancourt/spl-asdlc/compare/v0.3.0...v1.0.0) (2026-10-15)


### ⚠ BREAKING CHANGES

* **tick:** a draft PR no longer moves its ticket (put back in draft before its round, on a ticket past the merge, or holding a complete ticket In Progress); `required-check` fails on every draft PR; the release note no longer reads the CHANGELOG for PRs without a ticket.
* `release.registry_path` and `release.business_rule_paths` are removed; a configuration that writes them is refused as an unknown key. A project moves its `docs/business-rules.md` rows into `.spl-asdlc/business-rules/<ID>.md` files.
* **release:** the cycle prepares its version and notes itself, without release-please (SID-1058) ([#64](https://github.com/Martin-Rancourt/spl-asdlc/issues/64))

### Features

* **release:** the cycle prepares its version and notes itself, without release-please (SID-1058) ([#64](https://github.com/Martin-Rancourt/spl-asdlc/issues/64)) ([f0155e1](https://github.com/Martin-Rancourt/spl-asdlc/commit/f0155e1b9b90a777b3a2084ceb43921e73fb6c3b))
* the code and the reference follow the method page ([#69](https://github.com/Martin-Rancourt/spl-asdlc/issues/69)) ([9819488](https://github.com/Martin-Rancourt/spl-asdlc/commit/981948879c635c3fe16e54921edbb75b5f8d0d03))


### Bug Fixes

* **release:** a product's first release is measured from the repository's root ([#63](https://github.com/Martin-Rancourt/spl-asdlc/issues/63)) ([4342944](https://github.com/Martin-Rancourt/spl-asdlc/commit/434294464794fda4bd466bdf1fd3e7b19ae20b8d))
* **release:** the cut reads the method's releases with a token scoped to its repository ([#65](https://github.com/Martin-Rancourt/spl-asdlc/issues/65)) ([f6ebbeb](https://github.com/Martin-Rancourt/spl-asdlc/commit/f6ebbebc12e70d4234c9ece700f3e21ed53b884e))
* the light flow runs alone, and every step can be taken over by hand (SID-1052) ([#59](https://github.com/Martin-Rancourt/spl-asdlc/issues/59)) ([ddb527b](https://github.com/Martin-Rancourt/spl-asdlc/commit/ddb527b08116469fb5ec369ddb472ea8a8c8b4f7))
* **tick:** draft PRs out of the flow, release notes from every PR, clearer texts ([#70](https://github.com/Martin-Rancourt/spl-asdlc/issues/70)) ([f2d9622](https://github.com/Martin-Rancourt/spl-asdlc/commit/f2d9622793054447cb2af65f39d9dc27a088115f))


### Documentation

* approval pass on the specification ([#66](https://github.com/Martin-Rancourt/spl-asdlc/issues/66)) ([441a4dd](https://github.com/Martin-Rancourt/spl-asdlc/commit/441a4ddc18b1a31aacb7be2ca3eab0406dd064e1))
* the method in ten minutes ([#67](https://github.com/Martin-Rancourt/spl-asdlc/issues/67)) ([22ab0b0](https://github.com/Martin-Rancourt/spl-asdlc/commit/22ab0b09427bd0fa8f52d1289d960e53e6766360))
* the method, the owner's second review ([#68](https://github.com/Martin-Rancourt/spl-asdlc/issues/68)) ([50c5ea9](https://github.com/Martin-Rancourt/spl-asdlc/commit/50c5ea97acf5e6b82135d1013ca55e6ac5ca4231))


### Chores

* **release:** merge release/2026-10-11-19-02-utc back into main ([52c0430](https://github.com/Martin-Rancourt/spl-asdlc/commit/52c04305c89e45a9df856fdf91e7812bcd90c57e))


### Continuous Integration

* a staging channel rebuilds the bridge from main for the sandbox (SID-1054) ([#62](https://github.com/Martin-Rancourt/spl-asdlc/issues/62)) ([273a2aa](https://github.com/Martin-Rancourt/spl-asdlc/commit/273a2aadd6d352638f7f904860d1b44a43a88b17))
* develop spl-asdlc with GitHub Flow and release-please on main ([#60](https://github.com/Martin-Rancourt/spl-asdlc/issues/60)) ([82711a8](https://github.com/Martin-Rancourt/spl-asdlc/commit/82711a876443018135fbc030f1aa01e94f85592f))
* **pr:** the latest published release must read this repository's spl-asdlc.toml ([#58](https://github.com/Martin-Rancourt/spl-asdlc/issues/58)) ([a7671ee](https://github.com/Martin-Rancourt/spl-asdlc/commit/a7671eebf05b7a061939a3493dc3c2eb2fc7e6ef))

## [0.3.0](https://github.com/Martin-Rancourt/spl-asdlc/compare/v0.2.0...v0.3.0) (2026-10-11)


### Features

* **config:** this repository runs the light setup ([#54](https://github.com/Martin-Rancourt/spl-asdlc/issues/54)) ([9fb297b](https://github.com/Martin-Rancourt/spl-asdlc/commit/9fb297b4732e8d24b0bd0bc74d142eda88e94f42))


### Bug Fixes

* trial 2 fixes, in one PR (SID-1041) ([#50](https://github.com/Martin-Rancourt/spl-asdlc/issues/50)) ([088cf30](https://github.com/Martin-Rancourt/spl-asdlc/commit/088cf30f8aaa5f1961cd1c472fd30e68e642138f))


### Documentation

* **setup:** a light configuration template by default, with the steps to add each layer (SID-1017) ([#55](https://github.com/Martin-Rancourt/spl-asdlc/issues/55)) ([e9533f3](https://github.com/Martin-Rancourt/spl-asdlc/commit/e9533f3f54417907351d498773b502e838c6c9d2))


### Chores

* **release:** merge release/2026-10-10-16-54-utc back into main ([8c9b770](https://github.com/Martin-Rancourt/spl-asdlc/commit/8c9b770ad6640a4597fd23319bbd2c61247d92db))

## [0.2.0](https://github.com/Martin-Rancourt/spl-asdlc/compare/v0.1.0...v0.2.0) (2026-10-11)


### Features

* **ci:** pr-title, pr-guards and the required-check aggregator (step 3, lot 1) ([49c2dd0](https://github.com/Martin-Rancourt/spl-asdlc/commit/49c2dd0918337828d496f21d540662a248b89f2b))
* **ci:** pr-title, pr-guards and the required-check aggregator (step 3, lot 1) ([a7d1ce3](https://github.com/Martin-Rancourt/spl-asdlc/commit/a7d1ce335161b4b8fe02e9a13c0239cab246d246))
* **ci:** release-cut, release-gate and its auto-rerun (step 3, lot 2) ([4af329a](https://github.com/Martin-Rancourt/spl-asdlc/commit/4af329af46dc028e0f56062b67709c52042c07e5))
* **ci:** release-cut, release-gate, release-publish (step 3, lots 2 and 3) ([7875f4e](https://github.com/Martin-Rancourt/spl-asdlc/commit/7875f4e24120110e68fbc7ae977411067b620dd6))
* **ci:** release-publish in three stages, verify, record-deployed, close (step 3, lot 3) ([5c6e7c5](https://github.com/Martin-Rancourt/spl-asdlc/commit/5c6e7c52ba95cc4c3a1385c92199f4b0614c1693))
* **closure:** archive the items the cycle resolved (SID-978) ([#34](https://github.com/Martin-Rancourt/spl-asdlc/issues/34)) ([8b246ca](https://github.com/Martin-Rancourt/spl-asdlc/commit/8b246ca07371344f2c10084ca7b36cbe2c8029aa))
* **closure:** unassign a ticket the closure reopens (SID-896) ([a3e8028](https://github.com/Martin-Rancourt/spl-asdlc/commit/a3e802837cf2050e1f64e23c4a7d32ca19221d36))
* **closure:** unassign a ticket the closure reopens (SID-896) ([fe4d967](https://github.com/Martin-Rancourt/spl-asdlc/commit/fe4d9675fe2e6bf3a0e94654c347b3bf88a0dec7))
* **config:** a project may have no release cycle; a merged ticket is Done (SID-1015) ([#42](https://github.com/Martin-Rancourt/spl-asdlc/issues/42)) ([c8f60c2](https://github.com/Martin-Rancourt/spl-asdlc/commit/c8f60c2a0fd30230b5fb3c95c8aa67d1133b73b4))
* **config:** merge an instance overlay kept outside the repository ([37369c3](https://github.com/Martin-Rancourt/spl-asdlc/commit/37369c396c477464ed54e467462a9611ce47779a))
* **config:** merge an instance overlay kept outside the repository ([622effa](https://github.com/Martin-Rancourt/spl-asdlc/commit/622effada1d76200c5db8dec32f90ec9288c173e))
* **config:** name the coordinator who receives the consolidation item (SID-959) ([#35](https://github.com/Martin-Rancourt/spl-asdlc/issues/35)) ([2f2863a](https://github.com/Martin-Rancourt/spl-asdlc/commit/2f2863ac48a16df190eb73aa271864d06b6b9f14))
* **doctor:** report every ruleset the repository procedure requires (SID-957) ([#29](https://github.com/Martin-Rancourt/spl-asdlc/issues/29)) ([a659470](https://github.com/Martin-Rancourt/spl-asdlc/commit/a659470bb87b54814dd411b5b1a6a0d112e46357))
* **release:** a red gate opens an urgent gate-fix ticket; the validation sub-task is urgent (SID-976) ([#31](https://github.com/Martin-Rancourt/spl-asdlc/issues/31)) ([0dad7b4](https://github.com/Martin-Rancourt/spl-asdlc/commit/0dad7b480521969d4b1efd809109ddb3c2e43781))
* **release:** a release may have no QA angle; the GO is the gate (SID-1016) ([#41](https://github.com/Martin-Rancourt/spl-asdlc/issues/41)) ([83bc898](https://github.com/Martin-Rancourt/spl-asdlc/commit/83bc8982c2b03d37d054d1604e6392ecadf756fd))
* **release:** angle items in the normal flow, consolidation as the overall review (SID-980) ([#36](https://github.com/Martin-Rancourt/spl-asdlc/issues/36)) ([47fe4c2](https://github.com/Martin-Rancourt/spl-asdlc/commit/47fe4c26b4e2e6548665d21aade8a4a949ddbc5b))
* **review:** a project may configure no PR reviewer; the bridge merges on CI (SID-1014) ([#40](https://github.com/Martin-Rancourt/spl-asdlc/issues/40)) ([1591129](https://github.com/Martin-Rancourt/spl-asdlc/commit/1591129cc5d4cbd3dc837848e18f22bd76eee899))
* **texts:** embed the method's protocol in every review and consolidation item (SID-958) ([#30](https://github.com/Martin-Rancourt/spl-asdlc/issues/30)) ([3135b75](https://github.com/Martin-Rancourt/spl-asdlc/commit/3135b75e210d9280348652d38402fde4b150a96d))
* this repository's own spl-asdlc.toml, neutral ([60dc456](https://github.com/Martin-Rancourt/spl-asdlc/commit/60dc4563303c7bd66f85500d53a325f3c06b1389))
* this repository's own spl-asdlc.toml, neutral ([ef86dd3](https://github.com/Martin-Rancourt/spl-asdlc/commit/ef86dd3347c96cbda2b68d92aad3ef75fd8396d7))
* **tick:** one PR per ticket by default, the state precedence, a round scoped to its PR (SID-891) ([ea69278](https://github.com/Martin-Rancourt/spl-asdlc/commit/ea69278028aa0d68bf8eb393514bc8245d02fe81))
* **tick:** Post Deploy validation sub-task; Done once every box is checked (SID-887) ([4bf4a34](https://github.com/Martin-Rancourt/spl-asdlc/commit/4bf4a340ed2be7bf135934fef6d403fba95485e6))
* **tick:** post-deploy validation, Post Deploy to Done at the tick ([f9033f0](https://github.com/Martin-Rancourt/spl-asdlc/commit/f9033f0f59c1fe2beba98427b2787f97a609a8c1))
* **tick:** several PRs per ticket, rounds scoped to their PR, state rules in order ([1f4b911](https://github.com/Martin-Rancourt/spl-asdlc/commit/1f4b911d9df9bdf17d8daeb5398cfe6f76bd57b3))
* **tick:** the nits work carries a full contract, to triage; grouping per tracker (SID-901) ([6f75773](https://github.com/Martin-Rancourt/spl-asdlc/commit/6f75773a9208942ca9ca2c9f3b72d6f3eaa4b3fa))
* **tick:** the nits work carries a full contract, to triage; grouping per tracker (SID-901) ([4479e9c](https://github.com/Martin-Rancourt/spl-asdlc/commit/4479e9cca8728fc5d8a199f79e624a849dc1e855))
* **tick:** the release round and brief wait for a green gate ([a464171](https://github.com/Martin-Rancourt/spl-asdlc/commit/a4641712bd469be3855b48f3e03db08d12cac4bc))
* **tick:** the release round and brief wait for a green gate (SID-888) ([6d34775](https://github.com/Martin-Rancourt/spl-asdlc/commit/6d347753e90c4df5db086504625a05d6a2279811))


### Bug Fixes

* **ci:** the review of step 3, lot 1 ([f256d8e](https://github.com/Martin-Rancourt/spl-asdlc/commit/f256d8eb549ac7f9704aca0de35a2b72b6773345))
* **ci:** the review of step 3, lots 2 and 3 ([6c1bbff](https://github.com/Martin-Rancourt/spl-asdlc/commit/6c1bbffdcd87c7ed15b6cc491b64ebd1c18e0e01))
* **config:** keep this repository's own file readable by the installed bridge ([2b7734d](https://github.com/Martin-Rancourt/spl-asdlc/commit/2b7734d2360b1d1080423a8e27b14b6cc724f2e8))
* **config:** one overlay per configuration; an odd sibling is an error ([1a95dd7](https://github.com/Martin-Rancourt/spl-asdlc/commit/1a95dd719c937fa6a8bf5cc474f0ce85a2c9ceac))
* **config:** this repository's own file stays readable by the installed bridge ([c87cf26](https://github.com/Martin-Rancourt/spl-asdlc/commit/c87cf26b5e5956f23b203a6a3c6541b828294f9e))
* **qa:** associate security report with SID-1027 ([1c0a685](https://github.com/Martin-Rancourt/spl-asdlc/commit/1c0a6852f2d91f0a3894ef5cf57620bca20264d1))
* **qa:** the report names its angle sub-item, not the reviewer (BR-ASDLC-001) ([0aea3bf](https://github.com/Martin-Rancourt/spl-asdlc/commit/0aea3bfd6f0a94b374bd42447ab0f27a403f8ddb))
* **review:** recognise an App's login however it is spelt; say why a PR waits (SID-1008) ([#39](https://github.com/Martin-Rancourt/spl-asdlc/issues/39)) ([a82910f](https://github.com/Martin-Rancourt/spl-asdlc/commit/a82910fabe07066c0c60644ba5fd59d8eae4a83a))
* **rule-zero:** allow this product's repository slug, as BR-ASDLC-001 says ([4a5ad31](https://github.com/Martin-Rancourt/spl-asdlc/commit/4a5ad31ed67c083f62aa04b0ef0261775e7ec7c1))
* **rule-zero:** allow this product's repository slug, as BR-ASDLC-001 says ([e31a07c](https://github.com/Martin-Rancourt/spl-asdlc/commit/e31a07c91e51ef10c63d68d36e98f37c60772f2c))
* **security:** validate redirects before following ([bcca257](https://github.com/Martin-Rancourt/spl-asdlc/commit/bcca25777f720fa2175acaf8768a4043d447fdc7))
* **security:** validate redirects before following (SID-1023) ([9e2117f](https://github.com/Martin-Rancourt/spl-asdlc/commit/9e2117fcd35ff4deaace63319c80508fdb5a9bb5))
* **tick:** close a multi-component release; the gate record is no tip gap ([163949a](https://github.com/Martin-Rancourt/spl-asdlc/commit/163949ac96a1530a9136eea6142278edf83ea51b))
* **tick:** close a multi-component release; the gate record is no tip gap ([9103b83](https://github.com/Martin-Rancourt/spl-asdlc/commit/9103b83b3ddd9f300adc8be41b595de2a2761bac))
* **tick:** the review of SID-887 ([6953723](https://github.com/Martin-Rancourt/spl-asdlc/commit/6953723ed462e690aed5fcfec593510b750fb365))
* **tick:** the review of SID-891 ([2feedac](https://github.com/Martin-Rancourt/spl-asdlc/commit/2feedac897e32694b732732d42f48f63a2d8104a))


### Documentation

* **livela:** the arrow goes from Livela to ASDLC; transitions ignore locks ([cc5b89f](https://github.com/Martin-Rancourt/spl-asdlc/commit/cc5b89f6bb86465e351842f7793c31b5610c20ab))
* **livela:** the arrow goes from Livela to ASDLC; transitions ignore locks (SID-898, SID-899) ([b7d5647](https://github.com/Martin-Rancourt/spl-asdlc/commit/b7d56472bda4d0427895ae24139d6cc3ccb055cb))
* name this repository's CI workflows in the README and BR-ASDLC-001 ([b05b3ba](https://github.com/Martin-Rancourt/spl-asdlc/commit/b05b3baafd8abfdc07da8f105ba1885aec493066))
* say where the instance overlay lives on the bridge's host ([36fa8b3](https://github.com/Martin-Rancourt/spl-asdlc/commit/36fa8b346ab6f180b85797dcfb2acf73a03e67e8))
* **setup:** add the project templates, the example callers and this repository's release files ([1a8ce1d](https://github.com/Martin-Rancourt/spl-asdlc/commit/1a8ce1da84fcbc061eed319ef8a853b43d2323c9))
* **setup:** add the project templates, the example callers and this repository's release files ([95361fa](https://github.com/Martin-Rancourt/spl-asdlc/commit/95361fa519de906d7e4544920469cbad7f2daabe))
* **setup:** the bridge runs a published release and reads a local configuration (SID-986) ([#37](https://github.com/Martin-Rancourt/spl-asdlc/issues/37)) ([a1994d1](https://github.com/Martin-Rancourt/spl-asdlc/commit/a1994d1388cd4ec946756f4208de497ce37e8251))
* **spec:** the closure judges a ticket for the release that ships its latest PR ([8e83cb9](https://github.com/Martin-Rancourt/spl-asdlc/commit/8e83cb987d152cdd9b6d943f46bddb8cad7b3897))
* **spec:** the run journal follows the order of SID-888 ([1afcf29](https://github.com/Martin-Rancourt/spl-asdlc/commit/1afcf29c0762f834b220179637bf688feabb887e))


### Style

* rustfmt ([d3ef145](https://github.com/Martin-Rancourt/spl-asdlc/commit/d3ef1458b164f943c377d0d6e07fd81b5285ed32))


### Chores

* merge origin/main into feat/nits-contract ([0b81edd](https://github.com/Martin-Rancourt/spl-asdlc/commit/0b81eddc153432bd36243b538888a94a8de7841f))
* **qa:** add security report for v0.2.0-rc.2 ([215843a](https://github.com/Martin-Rancourt/spl-asdlc/commit/215843aae1ce852af84711e12a8c2f82b969429c))
* **qa:** associate security report with SID-1029 ([04fb0fe](https://github.com/Martin-Rancourt/spl-asdlc/commit/04fb0fef4194207094eee9412e3877680cb451f5))
* **qa:** security report for v0.2.0-rc.2 (SID-1029) ([069c2f6](https://github.com/Martin-Rancourt/spl-asdlc/commit/069c2f6e0941bef98eed05b1314269e907b13f33))


### Refactoring

* **ci:** name the reusable workflows asdlc-* ([2b9491b](https://github.com/Martin-Rancourt/spl-asdlc/commit/2b9491b8c5889179f6100f009721ae9bf1aea164))
* **config:** remove the tracker status mapping (SID-977) ([#32](https://github.com/Martin-Rancourt/spl-asdlc/issues/32)) ([a0cd61b](https://github.com/Martin-Rancourt/spl-asdlc/commit/a0cd61b308f5630f0de4ff1802a2f581da63b38d))


### Tests

* **ci:** stub commands for the workflow tests ([8468d35](https://github.com/Martin-Rancourt/spl-asdlc/commit/8468d3525fac57a3acf35b9e66294ebb8b55511f))


### Continuous Integration

* **binaries:** build the Intel macOS binary on macos-15-intel ([d6ef616](https://github.com/Martin-Rancourt/spl-asdlc/commit/d6ef616bd3cebe22b8082bf0d1128640f9ef989e))
* **release:** the v0.2.0 publication allows rc.2's own QA report ([#52](https://github.com/Martin-Rancourt/spl-asdlc/issues/52)) ([75abe3c](https://github.com/Martin-Rancourt/spl-asdlc/commit/75abe3c412b42f3c6ff93c3cbe85135c8ffc6086))
* run the checks on Linux only; macOS and Windows at version tags ([1f6990d](https://github.com/Martin-Rancourt/spl-asdlc/commit/1f6990db0f43aafc434e68a5735e1663bb998509))
* run the checks on Linux only; macOS and Windows at version tags ([40f4618](https://github.com/Martin-Rancourt/spl-asdlc/commit/40f46185ae536381538fee051edf6e3b4477f339))
* run this repository's CI on its own reusable workflows ([35a8cde](https://github.com/Martin-Rancourt/spl-asdlc/commit/35a8cde82342bdbb9cb6420c41b78be9202d31d9))
* run this repository's CI on its own reusable workflows ([c45de1b](https://github.com/Martin-Rancourt/spl-asdlc/commit/c45de1b2f5798ded28da0d07ed5916bbb22205ea))
* the Rust cache never cleans the runner's cargo bin directory (SID-987) ([#38](https://github.com/Martin-Rancourt/spl-asdlc/issues/38)) ([43ab28a](https://github.com/Martin-Rancourt/spl-asdlc/commit/43ab28a14b4a2f3f326f447bb61b50faa1b6475a))
